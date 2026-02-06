use std::fmt;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Category {
    PackageCache,
    BuildArtifact,
    InstalledDeps,
    BrowserCache,
    IdeCache,
    RustToolchain,
    Docker,
    LogFile,
    Trash,
    OldDownload,
    LargeFile,
    Duplicate,
    MacosSpecific,
    AppCache,
    SystemJunk,
    MobileBackup,
}

impl Category {
    #[allow(dead_code)]
    pub const ALL: &[Self] = &[
        Self::PackageCache,
        Self::BuildArtifact,
        Self::InstalledDeps,
        Self::BrowserCache,
        Self::IdeCache,
        Self::RustToolchain,
        Self::Docker,
        Self::LogFile,
        Self::Trash,
        Self::OldDownload,
        Self::LargeFile,
        Self::Duplicate,
        Self::MacosSpecific,
        Self::AppCache,
        Self::SystemJunk,
        Self::MobileBackup,
    ];

    pub fn default_safety(self) -> SafetyLevel {
        match self {
            Self::PackageCache
            | Self::BuildArtifact
            | Self::InstalledDeps
            | Self::BrowserCache
            | Self::IdeCache
            | Self::AppCache => SafetyLevel::Safe,
            Self::RustToolchain
            | Self::Docker
            | Self::LogFile
            | Self::OldDownload
            | Self::MacosSpecific
            | Self::SystemJunk
            | Self::MobileBackup => SafetyLevel::Caution,
            Self::Trash | Self::LargeFile | Self::Duplicate => SafetyLevel::Danger,
        }
    }
}

impl fmt::Display for Category {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PackageCache => write!(f, "Package Caches"),
            Self::BuildArtifact => write!(f, "Build Artifacts"),
            Self::InstalledDeps => write!(f, "Installed Dependencies"),
            Self::BrowserCache => write!(f, "Browser Caches"),
            Self::IdeCache => write!(f, "IDE Caches"),
            Self::RustToolchain => write!(f, "Rust Toolchains"),
            Self::Docker => write!(f, "Docker"),
            Self::LogFile => write!(f, "Log Files"),
            Self::Trash => write!(f, "Trash"),
            Self::OldDownload => write!(f, "Old Downloads"),
            Self::LargeFile => write!(f, "Large Files"),
            Self::Duplicate => write!(f, "Duplicates"),
            Self::MacosSpecific => write!(f, "macOS Specific"),
            Self::AppCache => write!(f, "App Caches"),
            Self::SystemJunk => write!(f, "System Junk"),
            Self::MobileBackup => write!(f, "Mobile Backups"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyLevel {
    Safe,
    Caution,
    Danger,
}

impl fmt::Display for SafetyLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Safe => write!(f, "Safe"),
            Self::Caution => write!(f, "Caution"),
            Self::Danger => write!(f, "Danger"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScannedEntry {
    pub path: PathBuf,
    pub size: u64,
    pub category: Category,
    pub safety: SafetyLevel,
    pub description: String,
    pub item_count: Option<usize>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanResult {
    pub entries: Vec<ScannedEntry>,
    pub total_size: u64,
    pub disk_info: Option<DiskInfo>,
    pub scan_duration_secs: Option<f64>,
}

impl ScanResult {
    #[allow(dead_code)]
    pub fn merge(&mut self, other: Self) {
        self.total_size += other.total_size;
        self.entries.extend(other.entries);
        if self.disk_info.is_none() {
            self.disk_info = other.disk_info;
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DiskInfo {
    pub name: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub usage_percent: f64,
    pub purgeable_bytes: Option<u64>,
    pub snapshot_bytes: u64,
}
