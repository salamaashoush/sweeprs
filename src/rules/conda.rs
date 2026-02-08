use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

/// Scan conda/mamba environments and package caches.
///
/// Covers:
/// - miniconda3, anaconda3, miniforge3, mambaforge environments
/// - Package caches (pkgs/) within each installation
pub struct CondaEnvsRule;

const CONDA_DIRS: &[&str] = &["miniconda3", "anaconda3", "miniforge3", "mambaforge"];

impl CleanupRule for CondaEnvsRule {
    fn name(&self) -> &'static str {
        "Conda environments"
    }

    fn category(&self) -> Category {
        Category::RustToolchain
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let mut entries = Vec::new();

        for conda_dir_name in CONDA_DIRS {
            let conda_root = home.join(conda_dir_name);
            if !conda_root.exists() {
                continue;
            }

            // Scan pkgs/ (package cache - always safe to clear)
            let pkgs_dir = conda_root.join("pkgs");
            if pkgs_dir.exists() {
                let size = walker::dir_size(&pkgs_dir);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path: pkgs_dir,
                        size,
                        category: Category::PackageCache,
                        safety: SafetyLevel::Safe,
                        description: format!("{conda_dir_name} package cache"),
                        item_count: None,
                    });
                }
            }

            // Scan individual environments
            let envs_dir = conda_root.join("envs");
            if !envs_dir.exists() {
                continue;
            }

            let Ok(read_dir) = std::fs::read_dir(&envs_dir) else {
                continue;
            };

            for entry in read_dir.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                // Skip symlinks (aliases)
                if path.symlink_metadata().is_ok_and(|m| m.is_symlink()) {
                    continue;
                }

                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();
                let size = walker::dir_size(&path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::RustToolchain,
                        safety: SafetyLevel::Caution,
                        description: format!("Conda env: {name_str}"),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(CondaEnvsRule)]
}
