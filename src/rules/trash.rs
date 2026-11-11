use crate::config::Config;
use crate::rules::CleanupRule;
use crate::rules::contents::Contents;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};

pub struct TrashRule;

impl CleanupRule for TrashRule {
    fn name(&self) -> &'static str {
        "Trash"
    }

    fn category(&self) -> Category {
        Category::Trash
    }

    /// Emptying the Trash removes what is in it. The folder itself carries the
    /// permissions Finder and the desktop's trash implementation expect.
    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let trash_path = if cfg!(target_os = "macos") {
            home.join(".Trash")
        } else {
            home.join(".local/share/Trash")
        };
        let mut entries = Vec::new();
        Contents {
            category: Category::Trash,
            safety: SafetyLevel::Danger,
            description: "Trash",
            untouched_since: None,
        }
        .push(&mut entries, &trash_path);
        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(TrashRule)]
}
