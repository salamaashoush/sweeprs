use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use anyhow::Result;

use crate::config::Config;
use crate::platform;
use crate::scanner;
use crate::util;

use super::notify;

pub fn pid_file_path() -> PathBuf {
    dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("sweeprs.pid")
}

pub fn run_loop(config: &Config) -> Result<()> {
    let poll_interval = Duration::from_secs(config.monitor.poll_interval_secs);
    let warning_threshold = config.monitor.warning_threshold_percent;
    let critical_threshold = config.monitor.critical_threshold_percent;

    loop {
        match platform::get_disk_info() {
            Ok(disk_info) => {
                let percent = disk_info.usage_percent;

                if percent >= f64::from(critical_threshold) {
                    let result = scanner::scan_all(config).ok();
                    let reclaimable = result
                        .as_ref()
                        .map_or_else(|| "unknown".to_owned(), |r| util::human_size(r.total_size));

                    let _ = notify::send_warning(
                        "sweeprs: Disk Critical!",
                        &format!(
                            "Disk usage at {:.1}%! {} free. {} reclaimable.",
                            percent,
                            util::human_size(disk_info.available_bytes),
                            reclaimable
                        ),
                    );
                } else if percent >= f64::from(warning_threshold) {
                    let _ = notify::send_warning(
                        "sweeprs: Disk Warning",
                        &format!(
                            "Disk usage at {:.1}%. {} free.",
                            percent,
                            util::human_size(disk_info.available_bytes)
                        ),
                    );
                }

                eprintln!(
                    "[sweeprs monitor] Disk: {:.1}% used, {} free",
                    percent,
                    util::human_size(disk_info.available_bytes)
                );
            }
            Err(e) => {
                eprintln!("[sweeprs monitor] Error checking disk: {e}");
            }
        }

        thread::sleep(poll_interval);
    }
}
