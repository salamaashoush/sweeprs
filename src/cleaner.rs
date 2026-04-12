use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use yansi::Paint;

use crate::rules::{brew, docker, git_data};
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::util;

/// Remove a directory tree, handling common edge cases:
/// - Read-only files/dirs (Go modules, `node_modules/.cache`): chmod before retry
/// - Dirs recreated by running apps (Chrome cache): retry once after short delay
fn remove_dir_robust(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            // Go modules and some caches have read-only dirs. Make writable and retry.
            fix_permissions(path);
            std::fs::remove_dir_all(path)
        }
        Err(e) if e.raw_os_error() == Some(if cfg!(target_os = "macos") { 66 } else { 39 }) /* ENOTEMPTY */ => {
            // Race: an app (e.g. Chrome) recreated files during deletion. Retry once.
            std::thread::sleep(std::time::Duration::from_millis(100));
            std::fs::remove_dir_all(path)
        }
        Err(e) => Err(e),
    }
}

/// Recursively make a directory tree writable so it can be deleted.
fn fix_permissions(path: &Path) {
    let walker = ignore::WalkBuilder::new(path)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .build();
    for entry in walker.flatten() {
        let p = entry.path();
        if let Ok(meta) = p.metadata() {
            let mut perms = meta.permissions();
            #[allow(clippy::permissions_set_readonly_false)]
            perms.set_readonly(false);
            let _ = std::fs::set_permissions(p, perms);
        }
    }
}

/// What to do with cleaned entries.
pub enum CleanAction {
    /// Delete entries permanently.
    Delete,
    /// Compress directories into .tar.zst (or .tar.gz) archives.
    /// If the path is Some, archives go there; otherwise next to the original.
    Archive(Option<PathBuf>),
}

pub struct CleanOptions {
    pub dry_run: bool,
    pub skip_confirm: bool,
    pub include_unsafe: bool,
    pub action: CleanAction,
}

pub fn clean(entries: &[ScannedEntry], options: &CleanOptions) -> Result<()> {
    let filtered: Vec<&ScannedEntry> = if options.include_unsafe {
        entries
            .iter()
            .filter(|e| e.safety != SafetyLevel::Error)
            .collect()
    } else {
        entries
            .iter()
            .filter(|e| e.safety == SafetyLevel::Safe)
            .collect()
    };

    if filtered.is_empty() {
        if !options.include_unsafe && !entries.is_empty() {
            let skipped: u64 = entries.iter().map(|e| e.size).sum();
            println!(
                "{}",
                format!(
                    "No safe items to clean. {} in Caution/Danger items skipped (use --all to include).",
                    util::human_size(skipped)
                )
                .dim()
            );
        } else {
            println!("{}", "Nothing to clean.".dim());
        }
        return Ok(());
    }

    let category_groups = group_by_category(&filtered);
    let total_size: u64 = filtered.iter().map(|e| e.size).sum();
    print_clean_summary(
        &category_groups,
        &filtered,
        entries,
        total_size,
        options.dry_run,
        options.include_unsafe,
    );

    if options.dry_run {
        println!(
            "\n{}",
            "Dry run - no files were deleted. Use --force to delete.".yellow()
        );
        return Ok(());
    }

    let (to_clean, clean_size) = if options.skip_confirm {
        (filtered, total_size)
    } else {
        let selected = interactive_confirm(&category_groups)?;
        if selected.is_empty() {
            println!("Cancelled.");
            return Ok(());
        }
        let to_clean: Vec<&ScannedEntry> = filtered
            .into_iter()
            .filter(|e| selected.contains(&e.category))
            .collect();
        let size = to_clean.iter().map(|e| e.size).sum();
        (to_clean, size)
    };

    if to_clean.is_empty() {
        println!("Nothing selected.");
        return Ok(());
    }

    let (archive, archive_dir) = match &options.action {
        CleanAction::Archive(dir) => (true, dir.as_deref()),
        CleanAction::Delete => (false, None),
    };
    delete_entries(&to_clean, clean_size, archive, archive_dir);
    Ok(())
}

fn print_safety_breakdown(
    filtered: &[&ScannedEntry],
    all_entries: &[ScannedEntry],
    include_unsafe: bool,
) {
    let safe_size: u64 = filtered
        .iter()
        .filter(|e| e.safety == SafetyLevel::Safe)
        .map(|e| e.size)
        .sum();
    let safe_count = filtered
        .iter()
        .filter(|e| e.safety == SafetyLevel::Safe)
        .count();
    let caution_size: u64 = filtered
        .iter()
        .filter(|e| e.safety == SafetyLevel::Caution)
        .map(|e| e.size)
        .sum();
    let caution_count = filtered
        .iter()
        .filter(|e| e.safety == SafetyLevel::Caution)
        .count();
    let danger_size: u64 = filtered
        .iter()
        .filter(|e| e.safety == SafetyLevel::Danger)
        .map(|e| e.size)
        .sum();
    let danger_count = filtered
        .iter()
        .filter(|e| e.safety == SafetyLevel::Danger)
        .count();

    if safe_count > 0 {
        println!(
            "  {} {} ({} items)",
            "[Safe]".green(),
            util::human_size(safe_size),
            safe_count
        );
    }
    if caution_count > 0 {
        println!(
            "  {} {} ({} items)",
            "[Caution]".yellow(),
            util::human_size(caution_size),
            caution_count
        );
    }
    if danger_count > 0 {
        println!(
            "  {} {} ({} items)",
            "[Danger]".red(),
            util::human_size(danger_size),
            danger_count
        );
    }

    if !include_unsafe {
        let unsafe_count = all_entries
            .iter()
            .filter(|e| e.safety != SafetyLevel::Safe && e.safety != SafetyLevel::Error)
            .count();
        if unsafe_count > 0 {
            let unsafe_size: u64 = all_entries
                .iter()
                .filter(|e| e.safety != SafetyLevel::Safe && e.safety != SafetyLevel::Error)
                .map(|e| e.size)
                .sum();
            println!(
                "  {} {unsafe_count} Caution/Danger items ({}) hidden. Use {} to include.",
                "Note:".dim(),
                util::human_size(unsafe_size),
                "--all".bold()
            );
        }
    }
}

fn print_clean_summary(
    category_groups: &[(Category, Vec<&ScannedEntry>)],
    filtered: &[&ScannedEntry],
    all_entries: &[ScannedEntry],
    total_size: u64,
    dry_run: bool,
    include_unsafe: bool,
) {
    println!(
        "\n{}",
        if dry_run {
            "Items to clean (dry run):".bold()
        } else {
            "Items to clean:".bold()
        }
    );

    // Detailed per-entry listing grouped by category
    for (i, (cat, cat_entries)) in category_groups.iter().enumerate() {
        let cat_size: u64 = cat_entries.iter().map(|e| e.size).sum();
        println!(
            "\n{} {} ({} items, {})",
            format!("[{:>2}]", i + 1).dim(),
            cat.to_string().bold(),
            cat_entries.len(),
            util::human_size(cat_size),
        );
        for entry in cat_entries {
            let safety_indicator = match entry.safety {
                SafetyLevel::Safe => "[Safe]".green(),
                SafetyLevel::Caution => "[Caution]".yellow(),
                SafetyLevel::Danger => "[Danger]".red(),
                SafetyLevel::Error => "[Error]".magenta(),
            };
            println!(
                "  {} {:>10}  {}",
                safety_indicator,
                util::human_size(entry.size),
                util::tilde_path(&entry.path)
            );
        }
    }

    println!(
        "\nTotal: {} across {} items in {} categories",
        util::human_size(total_size).bold(),
        filtered.len(),
        category_groups.len()
    );
    print_safety_breakdown(filtered, all_entries, include_unsafe);
}

/// Group entries by category, preserving order by total size descending.
fn group_by_category<'a>(entries: &[&'a ScannedEntry]) -> Vec<(Category, Vec<&'a ScannedEntry>)> {
    use indexmap::IndexMap;
    let mut groups: IndexMap<Category, Vec<&'a ScannedEntry>> = IndexMap::new();
    for entry in entries {
        groups.entry(entry.category).or_default().push(entry);
    }
    let mut result: Vec<_> = groups.into_iter().collect();
    result
        .sort_by_key(|(_, entries)| std::cmp::Reverse(entries.iter().map(|e| e.size).sum::<u64>()));
    result
}

/// Interactive confirmation that lets the user choose what to clean.
///
/// Returns the set of categories the user chose to clean, or empty if cancelled.
///
/// Accepts:
///   y / a / all  -- clean everything
///   n / q        -- cancel
///   s / safe     -- clean only Safe categories
///   c / caution  -- clean Safe + Caution categories
///   1,3,5        -- clean specific categories by number
///   1-4          -- clean a range of categories
fn interactive_confirm(
    category_groups: &[(Category, Vec<&ScannedEntry>)],
) -> Result<rustc_hash::FxHashSet<Category>> {
    let has_danger = category_groups
        .iter()
        .any(|(cat, _)| cat.default_safety() == SafetyLevel::Danger);
    if has_danger {
        println!(
            "\n{}",
            "WARNING: Some items are marked as Danger. Deletion may be irreversible."
                .red()
                .bold()
        );
    }

    println!(
        "\n{}",
        "Clean: [y]es all, [n]o cancel, [s]afe only, [c]aution+safe, or category numbers (1,3,5 or 1-4)"
            .dim()
    );
    print!("> ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let input = input.trim().to_lowercase();

    if input.is_empty() || input == "n" || input == "q" || input == "no" {
        return Ok(rustc_hash::FxHashSet::default());
    }

    // "y" / "a" / "all" / "yes" -- clean everything
    if input == "y" || input == "a" || input == "all" || input == "yes" {
        return Ok(category_groups.iter().map(|(cat, _)| *cat).collect());
    }

    // "s" / "safe" -- only Safe categories
    if input == "s" || input == "safe" {
        return Ok(category_groups
            .iter()
            .filter(|(cat, _)| cat.default_safety() == SafetyLevel::Safe)
            .map(|(cat, _)| *cat)
            .collect());
    }

    // "c" / "caution" -- Safe + Caution
    if input == "c" || input == "caution" {
        return Ok(category_groups
            .iter()
            .filter(|(cat, _)| {
                matches!(
                    cat.default_safety(),
                    SafetyLevel::Safe | SafetyLevel::Caution
                )
            })
            .map(|(cat, _)| *cat)
            .collect());
    }

    // Parse numbers: "1,3,5" or "1-4" or "1,3-5,7"
    let mut selected = rustc_hash::FxHashSet::default();
    for part in input.split(',') {
        let part = part.trim();
        if let Some((start, end)) = part.split_once('-') {
            let start: usize = start.trim().parse().unwrap_or(0);
            let end: usize = end.trim().parse().unwrap_or(0);
            if start >= 1 && end >= start {
                for i in start..=end {
                    if i <= category_groups.len() {
                        selected.insert(category_groups[i - 1].0);
                    }
                }
            }
        } else if let Ok(num) = part.parse::<usize>() {
            if num >= 1 && num <= category_groups.len() {
                selected.insert(category_groups[num - 1].0);
            }
        }
    }

    if selected.is_empty() {
        println!("No valid selection. Cancelled.");
    }

    Ok(selected)
}

/// Check if a filesystem path requires root privileges to modify.
fn needs_root(path: &Path) -> bool {
    let path_str = path.display().to_string();
    // System directories that require elevated permissions
    path_str.starts_with("/Library/")
        || path_str.starts_with("/System/")
        || path_str.starts_with("/private/var/")
        || path_str.starts_with("/var/cache/apt/")
        || path_str.starts_with("/var/cache/dnf/")
        || path_str.starts_with("/var/cache/pacman/")
        || path_str.starts_with("/var/cache/zypp/")
        || path_str.starts_with("/var/log/journal/")
        || path_str.starts_with("/var/lib/systemd/")
        || path_str.starts_with("/boot/")
        || path_str.starts_with("/usr/lib/modules/")
}

#[allow(clippy::too_many_lines)]
fn delete_entries(
    entries: &[&ScannedEntry],
    total_size: u64,
    archive: bool,
    archive_dir: Option<&Path>,
) {
    // Separate entries into fast (parallel filesystem ops) and slow (sequential git-gc, docker)
    let mut fast_entries: Vec<&ScannedEntry> = Vec::new();
    let mut slow_entries: Vec<&ScannedEntry> = Vec::new();
    let mut skipped_entries: Vec<(&ScannedEntry, &str)> = Vec::new();

    for entry in entries {
        let path_str = entry.path.display().to_string();
        if path_str.starts_with("git-gc:")
            || path_str.starts_with("docker:")
            || path_str.starts_with("brew:")
            || path_str.starts_with("journal:")
            || path_str.starts_with("pacman:")
            || path_str.starts_with("apt:")
            || path_str.starts_with("dnf:")
        {
            slow_entries.push(entry);
        } else if needs_root(&entry.path) {
            skipped_entries.push((entry, "requires sudo"));
        } else {
            fast_entries.push(entry);
        }
    }

    // Report skipped items upfront
    if !skipped_entries.is_empty() {
        println!();
        for (entry, reason) in &skipped_entries {
            println!(
                "  {} {} ({})",
                "Skipped".yellow(),
                util::tilde_path(&entry.path),
                reason
            );
        }
    }

    let actionable_count = fast_entries.len() + slow_entries.len();
    if actionable_count == 0 {
        println!("\nNothing to clean (all items require elevated permissions).");
        return;
    }

    let cleaned = AtomicU64::new(0);
    let errors: Mutex<Vec<(PathBuf, io::Error)>> = Mutex::new(Vec::new());

    let action = if archive { "Archiving" } else { "Cleaning" };

    run_slow_operations(&slow_entries, &cleaned, &errors);

    // Phase 2: fast parallel filesystem deletions with progress bar
    if !fast_entries.is_empty() {
        let bar = ProgressBar::new(fast_entries.len() as u64);
        bar.set_style(
            ProgressStyle::with_template(&format!(
                "{{spinner:.green}} {action} [{{bar:30.green/dim}}] {{pos}}/{{len}} items  {{msg}}"
            ))
            .expect("valid template")
            .progress_chars("=>-"),
        );

        fast_entries.par_iter().for_each(|entry| {
            let short_path = util::tilde_path(&entry.path);
            bar.set_message(short_path.clone());

            let result = if archive && entry.path.is_dir() {
                archive_directory(&entry.path, archive_dir)
            } else if entry.path.is_dir() {
                remove_dir_robust(&entry.path)
            } else if entry.path.is_file() {
                std::fs::remove_file(&entry.path)
            } else {
                bar.inc(1);
                return;
            };

            let freed = measure_freed(entry, &result);
            match result {
                Ok(()) => {
                    let total = cleaned.fetch_add(freed, Ordering::Relaxed) + freed;
                    bar.set_message(format!(
                        "{} / {}",
                        util::human_size(total),
                        util::human_size(total_size),
                    ));
                }
                Err(e) => {
                    if freed > 0 {
                        cleaned.fetch_add(freed, Ordering::Relaxed);
                    }
                    errors.lock().unwrap().push((entry.path.clone(), e));
                }
            }
            bar.inc(1);
        });

        bar.finish_and_clear();
    }

    let cleaned = cleaned.load(Ordering::Relaxed);
    let errors = errors.into_inner().unwrap();

    let verb = if archive { "Archived" } else { "Cleaned" };
    println!("\n{verb}: {}", util::human_size(cleaned).green().bold());

    if !skipped_entries.is_empty() {
        let skipped_size: u64 = skipped_entries.iter().map(|(e, _)| e.size).sum();
        println!(
            "Skipped: {} ({} items need sudo)",
            util::human_size(skipped_size).yellow(),
            skipped_entries.len()
        );
    }

    if !errors.is_empty() {
        println!("\nErrors:");
        for (path, err) in &errors {
            println!("  {} {}: {err}", "Failed".red(), util::tilde_path(path));
        }
    }
}

/// Run slow sequential operations (git gc, docker prune, brew cleanup) with per-item spinners.
fn run_slow_operations(
    entries: &[&ScannedEntry],
    cleaned: &AtomicU64,
    errors: &Mutex<Vec<(PathBuf, io::Error)>>,
) {
    if entries.is_empty() {
        return;
    }
    println!();
    for entry in entries {
        let path_str = entry.path.display().to_string();
        let display = if path_str.starts_with("git-gc:") {
            let repo = path_str.strip_prefix("git-gc:").unwrap_or(&path_str);
            format!(
                "git gc --aggressive {}",
                util::tilde_path(&PathBuf::from(repo))
            )
        } else if path_str.starts_with("docker:") {
            let kind = path_str.strip_prefix("docker:").unwrap_or(&path_str);
            format!("docker prune {kind}")
        } else if path_str.starts_with("journal:") {
            "journalctl --vacuum-size=100M".to_owned()
        } else if path_str.starts_with("pacman:") {
            "paccache -r -k 2".to_owned()
        } else if path_str.starts_with("apt:") {
            "apt clean".to_owned()
        } else if path_str.starts_with("dnf:") {
            "dnf clean all".to_owned()
        } else {
            format!("brew cleanup {path_str}")
        };

        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::with_template("{spinner:.cyan} {msg}")
                .expect("valid template")
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏"),
        );
        spinner.set_message(display.clone());
        spinner.enable_steady_tick(std::time::Duration::from_millis(80));

        let result = if path_str.starts_with("docker:") {
            docker::clean_docker_entry(&path_str)
        } else if path_str.starts_with("brew:") {
            brew::clean_brew_entry(&path_str)
        } else if path_str.starts_with("journal:") {
            #[cfg(target_os = "linux")]
            {
                crate::rules::linux::clean_journal()
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err(std::io::Error::other("not supported"))
            }
        } else if path_str.starts_with("pacman:") {
            #[cfg(target_os = "linux")]
            {
                crate::rules::linux::clean_pacman()
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err(std::io::Error::other("not supported"))
            }
        } else if path_str.starts_with("apt:") {
            #[cfg(target_os = "linux")]
            {
                crate::rules::linux::clean_apt()
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err(std::io::Error::other("not supported"))
            }
        } else if path_str.starts_with("dnf:") {
            #[cfg(target_os = "linux")]
            {
                crate::rules::linux::clean_dnf()
            }
            #[cfg(not(target_os = "linux"))]
            {
                Err(std::io::Error::other("not supported"))
            }
        } else {
            git_data::clean_git_gc(&path_str)
        };

        spinner.finish_and_clear();

        match result {
            Ok(()) => {
                cleaned.fetch_add(entry.size, Ordering::Relaxed);
                println!(
                    "  {} {} (freed ~{})",
                    "Done".green(),
                    display,
                    util::human_size(entry.size)
                );
            }
            Err(e) => {
                println!("  {} {display}: {e}", "Failed".red());
                errors.lock().unwrap().push((entry.path.clone(), e));
            }
        }
    }
}

/// Measure how many bytes were freed by a clean operation on an entry.
fn measure_freed(entry: &ScannedEntry, result: &Result<(), io::Error>) -> u64 {
    if result.is_ok() && !entry.path.exists() {
        // Fully removed
        return entry.size;
    }
    if entry.path.exists() {
        let remaining = if entry.path.is_dir() {
            crate::scanner::walker::dir_size(&entry.path)
        } else {
            entry.path.metadata().map_or(0, |m| m.len())
        };
        entry.size.saturating_sub(remaining)
    } else {
        entry.size
    }
}

/// Compress a directory into a .tar.zst (or .tar.gz fallback) archive, then remove the original.
/// If `archive_dir` is Some, the archive is placed there; otherwise next to the original.
fn archive_directory(dir: &Path, archive_dir: Option<&Path>) -> io::Result<()> {
    let dir_name = dir
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no directory name"))?
        .to_string_lossy();

    let parent = dir
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "no parent directory"))?;

    let dest_dir = archive_dir.unwrap_or(parent);

    // Ensure destination directory exists
    if !dest_dir.exists() {
        std::fs::create_dir_all(dest_dir)?;
    }

    // Try zstd first (faster, better compression), fall back to gzip
    let has_zstd = std::process::Command::new("zstd")
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success());

    if has_zstd {
        let archive_path = dest_dir.join(format!("{dir_name}.tar.zst"));

        // tar -cf - -C <parent> <dirname> | zstd -T0 -3 -o <archive>
        let tar = std::process::Command::new("tar")
            .args(["-cf", "-", "-C", &parent.display().to_string(), &dir_name])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()?;

        let zstd_status = std::process::Command::new("zstd")
            .args([
                "-T0",
                "-3",
                "--rm",
                "-o",
                &archive_path.display().to_string(),
            ])
            .stdin(tar.stdout.unwrap())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;

        if !zstd_status.success() {
            // Clean up partial archive
            let _ = std::fs::remove_file(&archive_path);
            return Err(io::Error::other("zstd compression failed"));
        }
    } else {
        // Fallback: gzip
        let archive_path = dest_dir.join(format!("{dir_name}.tar.gz"));

        let status = std::process::Command::new("tar")
            .args([
                "-czf",
                &archive_path.display().to_string(),
                "-C",
                &parent.display().to_string(),
                &dir_name,
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()?;

        if !status.success() {
            let _ = std::fs::remove_file(&archive_path);
            return Err(io::Error::other("tar compression failed"));
        }
    }

    // Archive created successfully, remove the original directory
    remove_dir_robust(dir)
}
