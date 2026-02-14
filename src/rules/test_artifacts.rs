use rayon::prelude::*;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index::PROJECT_INDEX;
use crate::scanner::walker;

/// Scan for test and coverage artifacts within discovered projects.
///
/// Uses the shared `PROJECT_INDEX` to avoid redundant directory walks.
/// Finds pytest_cache, mypy_cache, ruff_cache, htmlcov, nyc_output, and
/// coverage directories.
pub struct TestArtifactsRule;

impl CleanupRule for TestArtifactsRule {
    fn name(&self) -> &'static str {
        "test and coverage artifacts"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let patterns = vec![
            ".pytest_cache",
            ".mypy_cache",
            ".ruff_cache",
            "htmlcov",
            ".nyc_output",
        ];

        let mut all_dirs = Vec::new();

        // Collect all matching directories
        for pattern in patterns {
            all_dirs.extend(PROJECT_INDEX.find_dirs_by_name(pattern));
        }

        // Special handling for 'coverage' - only in JS projects
        for path in PROJECT_INDEX.find_dirs_by_name("coverage") {
            if let Some(parent) = path.parent() {
                if parent.join("package.json").exists() {
                    all_dirs.push(path);
                }
            }
        }

        if all_dirs.is_empty() {
            return Vec::new();
        }

        // Size them in parallel
        let entries: Vec<ScannedEntry> = all_dirs
            .par_iter()
            .filter_map(|path| {
                let size = walker::dir_size(path);
                if size > 0 {
                    let dir_name = path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();

                    Some(ScannedEntry {
                        path: path.clone(),
                        size,
                        category: Category::BuildArtifact,
                        safety: SafetyLevel::Safe,
                        description: format!("{dir_name}/"),
                        item_count: None,
                    })
                } else {
                    None
                }
            })
            .collect();

        if entries.is_empty() {
            return Vec::new();
        }

        // Enrich descriptions with parent directory context
        entries
            .into_iter()
            .map(|mut e| {
                if let Some(parent) = e.path.parent() {
                    let parent_name = parent
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    let dir_name = e
                        .path
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    e.description = format!("{dir_name}/ in {parent_name}");
                }
                e
            })
            .collect()
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(TestArtifactsRule)]
}
