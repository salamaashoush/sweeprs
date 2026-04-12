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

        // macOS: ~/Library/Logs (includes DiagnosticReports as subdirectory)
        if cfg!(target_os = "macos") {
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

            // Report DiagnosticReports separately for visibility, but size=0 (already counted)
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
        }

        entries
    }
}

/// Scan system-level logs in /var/log and /Library/Logs that accumulate over time.
pub struct SystemLogsRule;

impl CleanupRule for SystemLogsRule {
    fn name(&self) -> &'static str {
        "System logs"
    }

    fn category(&self) -> Category {
        Category::LogFile
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let mut entries = Vec::new();

        // /var/log - system logs (cross-platform, requires read permission)
        let var_log = std::path::PathBuf::from("/var/log");
        if var_log.exists() && std::fs::read_dir(&var_log).is_ok() {
            let (size, count) = walker::dir_size_and_count(&var_log);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: var_log,
                    size,
                    category: Category::LogFile,
                    safety: SafetyLevel::Caution,
                    description: "System logs (/var/log)".to_owned(),
                    item_count: Some(count),
                });
            }
        }

        // macOS: /Library/Logs - system-wide application logs
        if cfg!(target_os = "macos") {
            let lib_logs = std::path::PathBuf::from("/Library/Logs");
            if lib_logs.exists() && std::fs::read_dir(&lib_logs).is_ok() {
                let (size, count) = walker::dir_size_and_count(&lib_logs);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path: lib_logs,
                        size,
                        category: Category::LogFile,
                        safety: SafetyLevel::Caution,
                        description: "System application logs (/Library/Logs)".to_owned(),
                        item_count: Some(count),
                    });
                }
            }
        }

        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(LogFilesRule), Box::new(SystemLogsRule)]
}
