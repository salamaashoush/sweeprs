use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use crate::config::Config;
use crate::platform;
use crate::util;

use super::notify;

/// How long to suppress repeated notifications after one is sent.
const NOTIFICATION_COOLDOWN: Duration = Duration::from_secs(4 * 3600); // 4 hours

/// Sleep granularity -- wake up this often to check the shutdown flag.
const TICK: Duration = Duration::from_secs(5);

pub fn pid_file_path() -> PathBuf {
    dirs::runtime_dir()
        .or_else(dirs::cache_dir)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
        .join("sweeprs.pid")
}

pub fn run_loop(config: &Config, shutdown: &AtomicBool) {
    let poll_interval = Duration::from_secs(config.monitor.poll_interval_secs);
    let warning_threshold = f64::from(config.monitor.warning_threshold_percent);
    let critical_threshold = f64::from(config.monitor.critical_threshold_percent);

    let mut last_warning_at: Option<Instant> = None;
    let mut last_critical_at: Option<Instant> = None;

    loop {
        if shutdown.load(Ordering::Relaxed) {
            eprintln!("[sweeprs monitor] Shutting down.");
            break;
        }

        match platform::get_disk_info() {
            Ok(info) => {
                let pct = info.usage_percent;
                let free = util::human_size(info.available_bytes);

                if pct >= critical_threshold && should_notify(last_critical_at.as_ref()) {
                    let _ = notify::send_warning(
                        "sweeprs: Disk Critical!",
                        &format!(
                            "Disk usage at {pct:.1}%! {free} free. Run `sweeprs scan` to find reclaimable space.",
                        ),
                    );
                    last_critical_at = Some(Instant::now());
                } else if pct >= warning_threshold && should_notify(last_warning_at.as_ref()) {
                    let _ = notify::send_warning(
                        "sweeprs: Disk Warning",
                        &format!("Disk usage at {pct:.1}%. {free} free."),
                    );
                    last_warning_at = Some(Instant::now());
                }

                eprintln!("[sweeprs monitor] Disk: {pct:.1}% used, {free} free");
            }
            Err(e) => {
                eprintln!("[sweeprs monitor] Error checking disk: {e}");
            }
        }

        // Sleep in small ticks so we can respond to shutdown quickly.
        let deadline = Instant::now() + poll_interval;
        while Instant::now() < deadline {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            std::thread::sleep(TICK.min(deadline - Instant::now()));
        }
    }
}

fn should_notify(last: Option<&Instant>) -> bool {
    last.is_none_or(|t| t.elapsed() >= NOTIFICATION_COOLDOWN)
}
