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
    #[serde(alias = "confirmBeforeDelete")]
    pub confirm_before_delete: bool,
    #[serde(alias = "cliDryRunDefault")]
    pub cli_dry_run_default: bool,
    #[serde(alias = "outputFormat")]
    pub output_format: String,
    #[serde(alias = "globalExcludes")]
    pub global_excludes: Vec<String>,
    /// Default categories for `sweeprs clean` when no category is specified.
    /// If empty, all enabled categories are used (original behavior).
    /// Example: `["cache", "build", "browser", "ide", "app-cache"]`
    #[serde(alias = "defaultCleanCategories")]
    pub default_clean_categories: Vec<String>,
    /// Categories to remove from the default clean list.
    /// Easier than rewriting `default_clean_categories` when you only want to skip a few.
    /// Example: `["trash", "llm"]` keeps all defaults except trash and LLM models.
    #[serde(alias = "excludeCleanCategories")]
    pub exclude_clean_categories: Vec<String>,
    /// Default safety level for `sweeprs clean` when --all is not passed.
    /// "safe" = only Safe items, "caution" = Safe + Caution, "all" = everything.
    #[serde(alias = "defaultCleanSafety")]
    pub default_clean_safety: String,
    /// Default directory for `--archive` mode. If empty, archives are placed
    /// next to the original directory.
    #[serde(alias = "archiveDir")]
    pub archive_dir: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ScanConfig {
    #[serde(alias = "maxDepth")]
    pub max_depth: usize,
    pub threads: usize,
    #[serde(alias = "followSymlinks")]
    pub follow_symlinks: bool,
    /// Project search roots for build/deps/gitignored scanning.
    /// Paths relative to home directory (e.g. "Workspace") or absolute.
    /// Defaults to: Workspace, Projects, Developer, Code, src, dev
    #[serde(alias = "projectRoots")]
    pub project_roots: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CategoriesConfig {
    #[serde(alias = "downloadAgeDays")]
    pub download_age_days: u64,
    #[serde(alias = "largeFileThreshold")]
    pub large_file_threshold: u64,
    #[serde(alias = "largeFileDirs")]
    pub large_file_dirs: Vec<String>,
    #[serde(alias = "enableDuplicates")]
    pub enable_duplicates: bool,
    #[serde(alias = "duplicateMinSize")]
    pub duplicate_min_size: u64,
    #[serde(alias = "duplicateDirs")]
    pub duplicate_dirs: Vec<String>,
    pub enabled: EnabledCategories,
    /// Days since last commit to consider a project stale (default: 90)
    #[serde(alias = "staleProjectDays")]
    pub stale_project_days: u64,
}

#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct EnabledCategories {
    #[serde(alias = "packageCache")]
    pub package_cache: bool,
    #[serde(alias = "buildArtifact")]
    pub build_artifact: bool,
    #[serde(alias = "installedDeps")]
    pub installed_deps: bool,
    #[serde(alias = "browserCache")]
    pub browser_cache: bool,
    #[serde(alias = "ideCache")]
    pub ide_cache: bool,
    #[serde(alias = "rustToolchain")]
    pub rust_toolchain: bool,
    pub docker: bool,
    #[serde(alias = "logFile")]
    pub log_file: bool,
    pub trash: bool,
    #[serde(alias = "oldDownload")]
    pub old_download: bool,
    #[serde(alias = "largeFile")]
    pub large_file: bool,
    pub duplicate: bool,
    #[serde(alias = "macosSpecific")]
    pub macos_specific: bool,
    #[serde(alias = "linuxSpecific")]
    pub linux_specific: bool,
    #[serde(alias = "appCache")]
    pub app_cache: bool,
    #[serde(alias = "systemJunk")]
    pub system_junk: bool,
    #[serde(alias = "mobileBackup")]
    pub mobile_backup: bool,
    #[serde(alias = "llmModels")]
    pub llm_models: bool,
    pub simulator: bool,
    #[serde(alias = "aiTools")]
    pub ai_tools: bool,
    #[serde(alias = "staleProject")]
    pub stale_project: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct MonitorConfig {
    #[serde(alias = "pollIntervalSecs")]
    pub poll_interval_secs: u64,
    #[serde(alias = "warningThresholdPercent")]
    pub warning_threshold_percent: u8,
    #[serde(alias = "criticalThresholdPercent")]
    pub critical_threshold_percent: u8,
    /// When true, the monitor daemon will auto-clean safe items when disk exceeds warning threshold
    #[serde(alias = "autoClean")]
    pub auto_clean: bool,
    /// Categories to auto-clean (same names as CLI args: cache, build, browser, etc.)
    /// If empty, defaults to: cache, build, browser, ide, app-cache
    #[serde(alias = "autoCleanCategories")]
    pub auto_clean_categories: Vec<String>,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            confirm_before_delete: true,
            cli_dry_run_default: true,
            output_format: "table".to_owned(),
            global_excludes: Vec::new(),
            // Default categories cover all safe-to-clean items on a dev machine.
            // Empty list means "all enabled categories" for backwards compat,
            // but we ship explicit defaults so users know what will be cleaned.
            default_clean_categories: {
                let mut cats = vec![
                    "cache".to_owned(),
                    "build".to_owned(),
                    "deps".to_owned(),
                    "browser".to_owned(),
                    "ide".to_owned(),
                    "app-cache".to_owned(),
                    "logs".to_owned(),
                    "system-junk".to_owned(),
                    "stale-project".to_owned(),
                    "docker".to_owned(),
                    "toolchain".to_owned(),
                    "trash".to_owned(),
                    "llm".to_owned(),
                    "mobile-backup".to_owned(),
                ];
                if cfg!(target_os = "macos") {
                    cats.push("macos".to_owned());
                }
                if cfg!(target_os = "linux") {
                    cats.push("linux".to_owned());
                }
                cats
            },
            exclude_clean_categories: Vec::new(),
            // "caution" = Safe + Caution items. On a dev machine, caution-level
            // items (temp files, old downloads, stale project artifacts, docker
            // images) are all regeneratable.
            default_clean_safety: "caution".to_owned(),
            archive_dir: None,
        }
    }
}

impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            max_depth: 10,
            threads: 0,
            follow_symlinks: false,
            project_roots: Vec::new(),
        }
    }
}

impl Default for CategoriesConfig {
    fn default() -> Self {
        Self {
            download_age_days: 30,
            large_file_threshold: 262_144_000, // 250 MB
            large_file_dirs: vec!["~/Downloads".to_owned(), "~/Desktop".to_owned()],
            enable_duplicates: false,
            duplicate_min_size: 1_048_576,
            duplicate_dirs: Vec::new(),
            enabled: EnabledCategories::default(),
            stale_project_days: 60,
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
            linux_specific: true,
            app_cache: true,
            system_junk: true,
            mobile_backup: true,
            llm_models: true,
            simulator: true,
            ai_tools: true,
            stale_project: true,
        }
    }
}

impl Default for MonitorConfig {
    fn default() -> Self {
        Self {
            poll_interval_secs: 3600,
            warning_threshold_percent: 80,
            critical_threshold_percent: 90,
            auto_clean: true,
            auto_clean_categories: vec![
                "cache".to_owned(),
                "build".to_owned(),
                "browser".to_owned(),
                "ide".to_owned(),
                "app-cache".to_owned(),
                "system-junk".to_owned(),
                "logs".to_owned(),
                "docker".to_owned(),
                "toolchain".to_owned(),
            ],
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

    /// Load a per-project .sweeprsrc config and merge it with the global config.
    /// Project config values override global config values.
    #[allow(dead_code)]
    pub fn load_with_project(project_dir: &Path) -> Result<Self> {
        let mut config = Self::load()?;

        let rc_path = project_dir.join(".sweeprsrc");
        if rc_path.exists() {
            let contents = std::fs::read_to_string(&rc_path)?;
            let project_config: Self = toml::from_str(&contents)?;
            // Merge: project overrides global for non-default values
            if !project_config.general.global_excludes.is_empty() {
                config
                    .general
                    .global_excludes
                    .extend(project_config.general.global_excludes);
            }
            if !project_config.scan.project_roots.is_empty() {
                config.scan.project_roots = project_config.scan.project_roots;
            }
        }

        Ok(config)
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
            Category::Toolchain => self.categories.enabled.rust_toolchain,
            Category::Docker => self.categories.enabled.docker,
            Category::LogFile => self.categories.enabled.log_file,
            Category::Trash => self.categories.enabled.trash,
            Category::OldDownload => self.categories.enabled.old_download,
            Category::LargeFile => self.categories.enabled.large_file,
            Category::Duplicate => self.categories.enabled.duplicate,
            Category::MacosSpecific => self.categories.enabled.macos_specific,
            Category::LinuxSpecific => self.categories.enabled.linux_specific,
            Category::AppCache => self.categories.enabled.app_cache,
            Category::SystemJunk => self.categories.enabled.system_junk,
            Category::MobileBackup => self.categories.enabled.mobile_backup,
            Category::LlmModels => self.categories.enabled.llm_models,
            Category::Simulator => self.categories.enabled.simulator,
            Category::AiTools => self.categories.enabled.ai_tools,
            Category::StaleProject => self.categories.enabled.stale_project,
        }
    }

    /// Parse a category name string (as used in CLI args / config) into a `Category`.
    pub fn parse_category(name: &str) -> Option<crate::scanner::entry::Category> {
        use crate::scanner::entry::Category;
        match name {
            "cache" => Some(Category::PackageCache),
            "build" => Some(Category::BuildArtifact),
            "deps" => Some(Category::InstalledDeps),
            "browser" => Some(Category::BrowserCache),
            "ide" => Some(Category::IdeCache),
            "toolchain" => Some(Category::Toolchain),
            "docker" => Some(Category::Docker),
            "logs" => Some(Category::LogFile),
            "trash" => Some(Category::Trash),
            "downloads" => Some(Category::OldDownload),
            "large-files" => Some(Category::LargeFile),
            "duplicates" => Some(Category::Duplicate),
            "macos" => Some(Category::MacosSpecific),
            "linux" => Some(Category::LinuxSpecific),
            "app-cache" => Some(Category::AppCache),
            "system-junk" => Some(Category::SystemJunk),
            "mobile-backup" => Some(Category::MobileBackup),
            "llm" | "llm-models" => Some(Category::LlmModels),
            "simulator" | "simulators" => Some(Category::Simulator),
            "ai" | "ai-tools" => Some(Category::AiTools),
            "stale-project" | "stale-projects" => Some(Category::StaleProject),
            _ => None,
        }
    }

    /// Get the default clean categories from config, or None if not configured (use all).
    /// Applies `exclude_clean_categories` as a deny-list on top of the include list.
    pub fn default_clean_categories(&self) -> Option<Vec<crate::scanner::entry::Category>> {
        if self.general.default_clean_categories.is_empty() {
            return None;
        }

        // Parse the exclude list into a set for O(1) lookup
        let excludes: rustc_hash::FxHashSet<_> = self
            .general
            .exclude_clean_categories
            .iter()
            .filter_map(|s| Self::parse_category(s))
            .collect();

        let cats: Vec<_> = self
            .general
            .default_clean_categories
            .iter()
            .filter_map(|s| Self::parse_category(s))
            .filter(|cat| !excludes.contains(cat))
            .collect();
        if cats.is_empty() { None } else { Some(cats) }
    }

    /// Get the auto-clean categories from config.
    pub fn auto_clean_categories(&self) -> Vec<crate::scanner::entry::Category> {
        self.monitor
            .auto_clean_categories
            .iter()
            .filter_map(|s| Self::parse_category(s))
            .collect()
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
