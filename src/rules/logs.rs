use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

pub struct LogFilesRule;

impl CleanupRule for LogFilesRule {
    fn name(&self) -> &'static str {
        "Log Files"
    }

    fn category(&self) -> Category {
        Category::LogFile
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let mut entries = Vec::new();

        // Scan ~/Library/Logs once (includes DiagnosticReports as a subdirectory).
        let logs_dir = home.join("Library/Logs");
        if logs_dir.exists() {
            let (size, count) = walker::dir_size_and_count(&logs_dir);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: logs_dir.clone(),
                    size,
                    category: Category::LogFile,
                    safety: SafetyLevel::Caution,
                    description: "User library logs".to_owned(),
                    item_count: Some(count),
                });
            }
        }

        // Report DiagnosticReports separately for visibility, but do NOT add its
        // size to the total (it is already included in the parent).
        let diag_dir = home.join("Library/Logs/DiagnosticReports");
        if diag_dir.exists() {
            let (diag_size, diag_count) = walker::dir_size_and_count(&diag_dir);
            if diag_size > 0 {
                entries.push(ScannedEntry {
                    path: diag_dir,
                    size: 0,
                    category: Category::LogFile,
                    safety: SafetyLevel::Caution,
                    description: format!(
                        "Diagnostic reports ({diag_count} items, included in logs total)"
                    ),
                    item_count: Some(diag_count),
                });
            }
        }

        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(LogFilesRule)]
}
