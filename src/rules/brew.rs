use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::cli_cache;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};

/// Detects reclaimable space from `brew cleanup -n` (outdated downloads,
/// old portable-ruby versions, stale cache files).
pub struct BrewCleanupRule;

/// Detects unneeded dependency formulae from `brew autoremove --dry-run`.
pub struct BrewAutoremoveRule;

impl CleanupRule for BrewCleanupRule {
    fn name(&self) -> &'static str {
        "Homebrew cleanup"
    }

    fn category(&self) -> Category {
        Category::PackageCache
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let Some(result) = cli_cache::get("brew_cleanup") else {
            return Vec::new();
        };

        // Last line of `brew cleanup -n` looks like:
        //   ==> This operation would free approximately 119.3MB of disk space.
        let size = parse_brew_free_size(&result.stdout);
        if size == 0 {
            return Vec::new();
        }

        let item_count = result
            .stdout
            .lines()
            .filter(|l| l.starts_with("Would remove"))
            .count();

        vec![ScannedEntry {
            path: std::path::PathBuf::from("brew:cleanup"),
            size,
            category: Category::PackageCache,
            safety: SafetyLevel::Safe,
            description: format!(
                "Homebrew outdated cache ({item_count} items, brew cleanup)"
            ),
            item_count: Some(item_count),
        }]
    }
}

impl CleanupRule for BrewAutoremoveRule {
    fn name(&self) -> &'static str {
        "Homebrew autoremove"
    }

    fn category(&self) -> Category {
        Category::PackageCache
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let Some(result) = cli_cache::get("brew_autoremove") else {
            return Vec::new();
        };

        // Output format:
        //   ==> Would autoremove 36 unneeded formulae:
        //   package1
        //   package2
        //   ...
        let formulae: Vec<&str> = result
            .stdout
            .lines()
            .filter(|l| !l.starts_with("==>") && !l.is_empty())
            .collect();

        if formulae.is_empty() {
            return Vec::new();
        }

        // Sum up sizes of Cellar directories for each formula.
        let cellar = std::path::Path::new("/opt/homebrew/Cellar");
        let mut total_size = 0u64;
        for pkg in &formulae {
            let pkg_dir = cellar.join(pkg.trim());
            if pkg_dir.exists() {
                total_size += crate::scanner::walker::dir_size(&pkg_dir);
            }
        }

        if total_size == 0 {
            return Vec::new();
        }

        vec![ScannedEntry {
            path: std::path::PathBuf::from("brew:autoremove"),
            size: total_size,
            category: Category::PackageCache,
            safety: SafetyLevel::Safe,
            description: format!(
                "Homebrew unneeded deps ({} formulae, brew autoremove)",
                formulae.len()
            ),
            item_count: Some(formulae.len()),
        }]
    }
}

/// Parse the "would free approximately X" line from `brew cleanup -n`.
fn parse_brew_free_size(stdout: &str) -> u64 {
    // ==> This operation would free approximately 119.3MB of disk space.
    for line in stdout.lines().rev() {
        if let Some(rest) = line.strip_prefix("==> This operation would free approximately ") {
            let size_str = rest.trim_end_matches(" of disk space.");
            return parse_brew_size(size_str);
        }
    }
    0
}

fn parse_brew_size(s: &str) -> u64 {
    let s = s.trim();
    let (num_str, multiplier) = if let Some(n) = s.strip_suffix("GB") {
        (n, 1_000_000_000u64)
    } else if let Some(n) = s.strip_suffix("MB") {
        (n, 1_000_000u64)
    } else if let Some(n) = s.strip_suffix("KB") {
        (n, 1_000u64)
    } else if let Some(n) = s.strip_suffix('B') {
        (n, 1u64)
    } else {
        return 0;
    };

    num_str
        .trim()
        .parse::<f64>()
        .map(|n| (n * multiplier as f64) as u64)
        .unwrap_or(0)
}

/// Run the appropriate brew command for a synthetic `brew:` entry.
pub fn clean_brew_entry(entry_path: &str) -> Result<(), std::io::Error> {
    let type_key = entry_path
        .strip_prefix("brew:")
        .unwrap_or(entry_path);

    let args: &[&str] = match type_key {
        "cleanup" => &["cleanup"],
        "autoremove" => &["autoremove"],
        _ => {
            return Err(std::io::Error::other(
                format!("unknown brew type: {type_key}"),
            ));
        }
    };

    let status = std::process::Command::new("brew")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(
            format!("brew {} failed with exit code {status}", args.join(" ")),
        ))
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(BrewCleanupRule),
        Box::new(BrewAutoremoveRule),
    ]
}
