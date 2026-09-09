use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;
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
                .max_depth(Some(5))
                .build();

            for entry in walker.flatten() {
                if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                    continue;
                }

                let size = walker::file_size(entry.path());
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

        // Shallow scan of home directory (depth 1) for large files sitting directly in ~/
        if let Some(home) = dirs::home_dir() {
            let walker = ignore::WalkBuilder::new(&home)
                .hidden(false)
                .ignore(false)
                .git_ignore(false)
                .git_global(false)
                .git_exclude(false)
                .follow_links(false)
                .max_depth(Some(1))
                .build();

            for entry in walker.flatten() {
                if !entry.file_type().is_some_and(|ft| ft.is_file()) {
                    continue;
                }

                // Skip files already found in large_file_dirs
                let path = entry.path();
                if config
                    .categories
                    .large_file_dirs
                    .iter()
                    .any(|d| path.starts_with(Config::expand_path(d)))
                {
                    continue;
                }

                let size = walker::file_size(entry.path());
                if size >= threshold {
                    let name = path
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default();
                    entries.push(ScannedEntry {
                        path: path.to_path_buf(),
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
