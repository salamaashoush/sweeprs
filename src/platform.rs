use anyhow::{Result, anyhow};
use sysinfo::Disks;

use crate::scanner::cli_cache;
use crate::scanner::entry::DiskInfo;

pub fn get_disk_info() -> Result<DiskInfo> {
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

    Ok(DiskInfo {
        name: root_disk.name().to_string_lossy().into_owned(),
        mount_point: "/".to_owned(),
        total_bytes: total,
        available_bytes: available,
        used_bytes: used,
        usage_percent,
        purgeable_bytes,
        snapshot_bytes,
    })
}

/// Query purgeable space on APFS volumes via `diskutil info -plist /`.
///
/// Purgeable space = `available_bytes` (which includes purgeable) - `APFSContainerFree` (physical free).
/// Returns None on non-APFS volumes or parse failure.
fn get_purgeable_bytes(available_bytes: u64) -> Option<u64> {
    let output = std::process::Command::new("diskutil")
        .args(["info", "-plist", "/"])
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let xml = String::from_utf8_lossy(&output.stdout);
    let container_free = parse_plist_integer(&xml, "APFSContainerFree")?;

    // available includes purgeable; container_free is physical free
    available_bytes.checked_sub(container_free)
}

/// Parse total APFS snapshot size from `diskutil apfs list` output.
///
/// Looks for lines containing `com.apple.TimeMachine` snapshot names,
/// then grabs the next `Snapshot Disk Size:` line and extracts the byte count
/// from the parenthesized `(NNNN Bytes)` value.
fn parse_snapshot_bytes() -> u64 {
    let Some(result) = cli_cache::get("diskutil_apfs_list") else {
        return 0;
    };

    let mut total = 0u64;
    let lines: Vec<&str> = result.stdout.lines().collect();
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
