use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::Result;
use indicatif::{ProgressBar, ProgressStyle};
use rayon::prelude::*;
use yansi::Paint;

use crate::rules::{brew, docker};
use crate::scanner::entry::{SafetyLevel, ScannedEntry};
use crate::util;

/// Remove a directory tree, handling common edge cases:
/// - Read-only files/dirs (Go modules, node_modules/.cache): chmod before retry
/// - Dirs recreated by running apps (Chrome cache): retry once after short delay
fn remove_dir_robust(path: &Path) -> io::Result<()> {
    match std::fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::PermissionDenied => {
            // Go modules and some caches have read-only dirs. Make writable and retry.
            fix_permissions(path);
            std::fs::remove_dir_all(path)
        }
        Err(e) if e.raw_os_error() == Some(66) /* ENOTEMPTY on macOS */ => {
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

pub struct CleanOptions {
    pub dry_run: bool,
    pub skip_confirm: bool,
    pub include_unsafe: bool,
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
                ).dim()
            );
        } else {
            println!("{}", "Nothing to clean.".dim());
        }
        return Ok(());
    }

    let total_size: u64 = filtered.iter().map(|e| e.size).sum();

    println!(
        "Items to {}:",
        if options.dry_run {
            "clean (dry run)"
        } else {
            "clean"
        }
    );

    for entry in &filtered {
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

    println!(
        "\nTotal: {} across {} items",
        util::human_size(total_size).bold(),
        filtered.len()
    );

    if !options.include_unsafe {
        let unsafe_count = entries
            .iter()
            .filter(|e| e.safety != SafetyLevel::Safe)
            .count();
        if unsafe_count > 0 {
            let unsafe_size: u64 = entries
                .iter()
                .filter(|e| e.safety != SafetyLevel::Safe)
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

    if options.dry_run {
        println!(
            "\n{}",
            "Dry run - no files were deleted. Use --force to delete.".yellow()
        );
        return Ok(());
    }

    if !options.skip_confirm && !confirm_deletion(&filtered)? {
        println!("Cancelled.");
        return Ok(());
    }

    delete_entries(&filtered, total_size);
    Ok(())
}

fn confirm_deletion(entries: &[&ScannedEntry]) -> Result<bool> {
    let has_danger = entries.iter().any(|e| e.safety == SafetyLevel::Danger);
    if has_danger {
        println!(
            "\n{}",
            "WARNING: Some items are marked as Danger. Deletion may be irreversible."
                .red()
                .bold()
        );
    }
    print!("\nProceed with deletion? [y/N] ");
    io::stdout().flush()?;

    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    Ok(input.trim().eq_ignore_ascii_case("y"))
}

fn delete_entries(entries: &[&ScannedEntry], total_size: u64) {
    let bar = ProgressBar::new(entries.len() as u64);
    bar.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} Deleting [{bar:30.green/dim}] {pos}/{len} items  {msg}",
        )
        .expect("valid template")
        .progress_chars("=>-"),
    );

    let cleaned = AtomicU64::new(0);
    let errors: Mutex<Vec<(PathBuf, io::Error)>> = Mutex::new(Vec::new());

    entries.par_iter().for_each(|entry| {
        let path_str = entry.path.display().to_string();

        let is_synthetic = path_str.starts_with("docker:") || path_str.starts_with("brew:");

        let result = if path_str.starts_with("docker:") {
            docker::clean_docker_entry(&path_str)
        } else if path_str.starts_with("brew:") {
            brew::clean_brew_entry(&path_str)
        } else if entry.path.is_dir() {
            remove_dir_robust(&entry.path)
        } else if entry.path.is_file() {
            std::fs::remove_file(&entry.path)
        } else {
            bar.inc(1);
            return;
        };

        match result {
            Ok(()) => {
                // For synthetic paths (docker:/brew:) or fully removed entries,
                // count the full size. For filesystem paths, verify removal.
                let freed = if is_synthetic || !entry.path.exists() {
                    entry.size
                } else {
                    // Partial deletion: measure what remains and subtract
                    let remaining = if entry.path.is_dir() {
                        crate::scanner::walker::dir_size(&entry.path)
                    } else if entry.path.is_file() {
                        entry.path.metadata().map(|m| m.len()).unwrap_or(0)
                    } else {
                        0
                    };
                    entry.size.saturating_sub(remaining)
                };
                let total = cleaned.fetch_add(freed, Ordering::Relaxed) + freed;
                bar.set_message(format!(
                    "{} / {}",
                    util::human_size(total),
                    util::human_size(total_size),
                ));
            }
            Err(e) => {
                // Even on error, some bytes may have been freed (partial deletion)
                if !is_synthetic && entry.path.exists() {
                    let remaining = if entry.path.is_dir() {
                        crate::scanner::walker::dir_size(&entry.path)
                    } else {
                        entry.path.metadata().map(|m| m.len()).unwrap_or(0)
                    };
                    let freed = entry.size.saturating_sub(remaining);
                    if freed > 0 {
                        cleaned.fetch_add(freed, Ordering::Relaxed);
                    }
                }
                errors.lock().unwrap().push((entry.path.clone(), e));
            }
        }
        bar.inc(1);
    });

    bar.finish_and_clear();

    let cleaned = cleaned.load(Ordering::Relaxed);
    let errors = errors.into_inner().unwrap();

    println!("\nCleaned: {}", util::human_size(cleaned).green().bold());

    if !errors.is_empty() {
        println!("\nErrors:");
        for (path, err) in &errors {
            println!("  {} {}: {err}", "Failed".red(), util::tilde_path(path));
        }
    }
}
