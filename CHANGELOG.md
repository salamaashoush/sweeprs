# Changelog

All notable changes to this project will be documented in this file.
## [0.9.0] - 2026-11-11

### Features

- Attribute leftovers to Codex, Gemini, Qwen, opencode, Cline, Roo

### Bug Fixes

- Guard every delete and stop rules removing live data
## [0.8.2] - 2026-09-30

### Bug Fixes

- *(tui)* Keep selections, filters and the cursor across scans
## [0.8.1] - 2026-09-30

### Bug Fixes

- *(tui)* Delete on a worker thread and show progress while it runs
## [0.8.0] - 2026-09-29

### Features

- Compact, width-aware scan and clean listings
- Consistent progress and summaries while cleaning
- *(tui)* Aligned tree columns and a details panel worth reading

### Bug Fixes

- Name generic cache entries instead of repeating their path
- Decide the clean prompt's safe and caution answers per entry
## [0.7.0] - 2026-09-29

### Bug Fixes

- Stop a timed-out git gc and everything it spawned
- Never select or absorb an entry a rule refused to clean
- Resolve a clone and its worktrees to one git store
- *(ide)* Stop offering Zed and JetBrains data dirs as caches
- *(agents)* Judge session state by session id and worktrees by git
- Small rule corrections for macOS and project caches
- Stop auto-cleaning Electron sign-in state as a Safe cache
- *(agents)* Follow where each agent actually stores sessions now

### Other

- Speedup and performance imporvements
- Move to Rust stable 1.98 and update dependencies

### Styling

- Collapse nested ifs and use whole-unit Durations

### Miscellaneous Tasks

- Stop tracking personal Claude settings
## [0.6.0] - 2026-07-12

### Features

- Add simulator and AI tools scan categories
## [0.5.0] - 2026-04-12

### Features

- Interactive category selection during clean confirmation
- Add first-tier Linux support with deep scanning and platform abstraction
- Add Linux CI, release builds, and update docs

### Bug Fixes

- Improve scan progress display, remove downloads from defaults
- Remove GitRepoSizeRule to prevent accidental .git deletion
- Restore detailed entry listing, fix progress counter, add safety breakdown
- Show all large .git repos in scan, not just those with loose objects
- Remove generic Application Support and Group Containers scanners
- Improve clean UX with per-item feedback and permission handling
- Release workflow - correct runners, cross-compilation, changelog

### Styling

- Fix formatting for CI

### Miscellaneous Tasks

- Add git hooks for format, check, clippy, and test
- Drop Intel Mac target, use macos-latest for Apple Silicon
- Bump version to 0.5.0
## [0.4.0] - 2026-04-12

### Features

- Add diff command and scan history tracking
- Consolidate auto-clean into monitor, add stale project category
- Add SafetyLevel::Error for failed scan commands
- *(toolchain)* Improve Rust toolchain detection and add mise support
- Add 28 new macOS cleanup rules, fix sparse file sizing, fix Docker reporting
- Git optimization, archive mode, project index caching, more cleanup targets
- Optimize defaults for dev machines, add archive_dir config, improve example
- Add exclude_clean_categories for easy category removal

### Bug Fixes

- Eliminate monitor CPU/memory/thread leaks
- Detect Colima _disks/ dir and handle deletion errors
- Handle terminal resize, remove check-state panic, fix stale cursor after expand
- Resolve all clippy warnings and formatting issues

### Performance

- Smart cache warming, fast disk info, colima detection rewrite
- Reduce syscalls, cache TUI state, and skip redundant redraws

### Miscellaneous Tasks

- Remove diff command and scan history tracking
- Release v0.4.0
## [0.3.0] - 2026-02-08

### Features

- Add new scan rules for LLM models, conda, android, pycache, containers, and cloud CLIs
- Multi-category clean, default clean categories, and categories command
- TUI search/filter, copy path, and updated help bar
- Monitor launchd service install and notification cleanup action

### Bug Fixes

- Accurate byte counting for partial deletions

### Documentation

- Comprehensive README update for v0.3.0 features

### Miscellaneous Tasks

- Release v0.3.0
## [0.2.0] - 2026-02-06

### Features

- Add install script to download release from GitHub
- Improve scanner accuracy with hardlink dedup, physical sizes, and cross-volume boundaries
- Add CLI command timeouts and diskutil APFS prefetch
- Report purgeable space, APFS snapshot sizes, and Time Machine snapshot sizes
- Segmented disk bar, inline size bars, and reveal-in-Finder
- Add upgrade and completions subcommands

### Bug Fixes

- Remove full scan from monitor daemon, add notification cooldown and clean shutdown

### Performance

- Warm caches before rayon scan and show current rule in progress

### Styling

- Apply cargo fmt across entire codebase

### Miscellaneous Tasks

- Release v0.2.0
## [0.1.0] - 2026-02-06

### Features

- Initial release of sweeprs - fast macOS disk cleanup TUI
