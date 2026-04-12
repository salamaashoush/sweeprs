use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index::PROJECT_INDEX;

pub struct EmptyDirsRule;

impl CleanupRule for EmptyDirsRule {
    fn name(&self) -> &'static str {
        "Empty directories"
    }

    fn category(&self) -> Category {
        Category::SystemJunk
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let mut empty_count = 0usize;

        for root in PROJECT_INDEX.git_roots() {
            count_empty_dirs(root, &mut empty_count, 0, 4);
        }

        if empty_count == 0 {
            return Vec::new();
        }

        let home = dirs::home_dir().unwrap_or_default();
        vec![ScannedEntry {
            path: home.join(".empty_dirs_cleanup"),
            size: 0,
            category: Category::SystemJunk,
            safety: SafetyLevel::Safe,
            description: format!("{empty_count} empty directories in projects"),
            item_count: Some(empty_count),
        }]
    }
}

fn count_empty_dirs(dir: &std::path::Path, count: &mut usize, depth: usize, max_depth: usize) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    let entries: Vec<_> = entries.flatten().collect();

    for entry in &entries {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if matches!(
            name_str.as_ref(),
            "node_modules" | "target" | ".git" | ".build" | "vendor" | ".venv"
        ) {
            continue;
        }

        // Check if directory is empty
        if let Ok(mut read) = std::fs::read_dir(&path) {
            if read.next().is_none() {
                *count += 1;
            } else {
                count_empty_dirs(&path, count, depth + 1, max_depth);
            }
        }
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(EmptyDirsRule)]
}
