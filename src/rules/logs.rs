use std::time::{Duration, SystemTime};

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::rules::contents::Contents;
use crate::scanner::entry::{Category, ScannedEntry};

/// A log written to within the last hour is held open by whatever writes it,
/// and deleting an open file returns no space until that process closes it.
fn quiet_for_an_hour() -> Option<SystemTime> {
    SystemTime::now().checked_sub(Duration::from_secs(3600))
}

fn log_contents(description: &str) -> Contents<'_> {
    Contents {
        category: Category::LogFile,
        safety: Category::LogFile.default_safety(),
        description,
        untouched_since: quiet_for_an_hour(),
    }
}

pub struct LogFilesRule;

impl CleanupRule for LogFilesRule {
    fn name(&self) -> &'static str {
        "Log Files"
    }

    fn category(&self) -> Category {
        Category::LogFile
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let mut entries = Vec::new();
        if cfg!(target_os = "macos") {
            let home = dirs::home_dir().unwrap_or_default();
            log_contents("User library logs").push(&mut entries, &home.join("Library/Logs"));
            for entry in &mut entries {
                if entry.path.ends_with("DiagnosticReports") {
                    "Diagnostic reports".clone_into(&mut entry.description);
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
        log_contents("System logs (/var/log)").push(&mut entries, std::path::Path::new("/var/log"));
        if cfg!(target_os = "macos") {
            log_contents("System application logs (/Library/Logs)")
                .push(&mut entries, std::path::Path::new("/Library/Logs"));
        }
        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(LogFilesRule), Box::new(SystemLogsRule)]
}
