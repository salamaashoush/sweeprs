use rayon::prelude::*;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index::PROJECT_INDEX;
use crate::scanner::walker;

const ORPHAN_MARKERS: &[(&str, &str)] = &[
    ("target", "Cargo.toml"),
    (".build", "Package.swift"),
];

pub struct OrphanedNodeModulesRule;

impl CleanupRule for OrphanedNodeModulesRule {
    fn name(&self) -> &'static str {
        "Orphaned node_modules"
    }

    fn category(&self) -> Category {
        Category::InstalledDeps
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let node_modules_dirs = PROJECT_INDEX.find_dirs_by_name("node_modules");

        node_modules_dirs
            .par_iter()
            .filter_map(|dir| {
                let parent = dir.parent()?;
                let package_json = parent.join("package.json");

                if package_json.exists() {
                    return None;
                }

                let size = walker::dir_size(dir);
                if size == 0 {
                    return None;
                }

                Some(ScannedEntry {
                    path: dir.to_path_buf(),
                    size,
                    category: Category::InstalledDeps,
                    safety: SafetyLevel::Safe,
                    description: "Orphaned node_modules (no package.json)".to_string(),
                    item_count: None,
                })
            })
            .collect()
    }
}

pub struct OrphanedBuildArtifactsRule;

impl CleanupRule for OrphanedBuildArtifactsRule {
    fn name(&self) -> &'static str {
        "Orphaned build artifacts"
    }

    fn category(&self) -> Category {
        Category::BuildArtifact
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        ORPHAN_MARKERS
            .par_iter()
            .flat_map(|(dir_name, marker_file)| {
                let artifact_dirs = PROJECT_INDEX.find_dirs_by_name(dir_name);

                artifact_dirs
                    .par_iter()
                    .filter_map(|dir| {
                        let parent = dir.parent()?;
                        if parent.join(marker_file).exists() {
                            return None;
                        }

                        let size = walker::dir_size(dir);
                        if size == 0 {
                            return None;
                        }

                        Some(ScannedEntry {
                            path: dir.to_path_buf(),
                            size,
                            category: Category::BuildArtifact,
                            safety: SafetyLevel::Safe,
                            description: format!(
                                "Orphaned {dir_name}/ (no {marker_file})"
                            ),
                            item_count: None,
                        })
                    })
                    .collect::<Vec<_>>()
            })
            .collect()
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(OrphanedNodeModulesRule),
        Box::new(OrphanedBuildArtifactsRule),
    ]
}
