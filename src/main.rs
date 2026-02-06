mod cleaner;
mod config;
mod filter;
mod monitor;
mod output;
mod platform;
mod rules;
mod scanner;
mod tui;
mod util;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

use scanner::entry::Category;

#[derive(Parser)]
#[command(name = "sweeprs", version, about = "Fast macOS disk cleanup TUI")]
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
    },
    /// Clean up disk space
    Clean {
        /// Category to clean (defaults to "all")
        #[arg(default_value = "all")]
        target: CleanTarget,
        /// Actually delete (default is dry-run)
        #[arg(long)]
        force: bool,
        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
        /// Include Caution and Danger items (default: Safe only)
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
    },
    /// Manage configuration
    Config {
        /// Generate default config file
        #[arg(long)]
        init: bool,
        /// Show current config path
        #[arg(long)]
        path: bool,
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
    AppCache,
    SystemJunk,
    MobileBackup,
}

impl CategoryArg {
    fn to_category(&self) -> Category {
        match self {
            Self::Cache => Category::PackageCache,
            Self::Build => Category::BuildArtifact,
            Self::Deps => Category::InstalledDeps,
            Self::Browser => Category::BrowserCache,
            Self::Ide => Category::IdeCache,
            Self::Toolchain => Category::RustToolchain,
            Self::Docker => Category::Docker,
            Self::Logs => Category::LogFile,
            Self::Trash => Category::Trash,
            Self::Downloads => Category::OldDownload,
            Self::LargeFiles => Category::LargeFile,
            Self::Duplicates => Category::Duplicate,
            Self::Macos => Category::MacosSpecific,
            Self::AppCache => Category::AppCache,
            Self::SystemJunk => Category::SystemJunk,
            Self::MobileBackup => Category::MobileBackup,
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
    AppCache,
    SystemJunk,
    MobileBackup,
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
            Self::Toolchain => Some(Category::RustToolchain),
            Self::Docker => Some(Category::Docker),
            Self::Logs => Some(Category::LogFile),
            Self::Trash => Some(Category::Trash),
            Self::Downloads => Some(Category::OldDownload),
            Self::LargeFiles => Some(Category::LargeFile),
            Self::Duplicates => Some(Category::Duplicate),
            Self::Macos => Some(Category::MacosSpecific),
            Self::AppCache => Some(Category::AppCache),
            Self::SystemJunk => Some(Category::SystemJunk),
            Self::MobileBackup => Some(Category::MobileBackup),
        }
    }
}

/// Build an `EntryFilter` from CLI args and apply it to a `ScanResult` in place.
fn apply_filter(
    result: &mut scanner::entry::ScanResult,
    filters: &[String],
    exclude: &[String],
    min_size: Option<String>,
) -> Result<()> {
    let min_bytes = min_size.map(|s| filter::parse_size(&s)).transpose()?.unwrap_or(0);
    let entry_filter = filter::EntryFilter::new(filters, exclude, min_bytes)?;
    if entry_filter.is_active() {
        let kept: Vec<_> = entry_filter.apply(&result.entries).into_iter().cloned().collect();
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

fn main() -> Result<()> {
    let cli = Cli::parse();

    match cli.command {
        None => tui::run()?,
        Some(Command::Scan { json, category, filters, exclude, min_size }) => {
            let config = config::Config::load()?;
            let mut result = if json {
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

            apply_filter(&mut result, &filters, &exclude, min_size)?;

            if json {
                output::print_json(&result)?;
            } else {
                print_scan_summary(&result);
                output::print_table(&result);
            }
        }
        Some(Command::Clean { target, force, yes, all, filters, exclude, min_size }) => {
            let config = config::Config::load()?;
            let mut result = if let Some(category) = target.to_category() {
                scanner::scan_category_with_progress(&config, category)?
            } else {
                scanner::scan_all_with_progress(&config)?
            };

            apply_filter(&mut result, &filters, &exclude, min_size)?;
            print_scan_summary(&result);

            let options = cleaner::CleanOptions {
                dry_run: !force,
                skip_confirm: yes,
                include_unsafe: all,
            };
            cleaner::clean(&result.entries, &options)?;
        }
        Some(Command::Monitor {
            stop,
            status,
            foreground,
        }) => {
            if stop {
                monitor::stop()?;
            } else if status {
                monitor::status()?;
            } else if foreground {
                monitor::run_foreground()?;
            } else {
                monitor::start()?;
            }
        }
        Some(Command::Config { init, path }) => {
            if init {
                let config_path = config::Config::config_path();
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
    }

    Ok(())
}
