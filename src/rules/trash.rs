use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

pub struct TrashRule;

impl CleanupRule for TrashRule {
    fn name(&self) -> &'static str {
        "Trash"
    }

    fn category(&self) -> Category {
        Category::Trash
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let trash_path = home.join(".Trash");

        if !trash_path.exists() {
            return Vec::new();
        }

        let (size, item_count) = walker::dir_size_and_count(&trash_path);
        if size == 0 {
            return Vec::new();
        }

        vec![ScannedEntry {
            path: trash_path,
            size,
            category: Category::Trash,
            safety: SafetyLevel::Danger,
            description: format!("Trash ({item_count} items)"),
            item_count: Some(item_count),
        }]
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(TrashRule)]
}
