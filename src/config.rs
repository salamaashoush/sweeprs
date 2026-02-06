use std::path::{Path, PathBuf};

use anyhow::Result;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: GeneralConfig,
    pub scan: ScanConfig,
    pub categories: CategoriesConfig,
    pub monitor: MonitorConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GeneralConfig {
    pub confirm_before_delete: bool,
    pub cli_dry_run_default: bool,
    pub output_format: String,
    pub global_excludes: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanConfig {
    pub max_depth: usize,
    pub threads: usize,
    pub follow_symlinks: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CategoriesConfig {
    pub download_age_days: u64,
    pub large_file_threshold: u64,
    pub large_file_dirs: Vec<String>,
    pub enable_duplicates: bool,
    pub duplicate_min_size: u64,
    pub duplicate_dirs: Vec<String>,
    pub enabled: EnabledCategories,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EnabledCategories {
    pub package_cache: bool,
    pub build_artifact: bool,
    pub installed_deps: bool,
    pub browser_cache: bool,
    pub ide_cache: bool,
    pub rust_toolchain: bool,
    pub docker: bool,
    pub log_file: bool,
    pub trash: bool,
    pub old_download: bool,
    pub large_file: bool,
    pub duplicate: bool,
    pub macos_specific: bool,
    pub app_cache: bool,
    pub system_junk: bool,
    pub mobile_backup: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MonitorConfig {
    pub poll_interval_secs: u64,
    pub warning_threshold_percent: u8,
    pub critical_threshold_percent: u8,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            confirm_before_delete: true,
            cli_dry_run_default: true,
            output_format: "table".to_owned(),
            global_excludes: Vec::new(),
        }
    }
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            max_depth: 10,
            threads: 0,
            follow_symlinks: false,
        }
    }
}

impl Default for CategoriesConfig {
    fn default() -> Self {
        Self {
            download_age_days: 90,
            large_file_threshold: 524_288_000,
            large_file_dirs: vec!["~/Downloads".to_owned(), "~/Desktop".to_owned()],
            enable_duplicates: false,
            duplicate_min_size: 1_048_576,
            duplicate_dirs: Vec::new(),
            enabled: EnabledCategories::default(),
        }
    }
}

impl Default for EnabledCategories {
    fn default() -> Self {
        Self {
            package_cache: true,
            build_artifact: true,
            installed_deps: true,
            browser_cache: true,
            ide_cache: true,
            rust_toolchain: true,
            docker: true,
            log_file: true,
            trash: true,
            old_download: true,
            large_file: true,
            duplicate: false,
            macos_specific: true,
            app_cache: true,
            system_junk: true,
            mobile_backup: true,
        }
    }
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: 3600,
            warning_threshold_percent: 85,
            critical_threshold_percent: 95,
        }
    }
}

impl Config {
    pub fn config_path() -> PathBuf {
        dirs::config_dir()
            .unwrap_or_else(|| PathBuf::from("~/.config"))
            .join("sweeprs")
            .join("config.toml")
    }

    pub fn load() -> Result<Self> {
        let path = Self::config_path();
        if path.exists() {
            let contents = std::fs::read_to_string(&path)?;
            let config: Self = toml::from_str(&contents)?;
            Ok(config)
        } else {
            Ok(Self::default())
        }
    }

    pub fn save_default(path: &Path) -> Result<()> {
        let config = Self::default();
        let contents = toml::to_string_pretty(&config)?;
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(path, contents)?;
        Ok(())
    }

    pub fn is_category_enabled(&self, category: crate::scanner::entry::Category) -> bool {
        use crate::scanner::entry::Category;
        match category {
            Category::PackageCache => self.categories.enabled.package_cache,
            Category::BuildArtifact => self.categories.enabled.build_artifact,
            Category::InstalledDeps => self.categories.enabled.installed_deps,
            Category::BrowserCache => self.categories.enabled.browser_cache,
            Category::IdeCache => self.categories.enabled.ide_cache,
            Category::RustToolchain => self.categories.enabled.rust_toolchain,
            Category::Docker => self.categories.enabled.docker,
            Category::LogFile => self.categories.enabled.log_file,
            Category::Trash => self.categories.enabled.trash,
            Category::OldDownload => self.categories.enabled.old_download,
            Category::LargeFile => self.categories.enabled.large_file,
            Category::Duplicate => self.categories.enabled.duplicate,
            Category::MacosSpecific => self.categories.enabled.macos_specific,
            Category::AppCache => self.categories.enabled.app_cache,
            Category::SystemJunk => self.categories.enabled.system_junk,
            Category::MobileBackup => self.categories.enabled.mobile_backup,
        }
    }

    pub fn expand_path(path: &str) -> PathBuf {
        if let Some(stripped) = path.strip_prefix("~/") {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("/"))
                .join(stripped)
        } else {
            PathBuf::from(path)
        }
    }
}
