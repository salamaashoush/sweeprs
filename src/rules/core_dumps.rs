use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::util::human_size;

pub struct CoreDumpsRule;

impl CleanupRule for CoreDumpsRule {
    fn name(&self) -> &'static str {
        "Core dumps"
    }

    fn category(&self) -> Category {
        Category::SystemJunk
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let cores_dir = std::path::PathBuf::from("/cores");

        if !cores_dir.exists() || !cores_dir.is_dir() {
            return Vec::new();
        }

        // Check if we can read the directory
        let Ok(read_dir) = std::fs::read_dir(&cores_dir) else {
            return Vec::new();
        };

        let mut entries = Vec::new();

        for entry in read_dir.flatten() {
            let path = entry.path();
            if !path.is_file() {
                continue;
            }

            let Ok(meta) = path.metadata() else {
                continue;
            };

            let size = meta.len();
            if size == 0 {
                continue;
            }

            let filename = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            entries.push(ScannedEntry {
                path,
                size,
                category: Category::SystemJunk,
                safety: SafetyLevel::Safe,
                description: format!("Core dump: {filename} ({})", human_size(size)),
                item_count: None,
            });
        }

        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(CoreDumpsRule)]
}
