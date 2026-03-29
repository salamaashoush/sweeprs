use anyhow::{Result, anyhow};
use sysinfo::Disks;

use crate::scanner::cli_cache;
use crate::scanner::entry::{DiskInfo, VolumeInfo};
use crate::scanner::walker;

/// Lightweight disk usage query for the monitor daemon.
///
/// Takes a pre-existing `&Disks` reference (caller owns it, refreshes it each cycle)
/// and returns `(usage_percent, available_bytes)` for the root volume.
/// No CLI commands are spawned -- avoids triggering `CLI_CACHE` or `get_purgeable_bytes`.
pub fn get_disk_usage(disks: &Disks) -> Option<(f64, u64)> {
    let root = disks
        .iter()
        .find(|d| d.mount_point() == std::path::Path::new("/"))?;

    let total = root.total_space();
    let available = root.available_space();
    let used = total.saturating_sub(available);
    let pct = if total > 0 {
        (used as f64 / total as f64) * 100.0
    } else {
        0.0
    };
    Some((pct, available))
}

/// Get disk info with all the expensive supplementary data (iCloud, system/app size, etc.)
/// Used for full scans and TUI where the extra detail is displayed.
pub fn get_disk_info() -> Result<DiskInfo> {
    get_disk_info_inner(true)
}

/// Get basic disk info quickly (usage %, free space, purgeable, snapshots).
/// Skips expensive operations like walking /Applications and iCloud dirs.
/// Used for category scans and monitor where speed matters.
pub fn get_disk_info_fast() -> Result<DiskInfo> {
    get_disk_info_inner(false)
}

fn get_disk_info_inner(full: bool) -> Result<DiskInfo> {
    let disks = Disks::new_with_refreshed_list();

    let root_disk = disks
        .iter()
        .find(|d| d.mount_point() == std::path::Path::new("/"))
        .ok_or_else(|| anyhow!("Could not find root disk"))?;

    let total = root_disk.total_space();
    let available = root_disk.available_space();
    let used = total.saturating_sub(available);
    let usage_percent = if total > 0 {
        (used as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    let purgeable_bytes = get_purgeable_bytes(available);
    let snapshot_bytes = parse_snapshot_bytes();

    // Expensive operations only for full disk info
    let (icloud_local_bytes, system_app_bytes, other_volumes, tm_reclaimable_bytes) = if full {
        (
            get_icloud_local_bytes(),
            get_system_app_bytes(),
            get_other_volumes_from(&disks),
            get_tm_reclaimable_bytes(),
        )
    } else {
        (None, None, Vec::new(), None)
    };

    Ok(DiskInfo {
        name: root_disk.name().to_string_lossy().into_owned(),
        mount_point: "/".to_owned(),
        total_bytes: total,
        available_bytes: available,
        used_bytes: used,
        usage_percent,
        purgeable_bytes,
        snapshot_bytes,
        icloud_local_bytes,
        system_app_bytes,
        other_volumes,
        tm_reclaimable_bytes,
    })
}

/// Query purgeable space on APFS volumes via `diskutil info -plist /`.
///
/// Purgeable space = `available_bytes` (which includes purgeable) - `APFSContainerFree` (physical free).
/// Returns None on non-APFS volumes or parse failure.
/// Uses the prefetched CLI cache when available, falls back to spawning the command.
fn get_purgeable_bytes(available_bytes: u64) -> Option<u64> {
    let xml = if let Some(cached) = cli_cache::get("diskutil_info_root") {
        cached.stdout.clone()
    } else {
        let output = std::process::Command::new("diskutil")
            .args(["info", "-plist", "/"])
            .output()
            .ok()?;
        if !output.status.success() {
            return None;
        }
        String::from_utf8_lossy(&output.stdout).to_string()
    };

    let container_free = parse_plist_integer(&xml, "APFSContainerFree")?;

    // available includes purgeable; container_free is physical free
    available_bytes.checked_sub(container_free)
}

/// Parse total APFS snapshot size from `diskutil apfs list` output.
///
/// Looks for lines containing `com.apple.TimeMachine` snapshot names,
/// then grabs the next `Snapshot Disk Size:` line and extracts the byte count
/// from the parenthesized `(NNNN Bytes)` value.
///
/// Used by both `DiskInfo` and the Time Machine snapshots rule.
pub fn parse_snapshot_bytes() -> u64 {
    let Some(result) = cli_cache::get("diskutil_apfs_list") else {
        return 0;
    };
    parse_snapshot_bytes_from_output(&result.stdout)
}

/// Parse snapshot bytes from raw `diskutil apfs list` output text.
pub fn parse_snapshot_bytes_from_output(output: &str) -> u64 {
    let mut total = 0u64;
    let lines: Vec<&str> = output.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("Snapshot Name:") && line.contains("com.apple.TimeMachine") {
            // Scan forward for the next "Snapshot Disk Size:" line
            for following in &lines[i + 1..] {
                if following.contains("Snapshot Disk Size:") {
                    // Extract byte count from pattern like "(123456789 Bytes)"
                    if let Some(start) = following.find('(') {
                        if let Some(end) = following[start..].find(" Bytes)") {
                            if let Ok(bytes) =
                                following[start + 1..start + end].trim().parse::<u64>()
                            {
                                total += bytes;
                            }
                        }
                    }
                    break;
                }
                // Stop if we hit another snapshot
                if following.contains("Snapshot Name:") {
                    break;
                }
            }
        }
    }
    total
}

/// Measure local iCloud Drive cache size.
///
/// iCloud stores locally cached files in `~/Library/Mobile Documents/com~apple~CloudDocs/`
/// and metadata in `~/Library/Application Support/CloudDocs/`. These can be evicted by the OS.
fn get_icloud_local_bytes() -> Option<u64> {
    let home = dirs::home_dir()?;
    let cloud_docs = home.join("Library/Mobile Documents/com~apple~CloudDocs");
    if !cloud_docs.exists() {
        return None;
    }
    let size = walker::dir_size(&cloud_docs);
    // Also include the CloudDocs metadata/session cache
    let meta_dir = home.join("Library/Application Support/CloudDocs");
    let meta_size = if meta_dir.exists() {
        walker::dir_size(&meta_dir)
    } else {
        0
    };
    let total = size + meta_size;
    if total > 0 { Some(total) } else { None }
}

/// Estimate system + application install size.
///
/// Measures `/Applications` size for installed apps. `/System` is the sealed
/// read-only system volume -- we use a fixed estimate rather than walking it
/// (which would take 30+ seconds and the user can't reclaim any of it anyway).
fn get_system_app_bytes() -> Option<u64> {
    let apps_dir = std::path::Path::new("/Applications");
    let apps_size = if apps_dir.exists() {
        walker::dir_size(apps_dir)
    } else {
        0
    };

    // /System is the sealed read-only APFS volume. Walking it recursively is
    // extremely slow (30+ seconds) and the user cannot reclaim any of it.
    // Use a conservative 12 GB estimate (typical macOS system volume).
    let system_size: u64 = 12_884_901_888; // 12 GiB

    let total = apps_size + system_size;
    if total > 0 { Some(total) } else { None }
}

/// Simple plist integer parser: finds `<key>KEY</key>` followed by `<integer>N</integer>`.
fn parse_plist_integer(xml: &str, key: &str) -> Option<u64> {
    let key_tag = format!("<key>{key}</key>");
    let key_pos = xml.find(&key_tag)?;
    let after_key = &xml[key_pos + key_tag.len()..];
    let int_start = after_key.find("<integer>")? + "<integer>".len();
    let int_end = after_key[int_start..].find("</integer>")?;
    after_key[int_start..int_start + int_end]
        .trim()
        .parse()
        .ok()
}

/// Query non-root mounted volumes from an already-refreshed `Disks` instance.
fn get_other_volumes_from(disks: &Disks) -> Vec<VolumeInfo> {
    disks
        .iter()
        .filter(|d| d.mount_point() != std::path::Path::new("/"))
        .filter(|d| {
            let mp = d.mount_point().to_string_lossy();
            // Skip system volumes and virtual filesystems
            !mp.starts_with("/System")
                && !mp.starts_with("/private")
                && mp != "/dev"
                && d.total_space() > 0
        })
        .map(|d| {
            let total = d.total_space();
            let available = d.available_space();
            let used = total.saturating_sub(available);
            VolumeInfo {
                name: d.name().to_string_lossy().into_owned(),
                mount_point: d.mount_point().to_string_lossy().into_owned(),
                total_bytes: total,
                available_bytes: available,
                used_bytes: used,
                usage_percent: if total > 0 {
                    (used as f64 / total as f64) * 100.0
                } else {
                    0.0
                },
            }
        })
        .collect()
}

/// Estimate bytes reclaimable by deleting old Time Machine local snapshots.
///
/// Parses the `tmutil_snapshots` CLI cache output to count snapshots, then
/// uses the already-parsed `snapshot_bytes` to estimate what could be reclaimed
/// by deleting all but the most recent snapshot.
pub fn get_tm_reclaimable_bytes() -> Option<u64> {
    let result = cli_cache::get("tmutil_snapshots")?;
    let snapshots: Vec<&str> = result
        .stdout
        .lines()
        .filter(|l| l.contains("com.apple."))
        .collect();

    if snapshots.len() <= 1 {
        return None; // Only 1 or 0 snapshots, nothing to reclaim
    }

    // Total snapshot size divided proportionally by (n-1)/n
    // (rough estimate: keep newest, delete rest)
    let total = parse_snapshot_bytes();
    if total == 0 {
        return None;
    }

    let reclaimable = total * (snapshots.len() as u64 - 1) / snapshots.len() as u64;
    Some(reclaimable)
}
