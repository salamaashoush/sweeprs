use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::util;

pub struct LargeFilesRule;

impl CleanupRule for LargeFilesRule {
    fn name(&self) -> &'static str {
        "Large Files"
    }

    fn category(&self) -> Category {
        Category::LargeFile
    }

    fn scan(&self, config: &Config) -> Vec<ScannedEntry> {
        let threshold = config.categories.large_file_threshold;
        let mut entries = Vec::new();

        for dir_str in &config.categories.large_file_dirs {
            let dir = Config::expand_path(dir_str);
            if !dir.exists() {
                continue;
            }

            let walker = ignore::WalkBuilder::new(&dir)
                .hidden(false)
                .ignore(false)
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false)
                .follow_links(false)
                .max_depth(Some(3))
                .build();

            for entry in walker.flatten() {
                if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                    continue;
                }

                let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                if size >= threshold {
                    let name = entry
                        .path()
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    entries.push(ScannedEntry {
                        path: entry.path().to_path_buf(),
                        size,
                        category: Category::LargeFile,
                        safety: SafetyLevel::Danger,
                        description: format!("{name} ({})", util::human_size(size)),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(LargeFilesRule)]
}
