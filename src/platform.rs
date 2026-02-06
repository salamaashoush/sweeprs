use anyhow::{Result, anyhow};
use sysinfo::Disks;

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

    Ok(DiskInfo {
        name: root_disk.name().to_string_lossy().into_owned(),
        mount_point: "/".to_owned(),
        total_bytes: total,
        available_bytes: available,
        used_bytes: used,
        usage_percent,
    })
}
