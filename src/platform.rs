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

    // The two walks and the two prefetched commands do not depend on each
    // other; run one after another they cost their sum.
    let (
        purgeable_bytes,
        snapshot_bytes,
        icloud_local_bytes,
        system_app_bytes,
        tm_reclaimable_bytes,
    ) = std::thread::scope(|s| {
        let icloud = full.then(|| s.spawn(get_icloud_local_bytes));
        let apps = full.then(|| s.spawn(get_system_app_bytes));
        let purgeable = s.spawn(|| get_purgeable_bytes(available));
        let snapshots = s.spawn(parse_snapshot_bytes);
        let tm = if full {
            get_tm_reclaimable_bytes()
        } else {
            None
        };
        let joined = |handle: Option<std::thread::ScopedJoinHandle<'_, Option<u64>>>| {
            handle.and_then(|h| h.join().ok().flatten())
        };
        (
            purgeable.join().ok().flatten(),
            snapshots.join().unwrap_or(0),
            joined(icloud),
            joined(apps),
            tm,
        )
    });
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
                    if let Some(start) = following.find('(')
                        && let Some(end) = following[start..].find(" Bytes)")
                        && let Ok(bytes) = following[start + 1..start + end].trim().parse::<u64>()
                    {
                        total += bytes;
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
        let cache = dirs::cache_dir().map(|d| d.join("sweeprs").join("applications-v2.json"));
        app_bundles::total_size(apps_dir, cache.as_deref())
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

/// The size of `/Applications`, remembered per bundle between scans.
///
/// Walking every bundle is the slowest part of a scan, seconds on a machine
/// with a few large apps, and only feeds the "system + apps" line. A bundle is
/// measured again when its own, its `Contents` or its `Info.plist`
/// modification time moves, which an install or an update does, and every
/// bundle at least daily for the updaters that patch a bundle in place.
#[cfg(target_os = "macos")]
mod app_bundles {
    use std::path::{Path, PathBuf};
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use rayon::prelude::*;
    use rustc_hash::FxHashMap;
    use serde::{Deserialize, Serialize};

    use crate::scanner::walker;

    const MAX_AGE: Duration = Duration::from_hours(24);

    #[derive(Serialize, Deserialize, Clone, Copy)]
    struct Measured {
        stamp: u64,
        taken_at: u64,
        size: u64,
    }

    fn secs(time: SystemTime) -> u64 {
        time.duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs())
    }

    fn stamp(bundle: &Path) -> u64 {
        [
            bundle.to_path_buf(),
            bundle.join("Contents"),
            bundle.join("Contents/Info.plist"),
        ]
        .iter()
        .filter_map(|p| p.symlink_metadata().and_then(|m| m.modified()).ok())
        .map(secs)
        .max()
        .unwrap_or(0)
    }

    pub fn total_size(apps_dir: &Path, cache: Option<&Path>) -> u64 {
        let previous: FxHashMap<PathBuf, Measured> = cache
            .and_then(|c| std::fs::read_to_string(c).ok())
            .and_then(|json| serde_json::from_str(&json).ok())
            .unwrap_or_default();
        let now = secs(SystemTime::now());

        let Ok(read_dir) = std::fs::read_dir(apps_dir) else {
            return 0;
        };
        let bundles: Vec<PathBuf> = read_dir.flatten().map(|e| e.path()).collect();
        let current: FxHashMap<PathBuf, Measured> = bundles
            .into_par_iter()
            .map(|bundle| {
                let stamp = stamp(&bundle);
                let fresh = previous.get(&bundle).copied().filter(|m| {
                    m.stamp == stamp && now.saturating_sub(m.taken_at) < MAX_AGE.as_secs()
                });
                let measured = fresh.unwrap_or_else(|| Measured {
                    stamp,
                    taken_at: now,
                    size: if bundle.is_dir() {
                        walker::dir_size_uncached(&bundle)
                    } else {
                        walker::file_size(&bundle)
                    },
                });
                (bundle, measured)
            })
            .collect();

        if let Some(cache) = cache
            && let Ok(json) = serde_json::to_string(&current)
        {
            if let Some(parent) = cache.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::write(cache, json);
        }
        current.values().map(|m| m.size).sum()
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn an_unchanged_bundle_is_not_measured_again_and_a_changed_one_is() {
            let tmp = tempfile::tempdir().expect("tempdir");
            let apps = tmp.path().join("Applications");
            let cache = tmp.path().join("applications-v2.json");
            std::fs::create_dir_all(apps.join("Kept.app/Contents")).expect("bundle");
            std::fs::write(apps.join("Kept.app/Contents/Info.plist"), vec![0u8; 8192])
                .expect("plist");

            let first = total_size(&apps, Some(&cache));
            assert!(first > 0);

            // A stale record for an unchanged bundle is trusted until it expires.
            let mut recorded: FxHashMap<PathBuf, Measured> =
                serde_json::from_str(&std::fs::read_to_string(&cache).expect("cache"))
                    .expect("json");
            for measured in recorded.values_mut() {
                measured.size = 1;
            }
            std::fs::write(&cache, serde_json::to_string(&recorded).expect("json")).expect("write");
            assert_eq!(total_size(&apps, Some(&cache)), 1);

            // Touching Info.plist, as an update does, measures the bundle again.
            let later = SystemTime::now() + Duration::from_secs(5);
            std::fs::File::options()
                .write(true)
                .open(apps.join("Kept.app/Contents/Info.plist"))
                .expect("open")
                .set_modified(later)
                .expect("touch");
            assert_eq!(total_size(&apps, Some(&cache)), first);
        }
    }
}
