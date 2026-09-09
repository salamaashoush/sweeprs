use anyhow::{Result, anyhow};
use sysinfo::Disks;

#[cfg(target_os = "macos")]
use crate::scanner::cli_cache;
use crate::scanner::entry::{DiskInfo, VolumeInfo};
#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "linux")]
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

    let other_volumes = if full {
        get_other_volumes_from(&disks)
    } else {
        Vec::new()
    };

    Ok(DiskInfo {
        name: root_disk.name().to_string_lossy().into_owned(),
        mount_point: "/".to_owned(),
        total_bytes: total,
        available_bytes: available,
        used_bytes: used,
        usage_percent,
        purgeable_bytes: None,
        snapshot_bytes: 0,
        icloud_local_bytes: None,
        system_app_bytes: None,
        other_volumes,
        tm_reclaimable_bytes: None,
    })
}

// --- macOS-specific helpers ---

#[cfg(target_os = "macos")]
fn get_purgeable_bytes(available_bytes: u64) -> Option<u64> {
    let xml = read_root_plist()?;

    let container_free = parse_plist_integer(&xml, "APFSContainerFree")?;
    available_bytes.checked_sub(container_free)
}

#[cfg(target_os = "macos")]
pub fn parse_snapshot_bytes() -> u64 {
    let Some(result) = cli_cache::get("diskutil_apfs_list") else {
        return 0;
    };
    parse_snapshot_bytes_from_output(&result.stdout)
}

#[cfg(target_os = "macos")]
pub fn parse_snapshot_bytes_from_output(output: &str) -> u64 {
    let mut total = 0u64;
    let lines: Vec<&str> = output.lines().collect();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("Snapshot Name:") && line.contains("com.apple.TimeMachine") {
            for following in &lines[i + 1..] {
                if following.contains("Snapshot Disk Size:") {
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
                if following.contains("Snapshot Name:") {
                    break;
                }
            }
        }
    }
    total
}

#[cfg(target_os = "macos")]
fn get_icloud_local_bytes() -> Option<u64> {
    let home = dirs::home_dir()?;
    let cloud_docs = home.join("Library/Mobile Documents/com~apple~CloudDocs");
    if !cloud_docs.exists() {
        return None;
    }
    let size = walker::dir_size(&cloud_docs);
    let meta_dir = home.join("Library/Application Support/CloudDocs");
    let meta_size = if meta_dir.exists() {
        walker::dir_size(&meta_dir)
    } else {
        0
    };
    let total = size + meta_size;
    if total > 0 { Some(total) } else { None }
}

/// Bytes held by the sealed system volume plus installed applications.
///
/// The system half is read from the volume itself. It used to be a hard-coded
/// 12 GiB, which is a different number on every macOS release and on every
/// machine, and quoting it back to the user as measured disk usage was simply
/// wrong. When the volume cannot be read the system half is left out rather
/// than guessed.
#[cfg(target_os = "macos")]
fn get_system_app_bytes() -> Option<u64> {
    let apps_dir = std::path::Path::new("/Applications");
    let apps_size = if apps_dir.exists() {
        walker::dir_size(apps_dir)
    } else {
        0
    };

    let system_size = read_root_plist()
        .as_deref()
        .and_then(|xml| parse_plist_integer(xml, "CapacityInUse"))
        .unwrap_or(0);

    let total = apps_size + system_size;
    if total > 0 { Some(total) } else { None }
}

/// `diskutil info -plist /`, from the prefetch cache when it ran.
#[cfg(target_os = "macos")]
fn read_root_plist() -> Option<String> {
    if let Some(cached) = cli_cache::get("diskutil_info_root") {
        return Some(cached.stdout.clone());
    }
    let output = std::process::Command::new("diskutil")
        .args(["info", "-plist", "/"])
        .output()
        .ok()?;
    output
        .status
        .success()
        .then(|| String::from_utf8_lossy(&output.stdout).to_string())
}

#[cfg(target_os = "macos")]
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

#[cfg(target_os = "macos")]
pub fn get_tm_reclaimable_bytes() -> Option<u64> {
    let result = cli_cache::get("tmutil_snapshots")?;
    let snapshots: Vec<&str> = result
        .stdout
        .lines()
        .filter(|l| l.contains("com.apple."))
        .collect();

    if snapshots.len() <= 1 {
        return None;
    }

    let total = parse_snapshot_bytes();
    if total == 0 {
        return None;
    }

    let reclaimable = total * (snapshots.len() as u64 - 1) / snapshots.len() as u64;
    Some(reclaimable)
}

// --- Volume listing (cross-platform with platform-specific filters) ---

fn get_other_volumes_from(disks: &Disks) -> Vec<VolumeInfo> {
    disks
        .iter()
        .filter(|d| d.mount_point() != std::path::Path::new("/"))
        .filter(|d| {
            let mp = d.mount_point().to_string_lossy();
            if cfg!(target_os = "macos") {
                !mp.starts_with("/System")
                    && !mp.starts_with("/private")
                    && mp != "/dev"
                    && d.total_space() > 0
            } else {
                // Linux: skip virtual filesystems
                !mp.starts_with("/sys")
                    && !mp.starts_with("/proc")
                    && !mp.starts_with("/dev")
                    && !mp.starts_with("/run")
                    && !mp.starts_with("/snap")
                    && d.total_space() > 0
            }
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
