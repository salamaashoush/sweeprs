use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index::PROJECT_INDEX;
use crate::scanner::walker;

/// Directories already covered by other rules -- skip these to avoid duplicates.
const KNOWN_ARTIFACT_DIRS: &[&str] = &[
    "node_modules",
    "target",
    ".build",
    ".venv",
    "venv",
    ".tox",
    "vendor",
    "build",
    "_build",
    ".dart_tool",
];

/// Minimum size to report (1 MiB). Tiny ignored dirs are not worth listing.
const MIN_SIZE: u64 = 1_048_576;

pub struct GitignoredRule;

impl GitignoredRule {
    fn find_gitignored_dirs() -> Vec<(PathBuf, String)> {
        let git_roots = PROJECT_INDEX.git_roots();

        // Filter out git roots that are themselves inside a gitignored path
        // of an ancestor git root.  e.g. if `myproject/.gitignore` contains
        // `.tmp`, then `myproject/.tmp/subdir/` (which has its own `.git`)
        // should not be scanned -- the entire `.tmp/` subtree is already
        // considered disposable by the parent project.
        let filtered_roots: Vec<&PathBuf> = git_roots
            .iter()
            .filter(|root| !is_gitignored_by_ancestor(root, git_roots))
            .collect();

        filtered_roots
            .par_iter()
            .flat_map(|root| find_ignored_in_project(root))
            .collect()
    }
}

/// Check whether `root` falls under a gitignored path of any ancestor git root.
fn is_gitignored_by_ancestor(root: &Path, all_roots: &[PathBuf]) -> bool {
    for ancestor in all_roots {
        if ancestor == root || !root.starts_with(ancestor) {
            continue;
        }

        let gitignore_path = ancestor.join(".gitignore");
        if !gitignore_path.exists() {
            continue;
        }

        let mut builder = ignore::gitignore::GitignoreBuilder::new(ancestor);
        if builder.add(&gitignore_path).is_some() {
            continue;
        }
        let Ok(gitignore) = builder.build() else {
            continue;
        };

        // `matched_path_or_any_parents` checks root AND every intermediate
        // directory between `ancestor` and `root`, so if `.tmp/` is
        // ignored it will match even when root is `.tmp/subdir/`.
        if gitignore
            .matched_path_or_any_parents(root, true)
            .is_ignore()
        {
            return true;
        }
    }
    false
}

/// For a given project root, parse its `.gitignore` and find large ignored directories.
fn find_ignored_in_project(root: &Path) -> Vec<(PathBuf, String)> {
    let gitignore_path = root.join(".gitignore");
    if !gitignore_path.exists() {
        return Vec::new();
    }

    let mut builder = ignore::gitignore::GitignoreBuilder::new(root);
    if builder.add(&gitignore_path).is_some() {
        return Vec::new();
    }
    let Ok(gitignore) = builder.build() else {
        return Vec::new();
    };

    let Ok(entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };

    let project_name = root.file_name().map_or_else(
        || root.display().to_string(),
        |n| n.to_string_lossy().to_string(),
    );

    let mut found = Vec::new();

    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // Skip dirs already handled by other rules
        if KNOWN_ARTIFACT_DIRS.contains(&name_str.as_ref()) {
            continue;
        }

        // Skip .git itself (essential, never gitignored).
        // Other dot-dirs may be gitignored and worth reporting.
        if name_str.as_ref() == ".git" {
            continue;
        }

        let path = entry.path();

        // Check if this directory is gitignored
        let matched = gitignore.matched_path_or_any_parents(&path, true);
        if matched.is_ignore() {
            let label = format!("{name_str}/ (gitignored in {project_name})");
            found.push((path, label));
        }
    }

    found
}

impl CleanupRule for GitignoredRule {
    fn name(&self) -> &'static str {
        "Gitignored Artifacts"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let dirs = Self::find_gitignored_dirs();

        dirs.par_iter()
            .filter_map(|(path, description)| {
                let (size, count) = walker::dir_size_and_count(path);
                if size >= MIN_SIZE {
                    Some(ScannedEntry {
                        path: path.clone(),
                        size,
                        category: Category::BuildArtifact,
                        safety: SafetyLevel::Safe,
                        description: description.clone(),
                        item_count: Some(count),
                    })
                } else {
                    None
                }
            })
            .collect()
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(GitignoredRule)]
}
