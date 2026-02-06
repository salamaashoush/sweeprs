```

   _______ _      __ ___  ___  ____  _____
  / ___/ | | /| / // _ \/ _ \/ __ \/ ___/
 (__  )| |/ |/ // __/  __/ /_/ / /
/____/ |__/|__/ \___/\___/ .___/_/
                         /_/
```

# sweeprs

Fast macOS disk cleanup TUI and CLI written in Rust.

Scans your system for reclaimable disk space across 15+ categories, presents results
in an interactive terminal UI or structured CLI output, and cleans up safely with
dry-run by default.

---

## Features

- **Interactive TUI** -- tree-based browser with real-time scan progress
- **15 scan categories** -- caches, build artifacts, dependencies, Docker, Homebrew, logs, and more
- **Safety levels** -- entries classified as Safe, Caution, or Danger with safe-only cleanup by default
- **Glob filtering** -- `--filter` and `--exclude` patterns to narrow results by path
- **Size filtering** -- `--min-size` to focus on large items
- **Parallel scanning** -- rayon-powered concurrent rule execution
- **macOS optimized** -- `getattrlistbulk` syscall for batched file metadata reads
- **Streaming results** -- TUI updates as each rule completes, no waiting for full scan
- **Docker and Homebrew** -- detects and prunes containers, images, volumes, caches, and unneeded formulae
- **Gitignore-aware** -- finds large directories ignored by git in your projects
- **Duplicate detection** -- XXH3-based file deduplication (opt-in)
- **Background monitor** -- daemon that alerts when disk usage exceeds thresholds
- **JSON output** -- machine-readable scan results for scripting
- **Dry-run by default** -- never deletes anything unless you pass `--force`
- **Configurable** -- TOML config for thresholds, excludes, and category toggles

## Installation

### Quick install (latest release)

```bash
curl -fsSL https://raw.githubusercontent.com/salamaashoush/sweeprs/main/scripts/install.sh | bash
```

Or install a specific version:

```bash
VERSION=0.1.0 curl -fsSL https://raw.githubusercontent.com/salamaashoush/sweeprs/main/scripts/install.sh | bash
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
# Dry-run clean of all safe items (shows what would be deleted)
sweeprs clean all

# Actually delete
sweeprs clean all --force

# Clean a specific category
sweeprs clean build --force

# Include Caution and Danger items
sweeprs clean all --force --all

# Clean only matching entries
sweeprs clean all --force -f '~/Projects/old-app/**'

# Skip confirmation prompt
sweeprs clean all --force --yes
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

# Clean Homebrew outdated cache and unneeded formulae
sweeprs clean cache --force -f 'brew:*' -y

# Export full scan to JSON for scripting
sweeprs scan --json > scan-results.json

# Find the biggest items across all categories
sweeprs scan --min-size 2G
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
| `y` | Confirm deletion |
| `n` / `Esc` | Cancel / go back |
| `q` | Quit |
| `Ctrl+C` | Force quit |

## Categories

| Category | Safety | What it finds |
|---|---|---|
| Package Caches | Safe | npm, yarn, pnpm, bun, cargo, pip caches |
| Build Artifacts | Safe | Rust target/, Xcode, Maven, Gradle, CMake outputs, gitignored dirs |
| Installed Dependencies | Safe | node_modules/, .venv/, vendor/ |
| Browser Caches | Safe | Chrome, Safari, Firefox caches |
| IDE Caches | Safe | VS Code, Cursor, JetBrains, Xcode caches |
| App Caches | Safe | Slack, Spotify, Discord, Teams caches |
| Rust Toolchains | Caution | Old rustup toolchains, Python/Node/Ruby versions |
| Docker | Caution | Images, containers, volumes, build cache |
| Log Files | Caution | System logs, diagnostic reports |
| Old Downloads | Caution | Downloads older than 90 days (configurable) |
| macOS Specific | Caution | Xcode simulators, QuickLook, Mail caches |
| System Junk | Caution | Temp files, software update cache |
| Mobile Backups | Caution | iOS device backups |
| Trash | Danger | ~/.Trash contents |
| Large Files | Danger | Files over 500 MB (configurable) |
| Duplicates | Danger | Identical files by content hash (disabled by default) |

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

## Configuration

Config file location: `~/.config/sweeprs/config.toml`

Generate a default config with `sweeprs config --init`.

```toml
[general]
confirm_before_delete = true
cli_dry_run_default = true
output_format = "table"
global_excludes = []

[scan]
max_depth = 10
threads = 0           # 0 = auto-detect
follow_symlinks = false

[categories]
download_age_days = 90
large_file_threshold = 524288000   # 500 MB
large_file_dirs = ["~/Downloads", "~/Desktop"]
enable_duplicates = false
duplicate_min_size = 1048576       # 1 MB
duplicate_dirs = []

# Toggle individual categories
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
duplicate = false      # opt-in
macos_specific = true
app_cache = true
system_junk = true
mobile_backup = true

[monitor]
poll_interval_secs = 3600      # 1 hour
warning_threshold_percent = 85
critical_threshold_percent = 95
```

## Architecture

```
src/
  main.rs            CLI entry point (clap)
  tui/               Interactive terminal UI (ratatui)
    app.rs           Application state and key handling
    views/           Rendering: main tree view, confirm dialog
    tree.rs          Hierarchical data model for scan results
  scanner/
    mod.rs           Scan orchestration
    entry.rs         ScannedEntry, Category, SafetyLevel types
    walker.rs        Directory size calculation
    bulk_stat.rs     macOS getattrlistbulk FFI
    project_index.rs Shared git root discovery (LazyLock)
    cli_cache.rs     Parallel CLI command prefetch
  rules/
    mod.rs           Rule engine, CleanupRule trait, cache_rule! macro
    cache.rs         Package manager caches
    build.rs         Build artifacts
    gitignored.rs    Gitignore-aware directory detection
    brew.rs          Homebrew cleanup/autoremove
    docker.rs        Docker system prune
    ...              (18 rule modules total)
  filter.rs          Glob and size filtering
  cleaner.rs         Deletion logic with safety filtering
  config.rs          TOML configuration
  monitor.rs         Background disk usage daemon
  output.rs          CLI table and JSON output
  util.rs            Helpers (tilde paths, human sizes)
```

**Key design decisions:**
- **LazyLock rule registry** -- all rules initialized once on first access
- **rayon for outer parallelism** -- rules execute concurrently, inner walks are sequential (avoids VFS contention)
- **Streaming scan updates** -- mpsc channel pushes results to TUI as each rule completes
- **Synthetic paths** -- Docker and Homebrew entries use `docker:` / `brew:` path prefixes mapped to prune commands
- **Safety-first** -- CLI cleanup only targets Safe entries by default, `--all` required for Caution/Danger

## License

MIT
