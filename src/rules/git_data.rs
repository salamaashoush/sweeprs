use rayon::prelude::*;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index::PROJECT_INDEX;
use crate::scanner::walker;

const GIT_SIZE_THRESHOLD: u64 = 50 * 1024 * 1024;
const LFS_SIZE_THRESHOLD: u64 = 10 * 1024 * 1024;

pub struct GitRepoSizeRule;
pub struct GitLfsCacheRule;

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

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(GitRepoSizeRule), Box::new(GitLfsCacheRule)]
}
