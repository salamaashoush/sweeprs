# Changelog

All notable changes to this project will be documented in this file.
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
## [0.1.0] - 2026-02-06

### Features

- Initial implementation of sweeprs disk cleanup TUI
- Expand cleanup rules to ~70 and refactor TUI to tree view
- Add Homebrew cleanup and autoremove support
- Safe-only cleanup by default and tilde paths in CLI output
- Add glob/path filtering, exclude patterns, and min-size for scan/clean

### Bug Fixes

- Implement Docker cleanup via docker prune commands
- Basename glob matching, parallel deletion, and post-filter scan summary

### Documentation

- Add comprehensive README with usage, config, and architecture docs

### Performance

- Batch file metadata reads with getattrlistbulk on macOS

### Miscellaneous Tasks

- Add justfile, git-cliff config, and cross-platform release scripts
- Update dependencies (crossterm, toml, indicatif, sysinfo)
