use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use yansi::Paint;

use crate::config::Config;
use crate::guard;
use crate::in_use;
use crate::journal;
use crate::output;
use crate::rules::simulator;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::util;
use crate::virtual_entry;

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

/// Give the owner write access to every directory in a tree so it can be deleted.
///
/// Unlinking needs write permission on the parent directory only, so files are
/// left alone. Only the owner bit is added: `set_readonly(false)` adds the group
/// and world write bits too on Unix, and a daemon that finds its socket
/// directory world-writable refuses to use it.
fn fix_permissions(path: &Path) {
    use std::os::unix::fs::PermissionsExt;

    let walker = ignore::WalkBuilder::new(path)
        .hidden(false)
        .ignore(false)
        .git_ignore(false)
        .build();
    for entry in walker.flatten() {
        if !entry.file_type().is_some_and(|t| t.is_dir()) {
            continue;
        }
        let p = entry.path();
        if let Ok(meta) = p.symlink_metadata() {
            let mode = meta.permissions().mode();
            if mode & 0o200 == 0 {
                let _ = std::fs::set_permissions(p, std::fs::Permissions::from_mode(mode | 0o200));
            }
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
    pub detail: output::Detail,
    pub skip_confirm: bool,
    pub include_unsafe: bool,
    pub action: CleanAction,
    /// Rules that stand for a command rather than a path read their thresholds
    /// back out of the config at clean time.
    pub config: Config,
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
                "{} {}",
                "No safe items to clean.".dim(),
                format!(
                    "{} in caution/danger items held back; add {} to include them.",
                    util::human_size(skipped),
                    "--all".bold()
                )
                .dim()
            );
        } else {
            println!("{}", "Nothing to clean.".dim());
        }
        return Ok(());
    }

    let category_groups = output::group_by_category(filtered.iter().copied());
    let total_size: u64 = filtered.iter().map(|e| e.size).sum();
    print_clean_summary(&category_groups, &filtered, entries, options);

    if options.dry_run {
        println!(
            "\n{} {}",
            "Dry run, nothing was deleted.".yellow(),
            format!("Add {} to delete.", "--force".bold()).dim()
        );
        return Ok(());
    }

    let (to_clean, clean_size) = if options.skip_confirm {
        (filtered, total_size)
    } else {
        let choice = interactive_confirm(&category_groups)?;
        if choice == Choice::Cancel {
            println!("{}", "Cancelled, nothing was deleted.".dim());
            return Ok(());
        }
        let to_clean: Vec<&ScannedEntry> = filtered
            .into_iter()
            .filter(|e| choice.includes(e))
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
    delete_entries(&to_clean, clean_size, archive, archive_dir, &options.config);
    Ok(())
}

fn print_clean_summary(
    category_groups: &[(Category, Vec<&ScannedEntry>)],
    filtered: &[&ScannedEntry],
    all_entries: &[ScannedEntry],
    options: &CleanOptions,
) {
    let heading = if options.dry_run {
        "Would clean"
    } else {
        "To clean"
    };
    println!("\n{}", heading.bold());
    output::print_listing(
        category_groups,
        output::Listing {
            numbered: true,
            detail: options.detail,
        },
    );

    println!();
    output::rule();
    output::print_totals(filtered, category_groups.len(), "");

    if !options.include_unsafe {
        let held_back: Vec<&ScannedEntry> = all_entries
            .iter()
            .filter(|e| e.safety != SafetyLevel::Safe && e.safety != SafetyLevel::Error)
            .collect();
        if !held_back.is_empty() {
            let size: u64 = held_back.iter().map(|e| e.size).sum();
            println!(
                "  {}",
                format!(
                    "Not included: {} caution/danger items, {}. Add {} to include them.",
                    held_back.len(),
                    util::human_size(size),
                    "--all".bold()
                )
                .dim()
            );
        }
    }
}

/// What the user chose at the confirmation prompt.
#[derive(Debug, PartialEq, Eq)]
enum Choice {
    Cancel,
    Everything,
    /// Every entry at this safety level or safer, in any category. A category
    /// is not uniformly one level (a Safe category can hold a Caution entry),
    /// so "safe only" has to be decided per entry.
    UpTo(SafetyLevel),
    Categories(rustc_hash::FxHashSet<Category>),
}

impl Choice {
    fn includes(&self, entry: &ScannedEntry) -> bool {
        match self {
            Self::Cancel => false,
            Self::Everything => true,
            Self::UpTo(level) => entry.safety <= *level,
            Self::Categories(categories) => categories.contains(&entry.category),
        }
    }
}

/// Parse a prompt answer against the numbered categories.
///
/// Accepts `y`/`a`/`all`, `n`/`q` (or nothing), `s`/`safe`, `c`/`caution`,
/// and category numbers as a list and ranges (`1,3-5,7`).
fn parse_choice(input: &str, category_groups: &[(Category, Vec<&ScannedEntry>)]) -> Choice {
    let input = input.trim().to_lowercase();
    match input.as_str() {
        "" | "n" | "no" | "q" => return Choice::Cancel,
        "y" | "yes" | "a" | "all" => return Choice::Everything,
        "s" | "safe" => return Choice::UpTo(SafetyLevel::Safe),
        "c" | "caution" => return Choice::UpTo(SafetyLevel::Caution),
        _ => {}
    }

    let count = category_groups.len();
    let mut selected = rustc_hash::FxHashSet::default();
    for part in input.split(',').map(str::trim) {
        let range: Option<(usize, usize)> = match part.split_once('-') {
            Some((start, end)) => start.trim().parse().ok().zip(end.trim().parse().ok()),
            None => part.parse().ok().map(|n| (n, n)),
        };
        let Some((start, end)) = range else { continue };
        for number in start.max(1)..=end.min(count) {
            selected.insert(category_groups[number - 1].0);
        }
    }

    if selected.is_empty() {
        Choice::Cancel
    } else {
        Choice::Categories(selected)
    }
}

/// Ask which of the listed entries to clean.
fn interactive_confirm(category_groups: &[(Category, Vec<&ScannedEntry>)]) -> Result<Choice> {
    let has_danger = category_groups
        .iter()
        .flat_map(|(_, entries)| entries)
        .any(|e| e.safety == SafetyLevel::Danger);
    if has_danger {
        println!(
            "\n{}",
            "Some items are Danger: what they hold has no other copy."
                .red()
                .bold()
        );
    }

    let choices = [
        ("y", "everything"),
        ("s", "safe only"),
        ("c", "safe + caution"),
        ("1,3 2-4", "by number"),
        ("n", "cancel"),
    ];
    println!(
        "\n{} {}",
        "Clean which?".bold(),
        choices
            .iter()
            .map(|(key, what)| format!("{} {}", key.bold(), what.dim()))
            .collect::<Vec<_>>()
            .join("   ")
    );
    print!("{} ", "›".cyan().bold());
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    let choice = parse_choice(&input, category_groups);
    if choice == Choice::Cancel
        && !input.trim().is_empty()
        && !matches!(input.trim(), "n" | "no" | "q")
    {
        println!("{}", "No valid selection.".yellow());
    }
    Ok(choice)
}

/// Local Time Machine snapshots of the root volume.
///
/// A snapshot keeps every block that existed when it was taken, so what a
/// clean deletes stays allocated until each snapshot that saw it expires.
fn local_snapshot_count() -> usize {
    if !cfg!(target_os = "macos") {
        return 0;
    }
    crate::scanner::cli_cache::get("tmutil_snapshots").map_or(0, |r| {
        r.stdout
            .lines()
            .filter(|l| l.contains("com.apple."))
            .count()
    })
}

/// Check if a filesystem path requires root privileges to modify.
///
/// For a path that exists the filesystem answers: removing it takes owning it
/// and being able to write to its parent. A name such as `/private/var/` says
/// nothing on its own, since every user's temp directory lives under it.
pub fn needs_root(path: &Path) -> bool {
    use std::os::unix::fs::MetadataExt;

    if let Ok(meta) = path.symlink_metadata() {
        let uid = nix::unistd::getuid().as_raw();
        if uid == 0 {
            return false;
        }
        let parent_writable = path.parent().is_some_and(|parent| {
            nix::unistd::access(parent, nix::unistd::AccessFlags::W_OK).is_ok()
        });
        return meta.uid() != uid || !parent_writable;
    }
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
    config: &Config,
) {
    // Separate entries into fast (parallel filesystem ops) and slow (sequential git-gc, docker)
    let mut fast_entries: Vec<&ScannedEntry> = Vec::new();
    let mut slow_entries: Vec<&ScannedEntry> = Vec::new();
    let mut skipped_entries: Vec<(&ScannedEntry, &str)> = Vec::new();

    let open_files = in_use::OpenFiles::snapshot();
    if open_files.is_none() {
        println!(
            "{}",
            "Could not list open files; items in use will not be skipped.".yellow()
        );
    }

    for entry in entries {
        if virtual_entry::is_virtual(&entry.path) {
            // These stand for a command, not a directory: there is nothing to
            // put in a tarball. Running them anyway would delete the very data
            // the user asked to keep a copy of.
            if archive {
                skipped_entries.push((entry, "cannot be archived"));
            } else {
                slow_entries.push(entry);
            }
        } else if needs_root(&entry.path) {
            skipped_entries.push((entry, "requires sudo"));
        } else if open_files
            .as_ref()
            .is_some_and(|open| open.held_under(&entry.path).is_some())
        {
            skipped_entries.push((entry, "in use by a running process"));
        } else {
            fast_entries.push(entry);
        }
    }

    // Report skipped items upfront
    if !skipped_entries.is_empty() {
        println!();
        for (entry, reason) in &skipped_entries {
            println!(
                "  {} {}  {}",
                "–".yellow(),
                virtual_entry::display(&entry.path),
                format!("skipped, {reason}").dim()
            );
        }
    }

    let actionable_count = fast_entries.len() + slow_entries.len();
    if actionable_count == 0 {
        println!(
            "\n{}",
            "Nothing to clean: every selected item was skipped.".dim()
        );
        return;
    }

    let started = Instant::now();
    let cleaned = AtomicU64::new(0);
    let errors: Mutex<Vec<(PathBuf, io::Error)>> = Mutex::new(Vec::new());

    let action = if archive { "Archiving" } else { "Cleaning" };

    let slow_failures = run_slow_operations(&slow_entries, &cleaned, config);

    // Phase 2: fast parallel filesystem deletions with progress bar
    if !fast_entries.is_empty() {
        let bar = ProgressBar::new(fast_entries.len() as u64);
        bar.set_style(
            ProgressStyle::with_template(&format!(
                "  {{spinner:.cyan}} {action} {{bar:30.cyan/dim}} {{pos}}/{{len}}  {{wide_msg:.dim}}"
            ))
            .expect("valid template")
            .progress_chars("━╸─"),
        );

        fast_entries.par_iter().for_each(|entry| {
            let short_path = util::tilde_path(&entry.path);
            bar.set_message(short_path.clone());

            let result = if archive && entry.path.is_dir() {
                archive_directory(&entry.path, archive_dir)
            } else if entry.path.is_dir() || entry.path.is_file() {
                delete_path(&entry.path)
            } else {
                bar.inc(1);
                return;
            };

            let freed = measure_freed(entry, &result);
            match result {
                Ok(()) => {
                    journal::record(entry, freed);
                    let total = cleaned.fetch_add(freed, Ordering::Relaxed) + freed;
                    bar.set_message(format!(
                        "{} / {}",
                        util::human_size(total),
                        util::human_size(total_size),
                    ));
                }
                Err(e) => {
                    if freed > 0 {
                        journal::record(entry, freed);
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
    let failed = errors.len() + slow_failures;
    let succeeded = actionable_count - failed;

    let verb = if archive { "Archived" } else { "Freed" };
    println!();
    output::rule();
    println!(
        "  {} {} from {} {}",
        verb.bold(),
        util::human_size(cleaned).green().bold(),
        output::plural(succeeded, "item", "items"),
        format!("· {:.1}s", started.elapsed().as_secs_f64()).dim(),
    );
    if !archive && cleaned > 0 {
        let snapshots = local_snapshot_count();
        if snapshots > 0 {
            println!(
                "  {}",
                format!(
                    "{} kept by {} local Time Machine snapshot{}: free space rises as {} expire{}.",
                    util::human_size(cleaned),
                    snapshots,
                    if snapshots == 1 { "" } else { "s" },
                    if snapshots == 1 { "it" } else { "they" },
                    if snapshots == 1 { "s" } else { "" },
                )
                .yellow()
            );
        }
    }
    if let Some(log) = journal::path() {
        println!(
            "  {}",
            format!("Recorded in {}", util::tilde_path(&log)).dim()
        );
    }

    if !skipped_entries.is_empty() {
        let skipped_size: u64 = skipped_entries.iter().map(|(e, _)| e.size).sum();
        println!(
            "  {} {}, {} {}",
            "Skipped".yellow(),
            output::plural(skipped_entries.len(), "item", "items"),
            util::human_size(skipped_size),
            "(listed above)".dim()
        );
    }

    if failed > 0 {
        // Slow operations report their failure as they run; only the
        // filesystem deletions, which ran behind a progress bar, are listed here.
        let where_listed = if errors.is_empty() {
            " (listed above)"
        } else {
            ":"
        };
        println!(
            "  {} {}{}",
            "Failed".red(),
            output::plural(failed, "item", "items"),
            where_listed.dim()
        );
        for (path, err) in &errors {
            println!(
                "    {} {}  {}",
                "✗".red(),
                virtual_entry::display(path),
                err.to_string().dim()
            );
        }
    }
}

/// Delete a real filesystem entry, together with whatever registers it.
///
/// An Android AVD is a `.avd` payload plus a sibling `.ini` the emulator
/// enumerates it from; removing only the payload leaves a device that lists but
/// cannot launch.
pub fn delete_path(path: &Path) -> io::Result<()> {
    guard::check(path)?;
    if path.is_dir() {
        remove_dir_robust(path)?;
        if simulator::is_avd_payload(path) {
            simulator::remove_avd_ini(path);
        }
        Ok(())
    } else {
        std::fs::remove_file(path)
    }
}

/// Run slow sequential operations (git gc, docker prune, brew cleanup) with per-item spinners.
///
/// These are external tools with no progress output of their own once their
/// stderr is piped, so the spinner carries the elapsed time: a repack that takes
/// four minutes has to look like work in progress, not like a hang.
fn run_slow_operations(entries: &[&ScannedEntry], cleaned: &AtomicU64, config: &Config) -> usize {
    if entries.is_empty() {
        return 0;
    }
    println!();
    let mut failures = 0;
    for entry in entries {
        let path_str = entry.path.display().to_string();
        let display = virtual_entry::label(&path_str);

        let spinner = ProgressBar::new_spinner();
        spinner.set_style(
            ProgressStyle::with_template("  {spinner:.cyan} {wide_msg} {elapsed:.dim}")
                .expect("valid template")
                .tick_chars("⠋⠙⠹⠸⠼⠴⠦⠧⠇⠏ "),
        );
        spinner.set_message(display.clone());
        spinner.enable_steady_tick(Duration::from_millis(80));

        let started = Instant::now();
        let result = virtual_entry::clean(&path_str, config, config.git_gc_timeout());
        let elapsed = started.elapsed();

        spinner.finish_and_clear();

        match result {
            Ok(freed) => {
                let freed = freed.unwrap_or(entry.size);
                journal::record(entry, freed);
                cleaned.fetch_add(freed, Ordering::Relaxed);
                println!(
                    "  {} {display}  {}",
                    "✓".green(),
                    format!(
                        "freed {} · {:.1}s",
                        util::human_size(freed),
                        elapsed.as_secs_f64()
                    )
                    .dim(),
                );
            }
            Err(e) => {
                failures += 1;
                println!("  {} {display}  {}", "✗".red(), e.to_string().dim());
            }
        }
    }
    failures
}

/// Measure how many bytes were freed by a clean operation on an entry.
pub fn measure_freed(entry: &ScannedEntry, result: &Result<(), io::Error>) -> u64 {
    if result.is_ok() && !entry.path.exists() {
        // Fully removed
        return entry.size;
    }
    if entry.path.exists() {
        let remaining = if entry.path.is_dir() {
            crate::scanner::walker::dir_size_uncached(&entry.path)
        } else {
            crate::scanner::walker::file_size(&entry.path)
        };
        entry.size.saturating_sub(remaining)
    } else {
        entry.size
    }
}

/// Compress a directory into a .tar.zst (or .tar.gz fallback) archive, then remove the original.
/// If `archive_dir` is Some, the archive is placed there; otherwise next to the original.
fn archive_directory(dir: &Path, archive_dir: Option<&Path>) -> io::Result<()> {
    guard::check(dir)?;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn listed(category: Category, safety: SafetyLevel) -> ScannedEntry {
        ScannedEntry {
            path: PathBuf::from("/home/me/x"),
            size: 1,
            category,
            safety,
            description: String::new(),
            item_count: None,
        }
    }

    #[test]
    fn a_clean_removes_what_it_can_and_survives_what_it_cannot() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let cache = tmp.path().join("cache");
        std::fs::create_dir(&cache).expect("cache");
        std::fs::write(cache.join("blob"), vec![0u8; 8192]).expect("blob");

        let mut removable = listed(Category::PackageCache, SafetyLevel::Safe);
        removable.path = cache.clone();
        removable.size = 8192;
        // Not a repository, so the gc fails and is reported, not fatal.
        let mut failing = listed(Category::BuildArtifact, SafetyLevel::Caution);
        failing.path = PathBuf::from(format!("git-gc:{}", tmp.path().join("nope").display()));
        let mut privileged = listed(Category::LogFile, SafetyLevel::Caution);
        privileged.path = PathBuf::from("/Library/Logs/acme");

        delete_entries(
            &[&removable, &failing, &privileged],
            8192,
            false,
            None,
            &Config::default(),
        );

        assert!(!cache.exists());
    }

    #[test]
    fn safe_only_is_decided_per_entry_not_per_category() {
        // A Safe category holding a Caution entry, and a Caution category
        // holding a Safe one.
        let build_caution = listed(Category::BuildArtifact, SafetyLevel::Caution);
        let build_safe = listed(Category::BuildArtifact, SafetyLevel::Safe);
        let sim_safe = listed(Category::Simulator, SafetyLevel::Safe);
        let sim_danger = listed(Category::Simulator, SafetyLevel::Danger);
        let groups = vec![
            (Category::BuildArtifact, vec![&build_safe, &build_caution]),
            (Category::Simulator, vec![&sim_safe, &sim_danger]),
        ];

        let safe = parse_choice("s", &groups);
        assert!(safe.includes(&build_safe) && safe.includes(&sim_safe));
        assert!(!safe.includes(&build_caution));

        let caution = parse_choice(" C ", &groups);
        assert!(caution.includes(&build_caution));
        assert!(!caution.includes(&sim_danger));
    }

    #[test]
    fn numbers_and_ranges_select_categories_and_ignore_what_is_out_of_range() {
        let a = listed(Category::BuildArtifact, SafetyLevel::Safe);
        let b = listed(Category::Simulator, SafetyLevel::Caution);
        let c = listed(Category::Docker, SafetyLevel::Caution);
        let groups = vec![
            (Category::BuildArtifact, vec![&a]),
            (Category::Simulator, vec![&b]),
            (Category::Docker, vec![&c]),
        ];

        let choice = parse_choice("1, 3-9", &groups);
        assert!(choice.includes(&a) && choice.includes(&c));
        assert!(!choice.includes(&b));
        assert_eq!(parse_choice("0,42,x", &groups), Choice::Cancel);
        assert_eq!(parse_choice("", &groups), Choice::Cancel);
        assert_eq!(parse_choice("all", &groups), Choice::Everything);
    }

    #[test]
    fn deleting_an_avd_payload_takes_its_registering_ini_too() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();

        std::fs::create_dir(dir.join("Pixel_7.avd")).expect("avd dir");
        std::fs::write(dir.join("Pixel_7.avd/userdata.img"), b"x").expect("payload");
        std::fs::write(dir.join("Pixel_7.ini"), "path=/somewhere\n").expect("ini");

        std::fs::create_dir(dir.join("Keep_Me.avd")).expect("other avd");
        std::fs::write(dir.join("Keep_Me.ini"), "path=/elsewhere\n").expect("other ini");

        delete_path(&dir.join("Pixel_7.avd")).expect("delete");

        assert!(!dir.join("Pixel_7.avd").exists());
        // An orphaned .ini makes the emulator list an AVD that cannot launch.
        assert!(!dir.join("Pixel_7.ini").exists());
        assert!(dir.join("Keep_Me.avd").exists());
        assert!(dir.join("Keep_Me.ini").exists());
    }

    #[test]
    fn your_own_temp_files_need_no_root_and_system_files_do() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let mine = tmp
            .path()
            .canonicalize()
            .expect("canonical")
            .join("leftover");
        std::fs::write(&mine, b"x").expect("file");
        assert!(mine.starts_with("/private/var") || !cfg!(target_os = "macos"));
        assert!(!needs_root(&mine));
        assert!(needs_root(Path::new("/usr/bin")));
        assert!(needs_root(Path::new("/Library/Logs/acme-not-there")));
    }

    #[test]
    fn a_read_only_tree_is_still_deleted() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().expect("tempdir");
        let target = tmp.path().join("gomod");
        std::fs::create_dir_all(target.join("pkg")).expect("tree");
        std::fs::write(target.join("pkg/mod.go"), b"x").expect("file");
        std::fs::set_permissions(target.join("pkg"), std::fs::Permissions::from_mode(0o555))
            .expect("chmod");
        std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o555)).expect("chmod");

        delete_path(&target).expect("delete");

        assert!(!target.exists());
    }

    #[test]
    fn fixing_permissions_adds_only_the_owner_write_bit_to_directories() {
        use std::os::unix::fs::PermissionsExt;

        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().join("root");
        std::fs::create_dir_all(root.join("ipc")).expect("tree");
        std::fs::write(root.join("ipc/lock"), b"1").expect("file");
        std::fs::set_permissions(
            root.join("ipc/lock"),
            std::fs::Permissions::from_mode(0o400),
        )
        .expect("chmod");
        std::fs::set_permissions(root.join("ipc"), std::fs::Permissions::from_mode(0o500))
            .expect("chmod");

        fix_permissions(&root);

        let mode = |p: &Path| std::fs::metadata(p).expect("stat").permissions().mode() & 0o777;
        assert_eq!(mode(&root.join("ipc")), 0o700);
        assert_eq!(mode(&root.join("ipc/lock")), 0o400);
    }

    #[test]
    fn deleting_a_plain_cache_dir_leaves_siblings_alone() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();

        std::fs::create_dir(dir.join("Cache")).expect("cache dir");
        std::fs::write(dir.join("Cache.ini"), "not an avd\n").expect("lookalike");

        delete_path(&dir.join("Cache")).expect("delete");

        assert!(!dir.join("Cache").exists());
        assert!(dir.join("Cache.ini").exists());
    }
}
