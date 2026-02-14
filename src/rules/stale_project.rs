use std::path::Path;
use std::time::{Duration, SystemTime};

use rayon::prelude::*;

use crate::config::Config;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index::PROJECT_INDEX;
use crate::scanner::walker;

use super::CleanupRule;

/// Directories within a stale project that can be safely deleted and rebuilt.
const BUILD_DIRS: &[&str] = &["target", "build", "_build", ".build", "dist", "out"];
const DEPS_DIRS: &[&str] = &["node_modules", "vendor", ".venv", "venv", ".tox"];

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(StaleProjectRule)]
}

struct StaleProjectRule;

impl CleanupRule for StaleProjectRule {
    fn name(&self) -> &'static str {
        "Stale Projects"
    }

    fn category(&self) -> Category {
        Category::StaleProject
    }

    fn scan(&self, config: &Config) -> Vec<ScannedEntry> {
        let stale_days = config.categories.stale_project_days;
        let git_roots = PROJECT_INDEX.git_roots();

        git_roots
            .par_iter()
            .flat_map(|root| scan_stale_project(root, stale_days))
            .collect()
    }
}

fn scan_stale_project(root: &Path, stale_days: u64) -> Vec<ScannedEntry> {
    let days = match get_days_since_last_commit(root) {
        Some(d) if d >= stale_days => d,
        _ => return Vec::new(),
    };

    let project_name = root
        .file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| root.display().to_string());

    let mut entries = Vec::new();

    for &dir_name in BUILD_DIRS.iter().chain(DEPS_DIRS.iter()) {
        let dir = root.join(dir_name);
        if dir.exists() && dir.is_dir() {
            let size = walker::dir_size(&dir);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: dir,
                    size,
                    category: Category::StaleProject,
                    safety: SafetyLevel::Caution,
                    description: format!("Stale: {project_name} ({days} days) - {dir_name}"),
                    item_count: None,
                });
            }
        }
    }

    entries
}

fn get_days_since_last_commit(repo: &Path) -> Option<u64> {
    let output = std::process::Command::new("git")
        .args(["log", "-1", "--format=%ct"])
        .current_dir(repo)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let timestamp_str = String::from_utf8_lossy(&output.stdout);
    let timestamp: u64 = timestamp_str.trim().parse().ok()?;
    let commit_time = SystemTime::UNIX_EPOCH + Duration::from_secs(timestamp);

    let elapsed = SystemTime::now().duration_since(commit_time).ok()?;
    Some(elapsed.as_secs() / 86400)
}
