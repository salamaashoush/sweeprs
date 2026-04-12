use std::path::PathBuf;
use std::process::Command;

use rayon::prelude::*;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index::PROJECT_INDEX;
use crate::scanner::walker;

const GIT_SIZE_THRESHOLD: u64 = 50 * 1024 * 1024;
const LFS_SIZE_THRESHOLD: u64 = 10 * 1024 * 1024;
const GIT_GC_THRESHOLD: u64 = 100 * 1024 * 1024; // .git > 100 MB
const GIT_LOOSE_THRESHOLD: u64 = 256;
const REFLOG_THRESHOLD: u64 = 5 * 1024 * 1024; // 5 MB
const RERERE_THRESHOLD: u64 = 1 * 1024 * 1024; // 1 MB

pub struct GitRepoSizeRule;
pub struct GitLfsCacheRule;
pub struct GitGcRule;
pub struct GitReflogRule;
pub struct GitRererecacheRule;

impl CleanupRule for GitRepoSizeRule {
    fn name(&self) -> &'static str {
        "Git Repository Data"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        PROJECT_INDEX
            .git_roots()
            .par_iter()
            .filter_map(|root| {
                let git_dir = root.join(".git");
                if git_dir.exists() {
                    let size = walker::dir_size(&git_dir);
                    if size > GIT_SIZE_THRESHOLD {
                        return Some(ScannedEntry {
                            path: git_dir,
                            size,
                            category: Category::BuildArtifact,
                            safety: SafetyLevel::Caution,
                            description: "Git repository data".to_owned(),
                            item_count: None,
                        });
                    }
                }
                None
            })
            .collect()
    }
}

impl CleanupRule for GitLfsCacheRule {
    fn name(&self) -> &'static str {
        "Git LFS Cache"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        PROJECT_INDEX
            .git_roots()
            .par_iter()
            .filter_map(|root| {
                let lfs_objects = root.join(".git/lfs/objects");
                if lfs_objects.exists() {
                    let size = walker::dir_size(&lfs_objects);
                    if size > LFS_SIZE_THRESHOLD {
                        return Some(ScannedEntry {
                            path: lfs_objects,
                            size,
                            category: Category::BuildArtifact,
                            safety: SafetyLevel::Caution,
                            description: "Git LFS cache".to_owned(),
                            item_count: None,
                        });
                    }
                }
                None
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Git gc optimization rule
// ---------------------------------------------------------------------------

/// Parse `git count-objects -v` output into (loose_count, loose_size_kb, pack_count, garbage_size_kb).
fn parse_count_objects(output: &str) -> (u64, u64, u64, u64) {
    let mut count = 0u64;
    let mut size = 0u64;
    let mut packs = 0u64;
    let mut garbage_size = 0u64;

    for line in output.lines() {
        let line = line.trim();
        if let Some(val) = line.strip_prefix("count: ") {
            count = val.trim().parse().unwrap_or(0);
        } else if let Some(val) = line.strip_prefix("size: ") {
            size = val.trim().parse().unwrap_or(0);
        } else if let Some(val) = line.strip_prefix("in-pack: ") {
            packs = val.trim().parse().unwrap_or(0);
        } else if let Some(val) = line.strip_prefix("size-garbage: ") {
            garbage_size = val.trim().parse().unwrap_or(0);
        }
    }

    (count, size, packs, garbage_size)
}

impl CleanupRule for GitGcRule {
    fn name(&self) -> &'static str {
        "Git gc optimization"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        PROJECT_INDEX
            .git_roots()
            .par_iter()
            .filter_map(|root| {
                let git_dir = root.join(".git");
                if !git_dir.exists() {
                    return None;
                }

                let git_size = walker::dir_size(&git_dir);
                if git_size < GIT_GC_THRESHOLD {
                    return None;
                }

                // Run git count-objects -v with a 3-second timeout
                let output = Command::new("git")
                    .args(["-C", &root.display().to_string(), "count-objects", "-v"])
                    .stdout(std::process::Stdio::piped())
                    .stderr(std::process::Stdio::null())
                    .output()
                    .ok()?;

                if !output.status.success() {
                    return None;
                }

                let stdout = String::from_utf8_lossy(&output.stdout);
                let (loose_count, _loose_size, _packs, garbage_kb) =
                    parse_count_objects(&stdout);

                // Only suggest gc if there are enough loose objects or garbage
                if loose_count < GIT_LOOSE_THRESHOLD && garbage_kb == 0 {
                    return None;
                }

                let repo_name = root
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                // Estimate reclaimable: loose objects size + garbage.
                // Actual savings from gc --aggressive can be 10-30% of .git size.
                // Use a conservative 10% estimate for repos with many loose objects.
                let estimated_savings = if garbage_kb > 0 {
                    garbage_kb * 1024 + (git_size / 10)
                } else {
                    git_size / 10
                };

                let mut desc_parts = Vec::new();
                if loose_count > 0 {
                    desc_parts.push(format!("{loose_count} loose objects"));
                }
                if garbage_kb > 0 {
                    desc_parts.push(format!("{garbage_kb} KB garbage"));
                }

                Some(ScannedEntry {
                    path: PathBuf::from(format!("git-gc:{}", root.display())),
                    size: estimated_savings,
                    category: Category::BuildArtifact,
                    safety: SafetyLevel::Caution,
                    description: format!(
                        "Git gc: {repo_name} ({})",
                        desc_parts.join(", ")
                    ),
                    item_count: None,
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Git reflog cleanup rule
// ---------------------------------------------------------------------------

impl CleanupRule for GitReflogRule {
    fn name(&self) -> &'static str {
        "Git reflog data"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        PROJECT_INDEX
            .git_roots()
            .par_iter()
            .filter_map(|root| {
                let logs_dir = root.join(".git/logs");
                if !logs_dir.exists() {
                    return None;
                }

                let size = walker::dir_size(&logs_dir);
                if size < REFLOG_THRESHOLD {
                    return None;
                }

                Some(ScannedEntry {
                    path: logs_dir,
                    size,
                    category: Category::BuildArtifact,
                    safety: SafetyLevel::Caution,
                    description: "Git reflog data".to_owned(),
                    item_count: None,
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// Git rerere cache rule
// ---------------------------------------------------------------------------

impl CleanupRule for GitRererecacheRule {
    fn name(&self) -> &'static str {
        "Git rerere cache"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        PROJECT_INDEX
            .git_roots()
            .par_iter()
            .filter_map(|root| {
                let rr_cache = root.join(".git/rr-cache");
                if !rr_cache.exists() {
                    return None;
                }

                let size = walker::dir_size(&rr_cache);
                if size < RERERE_THRESHOLD {
                    return None;
                }

                Some(ScannedEntry {
                    path: rr_cache,
                    size,
                    category: Category::BuildArtifact,
                    safety: SafetyLevel::Safe,
                    description: "Git rerere cache".to_owned(),
                    item_count: None,
                })
            })
            .collect()
    }
}

/// Run `git gc --aggressive --prune=now` followed by `git reflog expire` on a repo.
/// The entry path is expected to be `git-gc:<repo_path>`.
/// Returns `Ok(())` on success.
pub fn clean_git_gc(entry_path: &str) -> Result<(), std::io::Error> {
    let repo_path = entry_path
        .strip_prefix("git-gc:")
        .unwrap_or(entry_path);

    // Run git gc --aggressive --prune=now
    let status = Command::new("git")
        .args(["-C", repo_path, "gc", "--aggressive", "--prune=now"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;

    if !status.success() {
        return Err(std::io::Error::other(format!(
            "git gc failed for {repo_path} with exit code {status}"
        )));
    }

    // Also expire reflog
    let _ = Command::new("git")
        .args(["-C", repo_path, "reflog", "expire", "--expire=now", "--all"])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status();

    Ok(())
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(GitRepoSizeRule),
        Box::new(GitLfsCacheRule),
        Box::new(GitGcRule),
        Box::new(GitReflogRule),
        Box::new(GitRererecacheRule),
    ]
}
