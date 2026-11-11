pub mod agent_leftovers;
pub mod agent_sessions;
pub mod ai_tools;
pub mod android;
pub mod app_cache;
pub mod brew;
pub mod browser;
pub mod build;
pub mod cache;
pub mod cloud_cache;
pub mod conda;
pub mod containers;
pub mod contents;
pub mod core_dumps;
pub mod dev_caches;
pub mod docker;
pub mod downloads;
#[cfg(target_os = "macos")]
pub mod ds_store;
pub mod duplicates;
pub mod electron_data;
pub mod empty_dirs;
pub mod generic_caches;
pub mod git_data;
pub mod gitignored;
pub mod held_space;
pub mod ide;
pub mod large_files;
#[cfg(target_os = "linux")]
pub mod linux;
pub mod llm;
pub mod logs;
#[cfg(target_os = "macos")]
pub mod macos;
#[cfg(target_os = "macos")]
pub mod macos_extra;
pub mod mobile;
pub mod orphan_detection;
pub mod project_caches;
pub mod pycache;
pub mod simulator;
pub mod stale_project;
pub mod system;
pub mod test_artifacts;
pub mod toolchain;
pub mod trash;
pub mod virtualization;

use std::path::PathBuf;
use std::sync::LazyLock;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;

use crate::config::Config;
use crate::scanner::entry::{Category, DiskInfo, SafetyLevel, ScanResult, ScannedEntry};

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
    /// 0 = warming caches, 1 = scanning rules, 2 = collecting disk info
    pub phase: AtomicUsize,
}

impl ScanProgress {
    pub fn new() -> Self {
        Self {
            rules_done: AtomicUsize::new(0),
            rules_total: AtomicUsize::new(0),
            bytes_found: AtomicU64::new(0),
            items_found: AtomicUsize::new(0),
            current_rule: std::sync::Mutex::new(String::new()),
            phase: AtomicUsize::new(0),
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
    rules.extend(git_data::rules());
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
    #[cfg(target_os = "macos")]
    rules.extend(macos::rules());
    #[cfg(target_os = "macos")]
    rules.extend(macos_extra::rules());
    #[cfg(target_os = "linux")]
    rules.extend(linux::rules());
    rules.extend(app_cache::rules());
    rules.extend(system::rules());
    rules.extend(mobile::rules());
    rules.extend(conda::rules());
    rules.extend(android::rules());
    rules.extend(pycache::rules());
    rules.extend(containers::rules());
    rules.extend(cloud_cache::rules());
    rules.extend(llm::rules());
    rules.extend(dev_caches::rules());
    rules.extend(test_artifacts::rules());
    rules.extend(project_caches::rules());
    rules.extend(core_dumps::rules());
    rules.extend(electron_data::rules());
    rules.extend(virtualization::rules());
    rules.extend(simulator::rules());
    rules.extend(ai_tools::rules());
    rules.extend(agent_sessions::rules());
    rules.push(Box::new(agent_leftovers::AgentLeftoversRule));
    rules.push(Box::new(held_space::HeldSpaceRule));
    rules.extend(generic_caches::rules());
    rules.extend(orphan_detection::rules());
    #[cfg(target_os = "macos")]
    rules.extend(ds_store::rules());
    rules.extend(empty_dirs::rules());
    rules.extend(stale_project::rules());
    rules
});

/// Categories whose rules use the `PROJECT_INDEX` cache.
const PROJECT_INDEX_CATEGORIES: &[Category] = &[
    Category::Toolchain,
    Category::BuildArtifact,
    Category::InstalledDeps,
    Category::StaleProject,
    Category::AgentSession,
];

/// Categories whose rules use the `CLI_CACHE`.
const CLI_CACHE_CATEGORIES: &[Category] = &[
    Category::Docker,
    Category::Toolchain,
    Category::PackageCache,
    Category::Simulator,
];

/// Run `scan` while the shared caches warm up beside it.
///
/// Rules that need neither cache start at once instead of waiting for the
/// slowest prefetched command; a rule that needs one blocks on its `LazyLock`
/// until it is ready. The project index is built on a pool of its own: its
/// parallel walk would otherwise queue behind global workers that are blocked
/// waiting for the index, which deadlocks once every worker is waiting.
fn with_warm_caches<T: Send>(
    need_cli: bool,
    need_project: bool,
    scan: impl FnOnce() -> T + Send,
) -> T {
    crate::scanner::walker::reset_size_cache();

    std::thread::scope(|s| {
        if need_cli {
            s.spawn(|| {
                timed_phase("CLI prefetch", crate::scanner::cli_cache::prefetch);
            });
        }
        if need_project {
            s.spawn(|| {
                timed_phase("project index", || {
                    let build = || {
                        let _ = &*crate::scanner::project_index::PROJECT_INDEX;
                    };
                    match rayon::ThreadPoolBuilder::new().build() {
                        Ok(pool) => pool.install(build),
                        Err(_) => build(),
                    }
                });
            });
        }
        let result = scan();
        crate::scanner::walker::persist_listings();
        result
    })
}

fn caches_needed_for(categories: &[Category]) -> (bool, bool) {
    (
        categories.iter().any(|c| CLI_CACHE_CATEGORIES.contains(c)),
        categories
            .iter()
            .any(|c| PROJECT_INDEX_CATEGORIES.contains(c)),
    )
}

/// Collapse entries that overlap on disk so the reported total is the space a
/// clean would actually return.
///
/// Rules are written independently and several of them legitimately look at
/// overlapping roots -- `~/.gradle/caches` is both a package cache and build
/// output, a gitignored `dist/` is also a stale-project build dir. Counting both
/// inflates the headline figure by whatever the overlap is worth.
///
/// An entry nested inside another is dropped, because cleaning the outer one
/// removes it too. The exception is an outer entry that is *less* safe than the
/// inner one: there, dropping the inner entry would push the user towards the
/// riskier action to reclaim the same bytes, so both are kept and the inner size
/// is deducted from the outer.
///
/// An `Error` entry on a real path is a rule refusing to let that path go (an
/// agent worktree holding unpushed work). It is never absorbed, and any other
/// entry at that path or above it is dropped, since cleaning it would delete
/// what the refusal protects.
pub fn deduplicate_entries(mut entries: Vec<ScannedEntry>) -> Vec<ScannedEntry> {
    let refused: Vec<PathBuf> = entries
        .iter()
        .filter(|e| e.safety == SafetyLevel::Error && !crate::virtual_entry::is_virtual(&e.path))
        .map(|e| e.path.clone())
        .collect();
    if !refused.is_empty() {
        entries.retain(|e| {
            e.safety == SafetyLevel::Error || !refused.iter().any(|kept| kept.starts_with(&e.path))
        });
    }

    entries.sort_by(|a, b| {
        a.path
            .cmp(&b.path)
            .then_with(|| a.safety.cmp(&b.safety))
            .then_with(|| b.size.cmp(&a.size))
    });
    // Equal paths are adjacent and sorted safest-first, so the survivor is the
    // one that lets the user reclaim those bytes at the lowest risk. It inherits
    // what the other rule had to say -- "Stale: signer (412 days)" is the reason
    // to act on a `target/` that would otherwise read as routine build output.
    entries.dedup_by(|later, earlier| {
        if later.path != earlier.path {
            return false;
        }
        if !earlier.description.contains(later.description.as_str()) {
            earlier.description = format!("{} | {}", earlier.description, later.description);
        }
        true
    });

    let mut kept: Vec<ScannedEntry> = Vec::with_capacity(entries.len());
    // Indices into `kept` forming the chain of ancestors of the current entry.
    let mut ancestors: Vec<usize> = Vec::new();

    for entry in entries {
        if entry.safety == SafetyLevel::Error || crate::virtual_entry::is_virtual(&entry.path) {
            kept.push(entry);
            continue;
        }

        while ancestors
            .last()
            .is_some_and(|&i| !entry.path.starts_with(&kept[i].path))
        {
            ancestors.pop();
        }

        if let Some(&parent) = ancestors.last() {
            if kept[parent].safety <= entry.safety {
                continue;
            }
            kept[parent].size = kept[parent].size.saturating_sub(entry.size);
        }

        ancestors.push(kept.len());
        kept.push(entry);
    }

    kept
}

/// Deduplicate a rule sweep and total up what is left.
fn finalize(entries: Vec<ScannedEntry>) -> ScanResult {
    let entries = deduplicate_entries(drop_protected(entries));
    let total_size = entries.iter().map(|e| e.size).sum();
    ScanResult {
        entries,
        total_size,
        disk_info: None,
        scan_duration_secs: None,
    }
}

/// Drop entries the cleaner would refuse, so a scan never offers what a clean
/// cannot do.
fn drop_protected(mut entries: Vec<ScannedEntry>) -> Vec<ScannedEntry> {
    entries.retain(|entry| {
        if crate::virtual_entry::is_virtual(&entry.path) {
            return true;
        }
        match crate::guard::check(&entry.path) {
            Ok(()) => true,
            Err(refusal) => {
                if profiling_enabled() {
                    eprintln!("dropped ({}): {refusal}", entry.description);
                }
                false
            }
        }
    });
    entries
}

/// Set `SWEEPRS_PROFILE=1` to print how long each rule took, slowest first.
///
/// Scan time is dominated by a handful of rules walking dense trees, and which
/// ones those are depends entirely on what the machine has on it.
fn profiling_enabled() -> bool {
    std::env::var_os("SWEEPRS_PROFILE").is_some_and(|v| v != "0")
}

/// Run one scan phase, reporting its time under `SWEEPRS_PROFILE`.
///
/// The cache warm-up before the rules and the disk summary after them are part
/// of what the user waits for, so a profile of the rules alone misplaces the time.
pub fn timed_phase<T>(label: &str, phase: impl FnOnce() -> T) -> T {
    if !profiling_enabled() {
        return phase();
    }
    let start = std::time::Instant::now();
    let value = phase();
    eprintln!("  {:>8.2}s  phase: {label}", start.elapsed().as_secs_f64());
    value
}

fn run_rules(
    rules: &[&(impl std::ops::Deref<Target = dyn CleanupRule> + Sync)],
    config: &Config,
    progress: Option<&ScanProgress>,
) -> Vec<ScannedEntry> {
    use rayon::prelude::*;

    let profile = profiling_enabled();
    let timings: std::sync::Mutex<Vec<(std::time::Duration, &'static str, usize)>> =
        std::sync::Mutex::new(Vec::new());

    let entries: Vec<ScannedEntry> = rules
        .par_iter()
        .flat_map(|rule| {
            if let Some(p) = progress
                && let Ok(mut name) = p.current_rule.lock()
            {
                *name = rule.name().to_string();
            }

            let started = std::time::Instant::now();
            let found = rule.scan(config);
            if profile && let Ok(mut timings) = timings.lock() {
                timings.push((started.elapsed(), rule.name(), found.len()));
            }

            if let Some(p) = progress {
                let rule_bytes: u64 = found.iter().map(|e| e.size).sum();
                p.bytes_found.fetch_add(rule_bytes, Ordering::Relaxed);
                p.items_found.fetch_add(found.len(), Ordering::Relaxed);
                p.rules_done.fetch_add(1, Ordering::Relaxed);
            }
            found
        })
        .collect();

    if profile && let Ok(mut timings) = timings.lock() {
        timings.sort_by_key(|(elapsed, _, _)| std::cmp::Reverse(*elapsed));
        eprintln!("rule timings (slowest first):");
        for (elapsed, name, count) in timings.iter() {
            eprintln!("  {:>8.2}s  {name} ({count} items)", elapsed.as_secs_f64());
        }
    }

    entries
}

pub struct RuleEngine;

#[allow(clippy::unused_self)]
impl RuleEngine {
    pub fn new(_config: &Config) -> Self {
        Self
    }

    pub fn scan_all(&self, config: &Config, progress: Option<&ScanProgress>) -> ScanResult {
        let filtered_rules: Vec<_> = RULES
            .iter()
            .filter(|rule| config.is_category_enabled(rule.category()))
            .collect();
        Self::scan_rules(&filtered_rules, (true, true), config, progress)
    }

    fn scan_rules(
        rules: &[&(impl std::ops::Deref<Target = dyn CleanupRule> + Sync)],
        (need_cli, need_project): (bool, bool),
        config: &Config,
        progress: Option<&ScanProgress>,
    ) -> ScanResult {
        if let Some(p) = progress {
            p.phase.store(1, Ordering::Relaxed);
            p.rules_total.store(rules.len(), Ordering::Relaxed);
        }
        with_warm_caches(need_cli, need_project, || {
            finalize(run_rules(rules, config, progress))
        })
    }

    pub fn scan_all_streaming(&self, config: &Config, tx: &mpsc::Sender<ScanUpdate>) {
        use rayon::prelude::*;

        let filtered_rules: Vec<_> = RULES
            .iter()
            .filter(|rule| config.is_category_enabled(rule.category()))
            .collect();

        let _ = tx.send(ScanUpdate::Started {
            rules_total: filtered_rules.len(),
        });

        with_warm_caches(true, true, || {
            filtered_rules
                .par_iter()
                .for_each_with(tx.clone(), |tx, rule| {
                    let entries = drop_protected(rule.scan(config));
                    let _ = tx.send(ScanUpdate::RuleComplete {
                        rule_name: rule.name(),
                        entries,
                    });
                });
        });
    }

    pub fn scan_categories(
        &self,
        categories: &[Category],
        config: &Config,
        progress: Option<&ScanProgress>,
    ) -> ScanResult {
        let filtered_rules: Vec<_> = RULES
            .iter()
            .filter(|rule| categories.contains(&rule.category()))
            .collect();
        Self::scan_rules(
            &filtered_rules,
            caches_needed_for(categories),
            config,
            progress,
        )
    }

    pub fn scan_category(
        &self,
        category: Category,
        config: &Config,
        progress: Option<&ScanProgress>,
    ) -> ScanResult {
        let filtered_rules: Vec<_> = RULES
            .iter()
            .filter(|rule| rule.category() == category)
            .collect();
        Self::scan_rules(
            &filtered_rules,
            caches_needed_for(&[category]),
            config,
            progress,
        )
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

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(path: &str, size: u64, safety: SafetyLevel) -> ScannedEntry {
        ScannedEntry {
            path: PathBuf::from(path),
            size,
            category: Category::PackageCache,
            safety,
            description: path.to_owned(),
            item_count: None,
        }
    }

    fn paths(entries: &[ScannedEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|e| e.path.display().to_string())
            .collect()
    }

    #[test]
    fn a_nested_entry_is_not_counted_twice() {
        let kept = deduplicate_entries(vec![
            entry("/home/me/.gradle/caches", 900, SafetyLevel::Safe),
            entry("/home/me/.gradle/caches/modules-2", 400, SafetyLevel::Safe),
        ]);
        assert_eq!(paths(&kept), ["/home/me/.gradle/caches"]);
        assert_eq!(kept.iter().map(|e| e.size).sum::<u64>(), 900);
    }

    #[test]
    fn the_same_path_from_two_rules_survives_at_its_safest() {
        let kept = deduplicate_entries(vec![
            entry("/home/me/proj/dist", 100, SafetyLevel::Caution),
            entry("/home/me/proj/dist", 100, SafetyLevel::Safe),
        ]);
        assert_eq!(kept.len(), 1);
        assert_eq!(kept[0].safety, SafetyLevel::Safe);
    }

    #[test]
    fn a_safe_child_of_a_riskier_parent_stays_reachable() {
        // Deleting the whole toolchain dir is Caution; its download cache is not.
        // Collapsing them would make the user take the risky action for those bytes.
        let kept = deduplicate_entries(vec![
            entry("/home/me/.mise", 1000, SafetyLevel::Caution),
            entry("/home/me/.mise/downloads", 250, SafetyLevel::Safe),
        ]);
        assert_eq!(paths(&kept), ["/home/me/.mise", "/home/me/.mise/downloads"]);
        assert_eq!(kept.iter().map(|e| e.size).sum::<u64>(), 1000);
    }

    #[test]
    fn a_sibling_sharing_a_name_prefix_is_left_alone() {
        let kept = deduplicate_entries(vec![
            entry("/home/me/cache", 10, SafetyLevel::Safe),
            entry("/home/me/cache-old", 20, SafetyLevel::Safe),
        ]);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn deeply_nested_entries_collapse_into_the_outermost() {
        let kept = deduplicate_entries(vec![
            entry("/a", 100, SafetyLevel::Safe),
            entry("/a/b", 50, SafetyLevel::Safe),
            entry("/a/b/c", 25, SafetyLevel::Safe),
            entry("/z", 5, SafetyLevel::Safe),
        ]);
        assert_eq!(paths(&kept), ["/a", "/z"]);
    }

    #[test]
    fn virtual_entries_never_absorb_or_get_absorbed() {
        let kept = deduplicate_entries(vec![
            entry("git-gc:/home/me/proj", 100, SafetyLevel::Caution),
            entry("git-gc:/home/me/proj/nested", 50, SafetyLevel::Caution),
        ]);
        assert_eq!(kept.len(), 2);
    }

    #[test]
    fn a_refusal_is_never_absorbed_and_vetoes_what_would_delete_it() {
        let kept = deduplicate_entries(vec![
            entry("/home/me/.cursor/worktrees", 900, SafetyLevel::Safe),
            entry("/home/me/.cursor/worktrees/app/wip", 0, SafetyLevel::Error),
            entry(
                "/home/me/.cursor/worktrees/app/wip",
                500,
                SafetyLevel::Caution,
            ),
            entry("/home/me/.cursor/worktrees-old", 10, SafetyLevel::Safe),
        ]);
        assert_eq!(
            paths(&kept),
            [
                "/home/me/.cursor/worktrees/app/wip",
                "/home/me/.cursor/worktrees-old"
            ]
        );
        assert_eq!(kept[0].safety, SafetyLevel::Error);
    }
}
