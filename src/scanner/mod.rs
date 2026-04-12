pub mod cli_cache;
pub mod entry;
pub mod project_index;
pub mod walker;

#[cfg(target_os = "macos")]
pub mod bulk_stat;

use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::sync::mpsc;
use std::thread;
use std::time::Instant;

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};

use crate::config::Config;
use crate::platform;
pub use crate::rules::ScanUpdate;
use crate::rules::{RuleEngine, ScanProgress};
use crate::util;

use entry::{Category, ScanResult};

/// Spawn a background thread that updates a spinner with scan progress.
/// Returns a `JoinHandle` that should be joined after scanning completes.
fn spawn_progress_ticker(
    progress: Arc<ScanProgress>,
    spinner: ProgressBar,
) -> thread::JoinHandle<()> {
    thread::spawn(move || {
        loop {
            let phase = progress.phase.load(Ordering::Relaxed);
            let done = progress.rules_done.load(Ordering::Relaxed);
            let total = progress.rules_total.load(Ordering::Relaxed);
            let bytes = progress.bytes_found.load(Ordering::Relaxed);
            let items = progress.items_found.load(Ordering::Relaxed);

            match phase {
                0 => {
                    spinner.set_message("Warming caches (docker, brew, rustup, project index)...");
                }
                _ => {
                    if total > 0 {
                        let current = progress
                            .current_rule
                            .lock()
                            .map(|n| n.clone())
                            .unwrap_or_default();
                        spinner.set_message(format!(
                            "Scanning {done}/{total} rules | {items} items | {} | {current}",
                            util::human_size(bytes),
                        ));
                    }
                }
            }

            if phase >= 1 && done > 0 && done >= total && total > 0 {
                break;
            }
            thread::sleep(std::time::Duration::from_millis(80));
        }
    })
}

fn new_scan_spinner() -> ProgressBar {
    let spinner = ProgressBar::new_spinner();
    spinner.set_style(
        ProgressStyle::with_template("{spinner:.cyan} {msg} [{elapsed_precise}]")
            .expect("valid template")
            .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
    );
    spinner.set_message("Starting scan...");
    spinner.enable_steady_tick(std::time::Duration::from_millis(80));
    spinner
}

/// Configure rayon's global thread pool from the user's `scan.threads` setting.
/// If non-zero, limits the thread pool to that many threads.
/// Must be called before any rayon parallel iteration.
fn configure_thread_pool(config: &Config) {
    let threads = config.scan.threads;
    if threads > 0 {
        let _ = rayon::ThreadPoolBuilder::new()
            .num_threads(threads)
            .build_global();
    }
}

pub fn scan_all(config: &Config) -> Result<ScanResult> {
    configure_thread_pool(config);
    let engine = RuleEngine::new(config);
    let start = Instant::now();
    let mut result = engine.scan_all(config, None);
    result.scan_duration_secs = Some(start.elapsed().as_secs_f64());
    result.disk_info = Some(platform::get_disk_info()?);
    Ok(result)
}

pub fn scan_category(config: &Config, category: Category) -> Result<ScanResult> {
    configure_thread_pool(config);
    let engine = RuleEngine::new(config);
    let start = Instant::now();
    let mut result = engine.scan_category(category, config, None);
    result.scan_duration_secs = Some(start.elapsed().as_secs_f64());
    result.disk_info = Some(platform::get_disk_info_fast()?);
    Ok(result)
}

pub fn scan_all_with_progress(config: &Config) -> Result<ScanResult> {
    configure_thread_pool(config);
    let progress = Arc::new(ScanProgress::new());
    let spinner = new_scan_spinner();
    let tick_handle = spawn_progress_ticker(Arc::clone(&progress), spinner.clone());

    let engine = RuleEngine::new(config);
    let start = Instant::now();
    let mut result = engine.scan_all(config, Some(&progress));
    result.scan_duration_secs = Some(start.elapsed().as_secs_f64());

    let _ = tick_handle.join();
    spinner.finish_and_clear();

    result.disk_info = Some(platform::get_disk_info()?);
    Ok(result)
}

pub fn scan_all_streaming(config: &Config, tx: &mpsc::Sender<ScanUpdate>) {
    configure_thread_pool(config);
    let engine = RuleEngine::new(config);
    let start = Instant::now();
    engine.scan_all_streaming(config, tx);
    let disk_info = platform::get_disk_info().ok();
    let _ = tx.send(ScanUpdate::Finished {
        duration_secs: start.elapsed().as_secs_f64(),
        disk_info,
    });
}

/// Scan multiple categories with a progress spinner.
pub fn scan_categories_with_progress(
    config: &Config,
    categories: &[Category],
) -> Result<ScanResult> {
    use rayon::prelude::*;
    configure_thread_pool(config);
    let progress = Arc::new(ScanProgress::new());
    let spinner = new_scan_spinner();
    let tick_handle = spawn_progress_ticker(Arc::clone(&progress), spinner.clone());

    let engine = RuleEngine::new(config);
    let start = Instant::now();

    let partials: Vec<entry::ScanResult> = categories
        .par_iter()
        .map(|cat| engine.scan_category(*cat, config, Some(&progress)))
        .collect();

    let mut combined = entry::ScanResult::default();
    for partial in partials {
        combined.entries.extend(partial.entries);
        combined.total_size += partial.total_size;
    }
    combined.scan_duration_secs = Some(start.elapsed().as_secs_f64());

    let _ = tick_handle.join();
    spinner.finish_and_clear();

    combined.disk_info = Some(platform::get_disk_info_fast()?);
    Ok(combined)
}

pub fn scan_category_with_progress(config: &Config, category: Category) -> Result<ScanResult> {
    configure_thread_pool(config);
    let progress = Arc::new(ScanProgress::new());
    let spinner = new_scan_spinner();
    let tick_handle = spawn_progress_ticker(Arc::clone(&progress), spinner.clone());

    let engine = RuleEngine::new(config);
    let start = Instant::now();
    let mut result = engine.scan_category(category, config, Some(&progress));
    result.scan_duration_secs = Some(start.elapsed().as_secs_f64());

    let _ = tick_handle.join();
    spinner.finish_and_clear();

    result.disk_info = Some(platform::get_disk_info_fast()?);
    Ok(result)
}
