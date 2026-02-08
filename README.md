```

   _______ _      __ ___  ___  ____  _____
  / ___/ | | /| / // _ \/ _ \/ __ \/ ___/
 (__  )| |/ |/ // __/  __/ /_/ / /
/____/ |__/|__/ \___/\___/ .___/_/
                         /_/
```

# sweeprs

Fast macOS disk cleanup TUI and CLI written in Rust.

Scans your system for reclaimable disk space across 17 categories and 23+ rule modules,
presents results in an interactive terminal UI or structured CLI output, and cleans up
safely with dry-run by default.

---

## Features

- **Interactive TUI** -- tree-based browser with real-time scan progress, search/filter, and clipboard support
- **17 scan categories** -- caches, build artifacts, dependencies, Docker, Homebrew, LLM models, cloud CLIs, and more
- **Safety levels** -- entries classified as Safe, Caution, or Danger with safe-only cleanup by default
- **Multi-category clean** -- clean multiple categories in one command: `sweeprs clean cache build deps --force`
- **Default clean categories** -- configure which categories to clean by default so `sweeprs clean --force` does the right thing
- **Glob filtering** -- `--filter` and `--exclude` patterns to narrow results by path
- **Size filtering** -- `--min-size` to focus on large items
- **Global excludes** -- configure paths to never touch in your config file
- **Parallel scanning** -- rayon-powered concurrent rule execution
- **macOS optimized** -- `getattrlistbulk` syscall for batched file metadata reads
- **Streaming results** -- TUI updates as each rule completes, no waiting for full scan
- **Docker and Homebrew** -- detects and prunes containers, images, volumes, caches, and unneeded formulae
- **Gitignore-aware** -- finds large directories ignored by git in your projects
- **LLM model detection** -- finds Ollama, HuggingFace, LM Studio, GPT4All, Jan AI, and llama.cpp caches
- **Duplicate detection** -- XXH3-based file deduplication (opt-in)
- **Background monitor** -- daemon that alerts when disk usage exceeds thresholds
- **JSON output** -- machine-readable scan results with save/load support
- **Shell completions** -- auto-generated completions for bash, zsh, fish, and more
- **Self-update** -- `sweeprs upgrade` checks for and installs the latest version
- **Dry-run by default** -- never deletes anything unless you pass `--force`
- **Configurable** -- TOML config with camelCase support for thresholds, excludes, defaults, and category toggles

## Installation

### Quick install (latest release)

```bash
curl -fsSL https://raw.githubusercontent.com/salamaashoush/sweeprs/main/scripts/install.sh | bash
```

Or install a specific version:

```bash
VERSION=0.2.0 curl -fsSL https://raw.githubusercontent.com/salamaashoush/sweeprs/main/scripts/install.sh | bash
```

Custom install directory:

```bash
INSTALL_DIR=~/.local/bin curl -fsSL https://raw.githubusercontent.com/salamaashoush/sweeprs/main/scripts/install.sh | bash
```

### Build from source

Requires Rust 1.85+ (nightly recommended for edition 2024).

```bash
cargo install --path .
```

Or:

```bash
git clone https://github.com/salamaashoush/sweeprs.git
cd sweeprs
cargo build --release
# Binary at target/release/sweeprs
```

## Usage

### Interactive TUI

```bash
sweeprs
```

Launches the full-screen terminal interface. Scans automatically on startup.

### Scan

```bash
# Scan everything
sweeprs scan

# Scan a specific category
sweeprs scan --category build

# Output as JSON
sweeprs scan --json

# Save scan results to file
sweeprs scan --save results.json

# Load previous scan results
sweeprs scan --load results.json

# Only show entries matching a pattern
sweeprs scan -f 'node_modules'
sweeprs scan -f '~/Projects/**'

# Exclude patterns
sweeprs scan -E node_modules -E '.venv'

# Only entries larger than 1 GiB
sweeprs scan --min-size 1G

# Combine filters
sweeprs scan -c cache -E '**/homebrew/**' --min-size 500M
```

### Clean

```bash
# Dry-run clean using config defaults (shows what would be deleted)
sweeprs clean

# Actually delete using config defaults
sweeprs clean --force

# Clean specific categories
sweeprs clean cache build --force

# Clean multiple categories at once
sweeprs clean cache build deps browser --force

# Clean everything (all categories)
sweeprs clean all --force

# Include Caution and Danger items (default: Safe only)
sweeprs clean all --force --all

# Clean only matching entries
sweeprs clean all --force -f '~/Projects/old-app/**'

# Skip confirmation prompt
sweeprs clean all --force --yes

# Clean with size filter
sweeprs clean --force --min-size 1G
```

### Common Recipes

```bash
# Delete all Rust target/ directories across every project
sweeprs clean build --force -f 'target' -y

# Delete all node_modules/ directories
sweeprs clean deps --force -y

# Clean everything over 1 GiB
sweeprs clean --force --min-size 1G

# Clean all caches except Homebrew
sweeprs clean cache --force -E '**/homebrew/**' -y

# Clean everything under a specific workspace
sweeprs clean --force -f '~/Projects/old-workspace/**' -y

# Nuke all node_modules except in one project
sweeprs clean deps --force -E '~/Projects/main-app/**' -y

# Preview what big items exist (dry-run, no deletion)
sweeprs clean --min-size 500M

# Clean Docker images and build cache
sweeprs clean docker --force --all -y

# Clean LLM models (Ollama, HuggingFace, LM Studio, etc.)
sweeprs clean llm --force --all -y

# Clean Homebrew outdated cache and unneeded formulae
sweeprs clean cache --force -f 'brew:*' -y

# Clean caches and build artifacts together
sweeprs clean cache build --force -y

# Export full scan to JSON for scripting
sweeprs scan --json > scan-results.json

# Find the biggest items across all categories
sweeprs scan --min-size 2G
```

### List Categories

```bash
# Show all available categories with safety levels and CLI args
sweeprs categories
```

### Monitor

```bash
# Start background disk usage monitor
sweeprs monitor

# Check status
sweeprs monitor --status

# Stop the monitor
sweeprs monitor --stop

# Run in foreground (for debugging)
sweeprs monitor --foreground
```

### Shell Completions

```bash
# Auto-detect shell and print completions
sweeprs completions

# Generate for a specific shell
sweeprs completions bash
sweeprs completions zsh
sweeprs completions fish

# Install completions to your shell config
sweeprs completions --install
```

### Self-Update

```bash
# Update to the latest version
sweeprs upgrade
```

### Config

```bash
# Generate default config file
sweeprs config --init

# Show config file location
sweeprs config --path
```

## TUI Keybindings

| Key | Action |
|---|---|
| `j` / `Down` | Move cursor down |
| `k` / `Up` | Move cursor up |
| `l` / `Right` / `Enter` | Expand node or move to first child |
| `h` / `Left` | Collapse node or jump to parent |
| `Space` | Toggle selection |
| `d` | Delete selected items |
| `r` | Rescan |
| `g` | Jump to top |
| `G` | Jump to bottom |
| `o` | Reveal in Finder |
| `y` | Copy path to clipboard |
| `/` | Search / filter entries |
| `Esc` | Clear search or go back |
| `q` | Quit |
| `Ctrl+C` | Force quit |

**Search mode** (`/`): Type to filter entries by name. Press `Enter` to apply the filter, `Esc` to cancel. While a search filter is active, only matching entries are shown in the tree.

## Categories

| Category | CLI Arg | Safety | What it finds |
|---|---|---|---|
| Package Caches | `cache` | Safe | npm, yarn, pnpm, bun, cargo, pip, go, maven, bundler, neovim caches |
| Build Artifacts | `build` | Safe | Rust target/, Xcode, Maven, Gradle, CMake outputs, gitignored dirs |
| Installed Dependencies | `deps` | Safe | node_modules/, .venv/, vendor/ |
| Browser Caches | `browser` | Safe | Chrome, Safari, Firefox caches |
| IDE Caches | `ide` | Safe | VS Code, Cursor, JetBrains, Xcode caches |
| App Caches | `app-cache` | Safe | Slack, Spotify, Discord, Teams caches |
| Rust Toolchains | `toolchain` | Caution | Old rustup toolchains, Python/Node/Ruby versions, Conda environments |
| Docker | `docker` | Caution | Images, containers, volumes, build cache |
| Log Files | `logs` | Caution | System logs, diagnostic reports, /var/log, /Library/Logs |
| Old Downloads | `downloads` | Caution | Downloads older than configurable age (default 90 days) |
| macOS Specific | `macos` | Caution | Xcode simulators, QuickLook, Mail caches, Xcode playgrounds |
| System Junk | `system-junk` | Caution | Temp files (/tmp, $TMPDIR), software update cache, cloud CLI caches |
| Mobile Backups | `mobile-backup` | Caution | iOS device backups, Android SDK/emulator caches |
| LLM Models | `llm` | Caution | Ollama, HuggingFace, LM Studio, GPT4All, Jan AI, llama.cpp |
| Trash | `trash` | Danger | ~/.Trash contents |
| Large Files | `large-files` | Danger | Files over 500 MB (configurable) |
| Duplicates | `duplicates` | Danger | Identical files by content hash (disabled by default) |

### Additional Rule Modules

Beyond the main categories above, sweeprs includes specialized rules for:

- **Conda/Mamba** -- miniconda3, anaconda3, miniforge3, mambaforge environments and package caches
- **Android** -- SDK system images, Gradle wrapper distributions, daemon logs, AVD emulators
- **Python** -- `__pycache__` directories across all projects
- **Containers** -- Podman, Lima VMs, Colima (alternative Docker runtimes)
- **Cloud CLIs** -- gcloud, AWS CLI, Terraform plugins, Azure CLI caches
- **Homebrew** -- outdated downloads, autoremove candidates

## Filter Patterns

Patterns use glob syntax powered by the `globset` crate.

**Basename matching** -- patterns without `/` match the final path component:
```bash
sweeprs scan -f 'node_modules'    # matches any node_modules/ anywhere
sweeprs scan -f 'target'         # matches any target/ anywhere
sweeprs scan -E '*.log'           # exclude anything ending in .log
```

**Full path matching** -- patterns with `/` match against the absolute path:
```bash
sweeprs scan -f '~/Projects/**'                # everything under Projects/
sweeprs scan -f '/Users/me/Projects/foo/**'   # specific project subtree
sweeprs scan -E '~/Projects/important-app/**' # exclude a specific project
```

**Tilde expansion** -- `~` is expanded to your home directory automatically.

**Multiple patterns** -- repeat `-f` or `-E` for multiple patterns (ORed together):
```bash
sweeprs scan -f 'node_modules' -f '.cache' -f 'target'
sweeprs scan -E 'node_modules' -E '.venv' -E 'target'
```

**Size filter** -- `--min-size` accepts `K`, `M`, `G` suffixes (binary units):
```bash
sweeprs scan --min-size 100M
sweeprs scan --min-size 1G
sweeprs clean all --force --min-size 500M
```

**Global excludes** -- set patterns in your config to always exclude certain paths:
```toml
[general]
global_excludes = ["~/Projects/important-app/**", "*.iso"]
```

These are merged with any CLI `--exclude` patterns automatically.

## Configuration

Config file location: `~/Library/Application Support/sweeprs/config.toml` (macOS)

Generate a default config with `sweeprs config --init`.

All multi-word keys accept both `snake_case` and `camelCase`:
```toml
# Both of these work:
confirm_before_delete = true
confirmBeforeDelete = true
```

### Full Config Reference

```toml
[general]
confirm_before_delete = true    # Prompt before deletion in TUI
cli_dry_run_default = true      # CLI clean is dry-run unless --force
output_format = "table"         # "table" or "json"
global_excludes = []            # Glob patterns to always exclude

# Default categories for `sweeprs clean` when no category is specified.
# If empty, all enabled categories are used.
# Example: ["cache", "build", "browser", "ide", "app-cache"]
default_clean_categories = []

# Default safety level for `sweeprs clean` when --all is not passed.
# "safe" = only Safe items, "caution" = Safe + Caution, "all" = everything
default_clean_safety = "safe"

[scan]
max_depth = 10                  # Max directory traversal depth
threads = 0                     # 0 = auto-detect CPU count
follow_symlinks = false         # Follow symbolic links during scan

[categories]
download_age_days = 90          # Age threshold for old downloads
large_file_threshold = 524288000  # 500 MB - threshold for large file detection
large_file_dirs = ["~/Downloads", "~/Desktop"]
enable_duplicates = false       # Duplicate detection is opt-in (CPU-intensive)
duplicate_min_size = 1048576    # 1 MB - minimum size for duplicate scan
duplicate_dirs = []             # Directories to scan for duplicates

# Toggle individual categories on/off
[categories.enabled]
package_cache = true
build_artifact = true
installed_deps = true
browser_cache = true
ide_cache = true
rust_toolchain = true
docker = true
log_file = true
trash = true
old_download = true
large_file = true
duplicate = false               # Disabled by default
macos_specific = true
app_cache = true
system_junk = true
mobile_backup = true
llm_models = true

[monitor]
poll_interval_secs = 3600       # Check interval (1 hour)
warning_threshold_percent = 85  # Warn at this disk usage %
critical_threshold_percent = 95 # Critical alert at this %
```

### Recommended Config for Automated Cleanup

For CI or cron jobs that should clean common safe caches automatically:

```toml
[general]
confirm_before_delete = false
cli_dry_run_default = false
default_clean_categories = ["cache", "build", "browser", "ide", "app-cache"]
default_clean_safety = "safe"
```

Then run `sweeprs clean -y` to clean those categories without any prompts.

### Category CLI Arg Reference

Use these names with `sweeprs scan --category` or `sweeprs clean`:

| CLI Arg | Category |
|---|---|
| `cache` | Package Caches |
| `build` | Build Artifacts |
| `deps` | Installed Dependencies |
| `browser` | Browser Caches |
| `ide` | IDE Caches |
| `toolchain` | Rust/Python/Node Toolchains |
| `docker` | Docker |
| `logs` | Log Files |
| `trash` | Trash |
| `downloads` | Old Downloads |
| `large-files` | Large Files |
| `duplicates` | Duplicates |
| `macos` | macOS Specific |
| `app-cache` | App Caches |
| `system-junk` | System Junk |
| `mobile-backup` | Mobile Backups |
| `llm` | LLM Models |

## Architecture

```
src/
  main.rs            CLI entry point (clap)
  commands/
    upgrade.rs       Self-update via GitHub releases
    completions.rs   Shell completion generation and installation
  tui/               Interactive terminal UI (ratatui)
    app.rs           Application state, key handling, search/filter
    views/           Rendering: main tree view, confirm dialog, help bar
    tree.rs          Hierarchical data model for scan results
  scanner/
    mod.rs           Scan orchestration (single, multi-category, streaming)
    entry.rs         ScannedEntry, Category, SafetyLevel types
    walker.rs        Directory size calculation
    bulk_stat.rs     macOS getattrlistbulk FFI
    project_index.rs Shared project directory discovery (LazyLock)
    cli_cache.rs     Parallel CLI command prefetch (rustup, node, docker, etc.)
  rules/
    mod.rs           Rule engine, CleanupRule trait, cache_rule! macro
    cache.rs         Package manager caches (npm, pip, cargo, go, maven, neovim, bundler)
    build.rs         Build artifacts (target/, dist/, .next/, etc.)
    gitignored.rs    Gitignore-aware directory detection
    brew.rs          Homebrew cleanup/autoremove
    docker.rs        Docker system prune
    browser.rs       Browser caches
    ide.rs           IDE/editor caches
    app_cache.rs     Application caches (Slack, Spotify, Discord, etc.)
    toolchain.rs     Rust, Python, Node version managers
    logs.rs          Log files and system logs
    downloads.rs     Old downloads
    large_files.rs   Large file detection
    duplicates.rs    XXH3-based duplicate detection
    macos.rs         macOS-specific caches and data
    system.rs        System temp files and junk
    mobile.rs        Mobile device backups
    trash.rs         Trash contents
    llm.rs           LLM model storage (Ollama, HuggingFace, LM Studio, GPT4All, Jan AI)
    conda.rs         Conda/Mamba environments and caches
    android.rs       Android SDK, Gradle, emulator caches
    pycache.rs       Python __pycache__ directories
    containers.rs    Podman, Lima, Colima
    cloud_cache.rs   Cloud CLI caches (gcloud, AWS, Terraform, Azure)
  filter.rs          Glob and size filtering
  cleaner.rs         Deletion logic with safety filtering and partial deletion tracking
  config.rs          TOML configuration with camelCase alias support
  monitor/           Background disk usage daemon with stale PID detection
  output.rs          CLI table and JSON output
  platform.rs        Disk info (total/used/available)
  util.rs            Helpers (tilde paths, human sizes)
```

**Key design decisions:**
- **LazyLock rule registry** -- all rules initialized once on first access
- **rayon for outer parallelism** -- rules execute concurrently, inner walks are sequential (avoids VFS contention)
- **Cache warming** -- `PROJECT_INDEX` and `CLI_CACHE` are initialized on dedicated OS threads before rayon dispatch to prevent thread pool starvation
- **Streaming scan updates** -- mpsc channel pushes results to TUI as each rule completes
- **Synthetic paths** -- Docker and Homebrew entries use `docker:` / `brew:` path prefixes mapped to prune commands
- **Safety-first** -- CLI cleanup only targets Safe entries by default, `--all` required for Caution/Danger
- **Partial deletion tracking** -- cleaner measures actual freed bytes rather than assuming full entry size

## License

MIT
