use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use sysinfo::Disks;

use crate::cleaner;
use crate::config::Config;
use crate::platform;
use crate::rules::RuleEngine;
use crate::scanner::entry::SafetyLevel;
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
    // Cap rayon to 2 threads -- a background daemon should not saturate all cores
    // if auto-clean triggers par_iter().
    let _ = rayon::ThreadPoolBuilder::new()
        .num_threads(2)
        .build_global();

    let poll_interval = Duration::from_secs(config.monitor.poll_interval_secs);
    let warning_threshold = f64::from(config.monitor.warning_threshold_percent);
    let critical_threshold = f64::from(config.monitor.critical_threshold_percent);

    let mut last_warning_at: Option<Instant> = None;
    let mut last_critical_at: Option<Instant> = None;
    let mut last_auto_clean_at: Option<Instant> = None;

    // Reuse a single Disks instance across poll cycles instead of reallocating each time.
    let mut disks = Disks::new_with_refreshed_list();

    loop {
        if shutdown.load(Ordering::Relaxed) {
            eprintln!("[sweeprs monitor] Shutting down.");
            break;
        }

        disks.refresh(true);

        match platform::get_disk_usage(&disks) {
            Some((pct, available)) => {
                let free = util::human_size(available);

                if pct >= critical_threshold && should_notify(last_critical_at.as_ref()) {
                    notify::send_notification(
                        "sweeprs: Disk Critical!",
                        &format!(
                            "Disk usage at {pct:.1}%! {free} free. Run `sweeprs clean` to reclaim space.",
                        ),
                    );
                    last_critical_at = Some(Instant::now());
                } else if pct >= warning_threshold && should_notify(last_warning_at.as_ref()) {
                    notify::send_notification(
                        "sweeprs: Disk Warning",
                        &format!(
                            "Disk usage at {pct:.1}%. {free} free. Run `sweeprs clean` to reclaim space.",
                        ),
                    );
                    last_warning_at = Some(Instant::now());
                }

                // Auto-clean: when enabled and disk exceeds warning threshold
                if config.monitor.auto_clean
                    && pct >= warning_threshold
                    && should_notify(last_auto_clean_at.as_ref())
                {
                    eprintln!("[sweeprs monitor] Auto-clean triggered at {pct:.1}% usage");
                    run_auto_clean(config);
                    last_auto_clean_at = Some(Instant::now());
                }

                eprintln!("[sweeprs monitor] Disk: {pct:.1}% used, {free} free");
            }
            None => {
                eprintln!("[sweeprs monitor] Error checking disk: root volume not found");
            }
        }

        // Sleep in small ticks so we can respond to shutdown quickly.
        let deadline = Instant::now() + poll_interval;
        while Instant::now() < deadline {
            if shutdown.load(Ordering::Relaxed) {
                break;
            }
            let Some(remaining) = deadline.checked_duration_since(Instant::now()) else {
                break;
            };
            std::thread::sleep(TICK.min(remaining));
        }
    }
}

/// Run auto-clean: scan configured categories and delete only Safe-level entries.
fn run_auto_clean(config: &Config) {
    let categories = config.auto_clean_categories();
    if categories.is_empty() {
        eprintln!("[sweeprs monitor] Auto-clean: no categories configured");
        return;
    }

    let engine = RuleEngine::new(config);

    // Scan configured auto-clean categories
    let mut all_entries = Vec::new();
    for cat in &categories {
        if config.is_category_enabled(*cat) {
            let result = engine.scan_category(*cat, config, None);
            all_entries.extend(result.entries);
        }
    }

    // Filter to only Safe items
    let safe_entries: Vec<_> = all_entries
        .into_iter()
        .filter(|e| e.safety == SafetyLevel::Safe)
        .collect();

    if safe_entries.is_empty() {
        eprintln!("[sweeprs monitor] Auto-clean: nothing safe to clean");
        return;
    }

    let total: u64 = safe_entries.iter().map(|e| e.size).sum();
    eprintln!(
        "[sweeprs monitor] Auto-clean: {} safe items, {} total",
        safe_entries.len(),
        util::human_size(total),
    );

    let options = cleaner::CleanOptions {
        dry_run: false,
        skip_confirm: true,
        include_unsafe: false,
        action: cleaner::CleanAction::Delete,
        config: config.clone(),
    };

    if let Err(e) = cleaner::clean(&safe_entries, &options) {
        eprintln!("[sweeprs monitor] Auto-clean error: {e}");
    } else {
        eprintln!("[sweeprs monitor] Auto-clean complete");
    }
}

fn should_notify(last: Option<&Instant>) -> bool {
    last.is_none_or(|t| t.elapsed() >= NOTIFICATION_COOLDOWN)
}
