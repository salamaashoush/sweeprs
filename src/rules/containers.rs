use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

/// Scan for Podman, Lima, and Colima container runtime data.
pub struct PodmanRule;
pub struct LimaRule;
pub struct ColimaRule;

impl CleanupRule for PodmanRule {
    fn name(&self) -> &'static str {
        "Podman data"
    }

    fn category(&self) -> Category {
        Category::Docker
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let mut entries = Vec::new();

        // Podman stores container data in ~/.local/share/containers
        let containers_dir = home.join(".local/share/containers");
        if containers_dir.exists() {
            let size = walker::dir_size(&containers_dir);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: containers_dir,
                    size,
                    category: Category::Docker,
                    safety: SafetyLevel::Caution,
                    description: "Podman container data".to_owned(),
                    item_count: None,
                });
            }
        }

        // Podman cache
        let cache_dir = home.join(".cache/containers");
        if cache_dir.exists() {
            let size = walker::dir_size(&cache_dir);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: cache_dir,
                    size,
                    category: Category::Docker,
                    safety: SafetyLevel::Safe,
                    description: "Podman cache".to_owned(),
                    item_count: None,
                });
            }
        }

        entries
    }
}

impl CleanupRule for LimaRule {
    fn name(&self) -> &'static str {
        "Lima VMs"
    }

    fn category(&self) -> Category {
        Category::Docker
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let lima_dir = home.join(".lima");

        if !lima_dir.exists() {
            return Vec::new();
        }

        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&lima_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();

                // Skip _config and _cache dirs
                if name_str.starts_with('_') {
                    continue;
                }

                let size = walker::dir_size(&path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::Docker,
                        safety: SafetyLevel::Caution,
                        description: format!("Lima VM: {name_str}"),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

impl CleanupRule for ColimaRule {
    fn name(&self) -> &'static str {
        "Colima data"
    }

    fn category(&self) -> Category {
        Category::Docker
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let colima_dir = home.join(".colima");

        if !colima_dir.exists() {
            return Vec::new();
        }

        let size = walker::dir_size(&colima_dir);
        if size == 0 {
            return Vec::new();
        }

        vec![ScannedEntry {
            path: colima_dir,
            size,
            category: Category::Docker,
            safety: SafetyLevel::Caution,
            description: "Colima VM data".to_owned(),
            item_count: None,
        }]
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(PodmanRule),
        Box::new(LimaRule),
        Box::new(ColimaRule),
    ]
}
