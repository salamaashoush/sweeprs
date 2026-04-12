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
    Toolchain,
    Docker,
    LogFile,
    Trash,
    OldDownload,
    LargeFile,
    Duplicate,
    MacosSpecific,
    LinuxSpecific,
    AppCache,
    SystemJunk,
    MobileBackup,
    LlmModels,
    StaleProject,
}

impl Category {
    #[allow(dead_code)]
    pub const ALL: &[Self] = &[
        Self::PackageCache,
        Self::BuildArtifact,
        Self::InstalledDeps,
        Self::BrowserCache,
        Self::IdeCache,
        Self::Toolchain,
        Self::Docker,
        Self::LogFile,
        Self::Trash,
        Self::OldDownload,
        Self::LargeFile,
        Self::Duplicate,
        Self::MacosSpecific,
        Self::LinuxSpecific,
        Self::AppCache,
        Self::SystemJunk,
        Self::MobileBackup,
        Self::LlmModels,
        Self::StaleProject,
    ];

    pub fn default_safety(self) -> SafetyLevel {
        match self {
            Self::PackageCache
            | Self::BuildArtifact
            | Self::InstalledDeps
            | Self::BrowserCache
            | Self::IdeCache
            | Self::AppCache => SafetyLevel::Safe,
            Self::Toolchain
            | Self::Docker
            | Self::LogFile
            | Self::OldDownload
            | Self::MacosSpecific
            | Self::LinuxSpecific
            | Self::SystemJunk
            | Self::MobileBackup
            | Self::LlmModels
            | Self::StaleProject => SafetyLevel::Caution,
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
            Self::Toolchain => write!(f, "Toolchains"),
            Self::Docker => write!(f, "Docker"),
            Self::LogFile => write!(f, "Log Files"),
            Self::Trash => write!(f, "Trash"),
            Self::OldDownload => write!(f, "Old Downloads"),
            Self::LargeFile => write!(f, "Large Files"),
            Self::Duplicate => write!(f, "Duplicates"),
            Self::MacosSpecific => write!(f, "macOS Specific"),
            Self::LinuxSpecific => write!(f, "Linux Specific"),
            Self::AppCache => write!(f, "App Caches"),
            Self::SystemJunk => write!(f, "System Junk"),
            Self::MobileBackup => write!(f, "Mobile Backups"),
            Self::LlmModels => write!(f, "LLM Models"),
            Self::StaleProject => write!(f, "Stale Projects"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SafetyLevel {
    Safe,
    Caution,
    Danger,
    /// Scan failed for this entry (tool not available, daemon not running, timeout, etc.)
    Error,
}

impl fmt::Display for SafetyLevel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Safe => write!(f, "Safe"),
            Self::Caution => write!(f, "Caution"),
            Self::Danger => write!(f, "Danger"),
            Self::Error => write!(f, "Error"),
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
pub struct VolumeInfo {
    pub name: String,
    pub mount_point: String,
    pub total_bytes: u64,
    pub available_bytes: u64,
    pub used_bytes: u64,
    pub usage_percent: f64,
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
    /// Size of locally cached iCloud Drive files that could be evicted.
    pub icloud_local_bytes: Option<u64>,
    /// Estimated size consumed by system and apps (not user data).
    pub system_app_bytes: Option<u64>,
    /// Other mounted APFS volumes (non-root).
    #[serde(default)]
    pub other_volumes: Vec<VolumeInfo>,
    /// Estimated bytes reclaimable by deleting old Time Machine snapshots.
    #[serde(default)]
    pub tm_reclaimable_bytes: Option<u64>,
}
