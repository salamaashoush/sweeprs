pub mod app_cache;
pub mod brew;
pub mod browser;
pub mod build;
pub mod cache;
pub mod docker;
pub mod downloads;
pub mod duplicates;
pub mod gitignored;
pub mod ide;
pub mod large_files;
pub mod logs;
pub mod macos;
pub mod mobile;
pub mod system;
pub mod toolchain;
pub mod trash;

use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;

use crate::config::Config;
use crate::scanner::entry::{Category, DiskInfo, ScanResult, ScannedEntry};

pub enum ScanUpdate {
    Started {
        rules_total: usize,
    },
    RuleComplete {
        rule_name: &'static str,
        entries: Vec<ScannedEntry>,
    },
    Finished {
        duration_secs: f64,
        disk_info: Option<DiskInfo>,
    },
}

pub struct ScanProgress {
    pub rules_done: AtomicUsize,
    pub rules_total: AtomicUsize,
    pub bytes_found: AtomicU64,
    pub items_found: AtomicUsize,
    pub current_rule: std::sync::Mutex<String>,
}

impl ScanProgress {
    pub fn new() -> Self {
        Self {
            rules_done: AtomicUsize::new(0),
            rules_total: AtomicUsize::new(0),
            bytes_found: AtomicU64::new(0),
            items_found: AtomicUsize::new(0),
            current_rule: std::sync::Mutex::new(String::new()),
        }
    }
}

pub trait CleanupRule: Send + Sync {
    #[allow(dead_code)]
    fn name(&self) -> &'static str;
    fn category(&self) -> Category;
    fn scan(&self, config: &Config) -> Vec<ScannedEntry>;
}

static RULES: LazyLock<Vec<Box<dyn CleanupRule>>> = LazyLock::new(|| {
    let mut rules: Vec<Box<dyn CleanupRule>> = Vec::new();
    rules.extend(cache::rules());
    rules.extend(brew::rules());
    rules.extend(build::rules());
    rules.extend(gitignored::rules());
    rules.extend(browser::rules());
    rules.extend(ide::rules());
    rules.extend(toolchain::rules());
    rules.extend(docker::rules());
    rules.extend(logs::rules());
    rules.extend(trash::rules());
    rules.extend(downloads::rules());
    rules.extend(large_files::rules());
    rules.extend(duplicates::rules());
    rules.extend(macos::rules());
    rules.extend(app_cache::rules());
    rules.extend(system::rules());
    rules.extend(mobile::rules());
    rules
});

/// Eagerly initialize all `LazyLock` caches on dedicated OS threads
/// BEFORE dispatching rules to the rayon pool. This prevents rayon thread
/// starvation where all pool threads block on a `LazyLock` while the one thread
/// doing the initialization can't get pool workers for its own `par_iter()`.
fn warm_caches() {
    std::thread::scope(|s| {
        s.spawn(|| {
            let _ = &*crate::scanner::cli_cache::CLI_CACHE;
        });
        s.spawn(|| {
            let _ = &*crate::scanner::project_index::PROJECT_INDEX;
        });
    });
}

pub struct RuleEngine;

#[allow(clippy::unused_self)]
impl RuleEngine {
    pub fn new(_config: &Config) -> Self {
        Self
    }

    pub fn scan_all(&self, config: &Config, progress: Option<&ScanProgress>) -> ScanResult {
        use rayon::prelude::*;

        warm_caches();

        let filtered_rules: Vec<_> = RULES
            .iter()
            .filter(|rule| config.is_category_enabled(rule.category()))
            .collect();

        if let Some(p) = progress {
            p.rules_total.store(filtered_rules.len(), Ordering::Relaxed);
        }

        let entries: Vec<ScannedEntry> = filtered_rules
            .par_iter()
            .flat_map(|rule| {
                if let Some(p) = progress {
                    if let Ok(mut name) = p.current_rule.lock() {
                        *name = rule.name().to_string();
                    }
                }
                let found = rule.scan(config);
                if let Some(p) = progress {
                    let rule_bytes: u64 = found.iter().map(|e| e.size).sum();
                    p.bytes_found.fetch_add(rule_bytes, Ordering::Relaxed);
                    p.items_found.fetch_add(found.len(), Ordering::Relaxed);
                    p.rules_done.fetch_add(1, Ordering::Relaxed);
                }
                found
            })
            .collect();

        let total_size = entries.iter().map(|e| e.size).sum();
        ScanResult {
            entries,
            total_size,
            disk_info: None,
            scan_duration_secs: None,
        }
    }

    pub fn scan_all_streaming(&self, config: &Config, tx: &mpsc::Sender<ScanUpdate>) {
        use rayon::prelude::*;

        warm_caches();

        let filtered_rules: Vec<_> = RULES
            .iter()
            .filter(|rule| config.is_category_enabled(rule.category()))
            .collect();

        let _ = tx.send(ScanUpdate::Started {
            rules_total: filtered_rules.len(),
        });

        filtered_rules
            .par_iter()
            .for_each_with(tx.clone(), |tx, rule| {
                let entries = rule.scan(config);
                let _ = tx.send(ScanUpdate::RuleComplete {
                    rule_name: rule.name(),
                    entries,
                });
            });
    }

    pub fn scan_category(
        &self,
        category: Category,
        config: &Config,
        progress: Option<&ScanProgress>,
    ) -> ScanResult {
        use rayon::prelude::*;

        warm_caches();

        let filtered_rules: Vec<_> = RULES
            .iter()
            .filter(|rule| rule.category() == category)
            .collect();

        if let Some(p) = progress {
            p.rules_total.store(filtered_rules.len(), Ordering::Relaxed);
        }

        let entries: Vec<ScannedEntry> = filtered_rules
            .par_iter()
            .flat_map(|rule| {
                if let Some(p) = progress {
                    if let Ok(mut name) = p.current_rule.lock() {
                        *name = rule.name().to_string();
                    }
                }
                let found = rule.scan(config);
                if let Some(p) = progress {
                    let rule_bytes: u64 = found.iter().map(|e| e.size).sum();
                    p.bytes_found.fetch_add(rule_bytes, Ordering::Relaxed);
                    p.items_found.fetch_add(found.len(), Ordering::Relaxed);
                    p.rules_done.fetch_add(1, Ordering::Relaxed);
                }
                found
            })
            .collect();

        let total_size = entries.iter().map(|e| e.size).sum();
        ScanResult {
            entries,
            total_size,
            disk_info: None,
            scan_duration_secs: None,
        }
    }
}

macro_rules! cache_rule {
    ($name:ident, $display:expr, $category:expr, $safety:expr, $($path:expr),+ $(,)?) => {
        pub struct $name;

        impl $crate::rules::CleanupRule for $name {
            fn name(&self) -> &'static str {
                $display
            }

            fn category(&self) -> $crate::scanner::entry::Category {
                $category
            }

            fn scan(&self, _config: &$crate::config::Config) -> Vec<$crate::scanner::entry::ScannedEntry> {
                let home = dirs::home_dir().unwrap_or_default();
                let mut entries = Vec::new();

                $(
                    let path = home.join($path);
                    if path.exists() {
                        let size = $crate::scanner::walker::dir_size(&path);
                        if size > 0 {
                            entries.push($crate::scanner::entry::ScannedEntry {
                                path,
                                size,
                                category: $category,
                                safety: $safety,
                                description: format!("{}", $display),
                                item_count: None,
                            });
                        }
                    }
                )+

                entries
            }
        }
    };
}

pub(crate) use cache_rule;
