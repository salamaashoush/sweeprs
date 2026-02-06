# Changelog

All notable changes to this project will be documented in this file.
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
