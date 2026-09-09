mod cleaner;
mod commands;
mod config;
mod filter;
mod monitor;
mod output;
mod platform;
mod rules;
mod scanner;
mod tui;
mod util;
mod virtual_entry;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

use scanner::entry::Category;

#[derive(Parser)]
#[command(name = "sweeprs", version, about = "Fast disk cleanup TUI")]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Subcommand)]
enum Command {
    /// Scan for reclaimable disk space
    Scan {
        /// Output as JSON
        #[arg(long)]
        json: bool,
        /// Scan specific category
        #[arg(short, long)]
        category: Option<CategoryArg>,
        /// Only show entries matching this glob (repeatable)
        #[arg(short, long = "filter")]
        filters: Vec<String>,
        /// Exclude entries matching this glob (repeatable)
        #[arg(short = 'E', long)]
        exclude: Vec<String>,
        /// Only show entries at least this large (e.g. 1G, 500M, 100K)
        #[arg(long)]
        min_size: Option<String>,
        /// Save scan results to a JSON file
        #[arg(long)]
        save: Option<String>,
        /// Load scan results from a JSON file instead of scanning
        #[arg(long)]
        load: Option<String>,
    },
    /// Clean up disk space
    ///
    /// Pass one or more categories to clean specific ones, or omit for defaults.
    /// Examples:
    ///   sweeprs clean --force              # clean default categories from config (or all if unset)
    ///   sweeprs clean cache build --force   # clean only cache + build
    ///   sweeprs clean llm --force --all     # clean LLM models including Caution items
    Clean {
        /// Categories to clean (omit for config defaults, or "all" for everything)
        #[arg(value_enum)]
        targets: Vec<CleanTarget>,
        /// Actually delete (default is dry-run)
        #[arg(long)]
        force: bool,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
        /// Include Caution and Danger items (default: Safe only, overridden by config)
        #[arg(short, long)]
        all: bool,
        /// Only clean entries matching this glob (repeatable)
        #[arg(short, long = "filter")]
        filters: Vec<String>,
        /// Exclude entries matching this glob (repeatable)
        #[arg(short = 'E', long)]
        exclude: Vec<String>,
        /// Only clean entries at least this large (e.g. 1G, 500M, 100K)
        #[arg(long)]
        min_size: Option<String>,
        /// Compress directories into .tar.zst archives instead of deleting
        #[arg(long)]
        archive: bool,
        /// Directory to store archives (default: next to original)
        #[arg(long)]
        archive_dir: Option<String>,
    },
    /// Background disk usage monitor
    Monitor {
        /// Stop running monitor
        #[arg(long)]
        stop: bool,
        /// Show monitor status
        #[arg(long)]
        status: bool,
        /// Run in foreground
        #[arg(long)]
        foreground: bool,
        /// Install as launchd service (starts on login)
        #[arg(long)]
        install: bool,
        /// Uninstall launchd service
        #[arg(long)]
        uninstall: bool,
        /// Auto-clean safe items when disk exceeds warning threshold
        #[arg(long)]
        auto_clean: bool,
    },
    /// Manage configuration
    Config {
        /// Generate default config file
        #[arg(long)]
        init: bool,
        /// Overwrite an existing config file with the defaults
        #[arg(long)]
        force: bool,
        /// Show current config path
        #[arg(long)]
        path: bool,
    },
    /// List all scan categories
    Categories,
    /// Update sweeprs to the latest version
    Upgrade,
    /// Generate shell completions
    Completions {
        /// Shell to generate completions for (auto-detected if omitted)
        #[arg(value_enum)]
        shell: Option<clap_complete::Shell>,
        /// Install completions to shell config file
        #[arg(long)]
        install: bool,
    },
}

#[derive(Clone, ValueEnum)]
enum CategoryArg {
    Cache,
    Build,
    Deps,
    Browser,
    Ide,
    Toolchain,
    Docker,
    Logs,
    Trash,
    Downloads,
    LargeFiles,
    Duplicates,
    Macos,
    Linux,
    AppCache,
    SystemJunk,
    MobileBackup,
    Llm,
    Simulator,
    Ai,
    AgentSessions,
    StaleProject,
}

impl CategoryArg {
    fn to_category(&self) -> Category {
        match self {
            Self::Cache => Category::PackageCache,
            Self::Build => Category::BuildArtifact,
            Self::Deps => Category::InstalledDeps,
            Self::Browser => Category::BrowserCache,
            Self::Ide => Category::IdeCache,
            Self::Toolchain => Category::Toolchain,
            Self::Docker => Category::Docker,
            Self::Logs => Category::LogFile,
            Self::Trash => Category::Trash,
            Self::Downloads => Category::OldDownload,
            Self::LargeFiles => Category::LargeFile,
            Self::Duplicates => Category::Duplicate,
            Self::Macos => Category::MacosSpecific,
            Self::Linux => Category::LinuxSpecific,
            Self::AppCache => Category::AppCache,
            Self::SystemJunk => Category::SystemJunk,
            Self::MobileBackup => Category::MobileBackup,
            Self::Llm => Category::LlmModels,
            Self::Simulator => Category::Simulator,
            Self::Ai => Category::AiTools,
            Self::AgentSessions => Category::AgentSession,
            Self::StaleProject => Category::StaleProject,
        }
    }
}

#[derive(Clone, ValueEnum)]
enum CleanTarget {
    All,
    Cache,
    Build,
    Deps,
    Browser,
    Ide,
    Toolchain,
    Docker,
    Logs,
    Trash,
    Downloads,
    LargeFiles,
    Duplicates,
    Macos,
    Linux,
    AppCache,
    SystemJunk,
    MobileBackup,
    Llm,
    Simulator,
    Ai,
    AgentSessions,
    StaleProject,
}

impl CleanTarget {
    fn to_category(&self) -> Option<Category> {
        match self {
            Self::All => None,
            Self::Cache => Some(Category::PackageCache),
            Self::Build => Some(Category::BuildArtifact),
            Self::Deps => Some(Category::InstalledDeps),
            Self::Browser => Some(Category::BrowserCache),
            Self::Ide => Some(Category::IdeCache),
            Self::Toolchain => Some(Category::Toolchain),
            Self::Docker => Some(Category::Docker),
            Self::Logs => Some(Category::LogFile),
            Self::Trash => Some(Category::Trash),
            Self::Downloads => Some(Category::OldDownload),
            Self::LargeFiles => Some(Category::LargeFile),
            Self::Duplicates => Some(Category::Duplicate),
            Self::Macos => Some(Category::MacosSpecific),
            Self::Linux => Some(Category::LinuxSpecific),
            Self::AppCache => Some(Category::AppCache),
            Self::SystemJunk => Some(Category::SystemJunk),
            Self::MobileBackup => Some(Category::MobileBackup),
            Self::Llm => Some(Category::LlmModels),
            Self::Simulator => Some(Category::Simulator),
            Self::Ai => Some(Category::AiTools),
            Self::AgentSessions => Some(Category::AgentSession),
            Self::StaleProject => Some(Category::StaleProject),
        }
    }
}

/// Build an `EntryFilter` from CLI args (merged with config `global_excludes`) and apply it
/// to a `ScanResult` in place.
fn apply_filter(
    result: &mut scanner::entry::ScanResult,
    filters: &[String],
    exclude: &[String],
    min_size: Option<String>,
    config: &config::Config,
) -> Result<()> {
    let min_bytes = min_size
        .map(|s| filter::parse_size(&s))
        .transpose()?
        .unwrap_or(0);

    // Merge CLI --exclude with config global_excludes
    let mut all_excludes: Vec<String> = config.general.global_excludes.clone();
    all_excludes.extend_from_slice(exclude);

    let entry_filter = filter::EntryFilter::new(filters, &all_excludes, min_bytes)?;
    if entry_filter.is_active() {
        let kept: Vec<_> = entry_filter
            .apply(&result.entries)
            .into_iter()
            .cloned()
            .collect();
        result.total_size = kept.iter().map(|e| e.size).sum();
        result.entries = kept;
    }
    Ok(())
}

fn print_scan_summary(result: &scanner::entry::ScanResult) {
    let duration_str = result
        .scan_duration_secs
        .map(|d| format!(" in {d:.1}s"))
        .unwrap_or_default();
    eprintln!(
        "Scan complete: {} items, {} found{duration_str}",
        result.entries.len(),
        util::human_size(result.total_size),
    );
}

#[allow(clippy::too_many_lines)]
fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => tui::run()?,
        Some(Command::Scan {
            json,
            category,
            filters,
            exclude,
            min_size,
            save,
            load,
        }) => {
            let config = config::Config::load()?;
            let mut result = if let Some(ref load_path) = load {
                let data = std::fs::read_to_string(load_path)?;
                serde_json::from_str(&data)?
            } else if json {
                if let Some(cat) = category {
                    scanner::scan_category(&config, cat.to_category())?
                } else {
                    scanner::scan_all(&config)?
                }
            } else if let Some(cat) = category {
                scanner::scan_category_with_progress(&config, cat.to_category())?
            } else {
                scanner::scan_all_with_progress(&config)?
            };

            apply_filter(&mut result, &filters, &exclude, min_size, &config)?;

            if let Some(ref save_path) = save {
                let json_data = serde_json::to_string_pretty(&result)?;
                std::fs::write(save_path, json_data)?;
                eprintln!("Scan results saved to: {save_path}");
            }

            if json {
                output::print_json(&result)?;
            } else {
                print_scan_summary(&result);
                output::print_table(&result);
            }
        }
        Some(Command::Clean {
            targets,
            force,
            yes,
            all,
            filters,
            exclude,
            min_size,
            archive,
            archive_dir,
        }) => {
            let config = config::Config::load()?;

            // Resolve which categories to clean:
            // 1. CLI args override everything
            // 2. If no CLI args, use config default_clean_categories
            // 3. If config is empty too, scan all
            let categories: Option<Vec<Category>> = if targets.is_empty() {
                // No CLI targets -> use config defaults
                config.default_clean_categories()
            } else if targets.iter().any(|t| matches!(t, CleanTarget::All)) {
                // Explicit "all"
                None
            } else {
                // Specific categories from CLI
                let cats: Vec<Category> = targets
                    .iter()
                    .filter_map(CleanTarget::to_category)
                    .collect();
                if cats.is_empty() { None } else { Some(cats) }
            };

            let mut result = match categories {
                None => scanner::scan_all_with_progress(&config)?,
                Some(ref cats) if cats.len() == 1 => {
                    scanner::scan_category_with_progress(&config, cats[0])?
                }
                Some(ref cats) => scanner::scan_categories_with_progress(&config, cats)?,
            };

            apply_filter(&mut result, &filters, &exclude, min_size, &config)?;
            print_scan_summary(&result);

            // Resolve safety level: CLI --all overrides, then config default_clean_safety
            let include_unsafe = if all {
                true
            } else {
                matches!(
                    config.general.default_clean_safety.as_str(),
                    "caution" | "all"
                )
            };

            // CLI --archive-dir overrides config archive_dir
            let resolved_archive_dir = archive_dir.map(std::path::PathBuf::from).or_else(|| {
                config
                    .general
                    .archive_dir
                    .as_ref()
                    .map(|s| config::Config::expand_path(s))
            });

            let action = if archive {
                cleaner::CleanAction::Archive(resolved_archive_dir)
            } else {
                cleaner::CleanAction::Delete
            };

            let options = cleaner::CleanOptions {
                dry_run: !force,
                skip_confirm: yes,
                include_unsafe,
                action,
                config: config.clone(),
            };
            cleaner::clean(&result.entries, &options)?;
        }
        Some(Command::Monitor {
            stop,
            status,
            foreground,
            install,
            uninstall,
            auto_clean,
        }) => {
            if install {
                monitor::install()?;
            } else if uninstall {
                monitor::uninstall()?;
            } else if stop {
                monitor::stop()?;
            } else if status {
                monitor::status()?;
            } else if foreground {
                monitor::run_foreground(auto_clean)?;
            } else {
                monitor::start()?;
            }
        }
        Some(Command::Config { init, force, path }) => {
            if init {
                let config_path = config::Config::config_path();
                // Writing the defaults over a config someone has tuned discards
                // their excludes and category choices with no way back.
                if config_path.exists() && !force {
                    anyhow::bail!(
                        "Config already exists at {}. Pass --force to replace it with the defaults.",
                        config_path.display()
                    );
                }
                config::Config::save_default(&config_path)?;
                println!("Config written to: {}", config_path.display());
            } else if path {
                println!("{}", config::Config::config_path().display());
            } else {
                let config_path = config::Config::config_path();
                if config_path.exists() {
                    println!("Config: {}", config_path.display());
                } else {
                    println!("No config file. Run `sweeprs config --init` to create one.");
                }
            }
        }
        Some(Command::Categories) => {
            println!("{:<20} {:<10} CLI ARG", "CATEGORY", "SAFETY");
            println!("{}", "-".repeat(50));
            for cat in Category::ALL {
                let arg = match cat {
                    Category::PackageCache => "cache",
                    Category::BuildArtifact => "build",
                    Category::InstalledDeps => "deps",
                    Category::BrowserCache => "browser",
                    Category::IdeCache => "ide",
                    Category::Toolchain => "toolchain",
                    Category::Docker => "docker",
                    Category::LogFile => "logs",
                    Category::Trash => "trash",
                    Category::OldDownload => "downloads",
                    Category::LargeFile => "large-files",
                    Category::Duplicate => "duplicates",
                    Category::MacosSpecific => "macos",
                    Category::LinuxSpecific => "linux",
                    Category::AppCache => "app-cache",
                    Category::SystemJunk => "system-junk",
                    Category::MobileBackup => "mobile-backup",
                    Category::LlmModels => "llm",
                    Category::Simulator => "simulator",
                    Category::AiTools => "ai",
                    Category::AgentSession => "agent-sessions",
                    Category::StaleProject => "stale-project",
                };
                println!("{:<20} {:<10} {}", cat, cat.default_safety(), arg);
            }
        }
        Some(Command::Upgrade) => commands::upgrade::execute()?,
        Some(Command::Completions { shell, install }) => {
            commands::completions::execute(shell, install)?;
        }
    }

    Ok(())
}
