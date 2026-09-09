use rayon::prelude::*;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index::PROJECT_INDEX;
use crate::scanner::walker;

/// `(dir_name, marker_file, contents_that_prove_it_is_build_output)`.
///
/// A missing marker file is not enough on its own: `target` and `.build` are
/// ordinary English words, and a directory of someone's data named `target/`
/// must never be offered as a safe delete. Each candidate has to also contain
/// something only the build tool writes.
const ORPHAN_MARKERS: &[(&str, &str, &[&str])] = &[
    (
        "target",
        "Cargo.toml",
        &["CACHEDIR.TAG", ".rustc_info.json", "debug", "release"],
    ),
    (
        ".build",
        "Package.swift",
        &["checkouts", "debug", "release", "workspace-state.json"],
    ),
];

/// Files and directories that only a package manager puts inside `node_modules`.
const NODE_MODULES_FINGERPRINTS: &[&str] = &[
    ".package-lock.json",
    ".yarn-state.yml",
    ".modules.yaml",
    ".pnpm",
    ".bin",
];

fn contains_any(dir: &std::path::Path, names: &[&str]) -> bool {
    names.iter().any(|name| dir.join(name).exists())
}

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

                if !contains_any(dir, NODE_MODULES_FINGERPRINTS) {
                    return None;
                }

                let size = walker::dir_size(dir);
                if size == 0 {
                    return None;
                }

                Some(ScannedEntry {
                    path: dir.clone(),
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
            .flat_map(|(dir_name, marker_file, fingerprints)| {
                let artifact_dirs = PROJECT_INDEX.find_dirs_by_name(dir_name);

                artifact_dirs
                    .par_iter()
                    .filter_map(|dir| {
                        let parent = dir.parent()?;
                        if parent.join(marker_file).exists() {
                            return None;
                        }

                        if !contains_any(dir, fingerprints) {
                            return None;
                        }

                        let size = walker::dir_size(dir);
                        if size == 0 {
                            return None;
                        }

                        Some(ScannedEntry {
                            path: dir.clone(),
                            size,
                            category: Category::BuildArtifact,
                            safety: SafetyLevel::Safe,
                            description: format!("Orphaned {dir_name}/ (no {marker_file})"),
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
