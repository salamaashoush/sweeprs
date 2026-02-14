use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index::PROJECT_INDEX;

pub struct DsStoreRule;

impl CleanupRule for DsStoreRule {
    fn name(&self) -> &'static str {
        "DS_Store files"
    }

    fn category(&self) -> Category {
        Category::SystemJunk
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        // Find all .DS_Store files in project directories via home + standard dirs
        let home = dirs::home_dir().unwrap_or_default();
        let mut total_size = 0u64;
        let mut count = 0usize;

        // Scan Desktop and Downloads for .DS_Store
        let dirs_to_scan = [
            home.join("Desktop"),
            home.join("Downloads"),
            home.join("Documents"),
        ];

        for dir in &dirs_to_scan {
            if !dir.exists() {
                continue;
            }
            scan_ds_store_recursive(dir, &mut total_size, &mut count, 0, 3);
        }

        // Also scan project roots from the index
        for root in PROJECT_INDEX.git_roots() {
            scan_ds_store_recursive(root, &mut total_size, &mut count, 0, 2);
        }

        if count == 0 {
            return Vec::new();
        }

        vec![ScannedEntry {
            path: home.join(".DS_Store_cleanup"),
            size: total_size,
            category: Category::SystemJunk,
            safety: SafetyLevel::Safe,
            description: format!("{count} .DS_Store files"),
            item_count: Some(count),
        }]
    }
}

fn scan_ds_store_recursive(
    dir: &std::path::Path,
    total: &mut u64,
    count: &mut usize,
    depth: usize,
    max_depth: usize,
) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(name) = path.file_name() {
                if name == ".DS_Store" {
                    if let Ok(meta) = path.metadata() {
                        *total += meta.len();
                        *count += 1;
                    }
                }
            }
        } else if path.is_dir() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            // Skip heavy dirs
            if !matches!(
                name_str.as_ref(),
                "node_modules" | "target" | ".git" | ".build" | "vendor" | ".venv"
            ) {
                scan_ds_store_recursive(&path, total, count, depth + 1, max_depth);
            }
        }
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(DsStoreRule)]
}
