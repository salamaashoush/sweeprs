use rayon::prelude::*;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index::PROJECT_INDEX;
use crate::scanner::walker;

/// Scan for __pycache__ directories and .pyc files within discovered projects.
///
/// Uses the shared `PROJECT_INDEX` to avoid redundant directory walks.
pub struct PycacheRule;

impl CleanupRule for PycacheRule {
    fn name(&self) -> &'static str {
        "__pycache__ directories"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        // Find all __pycache__ directories from the project index
        let pycache_dirs: Vec<_> = PROJECT_INDEX
            .find_dirs_by_name("__pycache__")
            .into_iter()
            .collect();

        if pycache_dirs.is_empty() {
            return Vec::new();
        }

        // Size them in parallel
        let entries: Vec<ScannedEntry> = pycache_dirs
            .par_iter()
            .filter_map(|path| {
                let size = walker::dir_size(path);
                if size > 0 {
                    Some(ScannedEntry {
                        path: path.clone(),
                        size,
                        category: Category::BuildArtifact,
                        safety: SafetyLevel::Safe,
                        description: "__pycache__/".to_owned(),
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

        // Return individual entries so users can selectively clean
        // Enrich descriptions with parent directory context
        entries
            .into_iter()
            .map(|mut e| {
                // Keep the path but enrich the description with parent context
                if let Some(parent) = e.path.parent() {
                    let parent_name = parent
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                    e.description = format!("__pycache__/ in {parent_name}");
                }
                e
            })
            .collect()
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(PycacheRule)]
}
