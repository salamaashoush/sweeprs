#!/usr/bin/env -S just --justfile
# sweeprs - Justfile

set shell := ["bash", "-cu"]

# ==================== ALIASES ====================
alias r := ready
alias f := fix
alias c := check

# ==================== DEFAULT ====================
_default:
    @just --list -u

# ==================== SETUP & INITIALIZATION ====================

# Initialize the project by installing all necessary tools
init: install-binstall
    cargo binstall --no-confirm watchexec-cli cargo-nextest typos-cli cargo-shear dprint cargo-deny -y

# Install cargo-binstall (pre-compiled binary installer for faster installs)
install-binstall:
    @if command -v cargo-binstall >/dev/null 2>&1; then \
        echo "cargo-binstall already installed"; \
    else \
        cargo install cargo-binstall; \
    fi

# Install git pre-commit hook to run full checks
install-hook:
    echo '#!/bin/sh' > .git/hooks/pre-commit
    echo 'just ready' >> .git/hooks/pre-commit
    chmod +x .git/hooks/pre-commit

# Install all Rust development tools (basic + advanced)
install-tools: install-binstall
    @echo "Installing all Rust tooling..."
    @echo ""
    @echo "Basic Development Tools..."
    cargo binstall --no-confirm watchexec-cli         # Auto-rebuild on file changes
    cargo binstall --no-confirm typos-cli             # Spell checker
    cargo binstall --no-confirm dprint                # Fast formatter for non-Rust files
    cargo binstall --no-confirm git-cliff             # Changelog generator
    @echo ""
    @echo "Security & Safety Tools..."
    cargo binstall --no-confirm cargo-audit           # CVE scanning
    cargo binstall --no-confirm cargo-deny            # Dependency policies
    cargo binstall --no-confirm cargo-geiger          # Unsafe code detector
    @echo ""
    @echo "Performance & Profiling Tools..."
    cargo binstall --no-confirm flamegraph            # CPU profiling with flame graphs
    cargo binstall --no-confirm cargo-bloat           # Binary size analysis
    @echo ""
    @echo "Testing & Coverage Tools..."
    cargo binstall --no-confirm cargo-llvm-cov        # Fast LLVM coverage
    cargo binstall --no-confirm cargo-nextest         # Faster test runner
    @echo ""
    @echo "Dependency Management Tools..."
    cargo binstall --no-confirm cargo-outdated        # Check outdated deps
    cargo binstall --no-confirm cargo-shear           # Find unused deps (fast)
    @echo ""
    @echo "Cross-compilation Tools..."
    cargo install cross --git https://github.com/cross-rs/cross
    cargo install cargo-xwin
    rustup target add x86_64-pc-windows-msvc
    @echo ""
    @echo "All tools installed successfully!"

# Setup development environment (installs all tools + hook)
setup: install-tools install-hook
    @echo ""
    @echo "Development environment setup complete!"
    @echo ""
    @echo "Quick Start:"
    @echo "  just ready    # Run full CI checks locally"
    @echo "  just check    # Quick compile check"
    @echo "  just fix      # Auto-fix all issues"
    @echo ""
    @echo "See 'just --list' for all available commands"

# ==================== CORE DEVELOPMENT ====================

# Run full CI checks locally (same as CI pipeline)
ready:
    git diff --exit-code --quiet || echo "Warning: uncommitted changes detected"
    typos
    just fmt-check
    just check
    just lint
    just doc
    cargo shear
    just test
    git status

# Run cargo check (fast compile verification)
check:
    cargo check

# Run all the tests
test:
    cargo nextest run --no-tests=pass

# Run tests with standard cargo test
test-cargo:
    cargo test

# Run doc tests only
test-doc:
    cargo test --doc

# Lint the whole project
lint:
    cargo clippy -- --deny warnings

# Lint with release-like settings (catches release-only issues)
lint-release:
    cargo clippy --all-features --profile=dev -- -D warnings

# Format all files
fmt:
    -cargo shear --fix
    cargo fmt
    dprint fmt

# Check formatting without modifying files
fmt-check:
    cargo fmt -- --check
    dprint check

# Generate documentation
doc:
    RUSTDOCFLAGS='-D warnings' cargo doc --no-deps --document-private-items

# Open documentation in browser
doc-open:
    cargo doc --no-deps --open

# Fix all auto-fixable format and lint issues
fix:
    cargo clippy --fix --allow-staged --allow-dirty --no-deps
    just fmt
    typos -w
    git status

# ==================== BUILDING ====================

# Build in debug mode (fast)
build:
    cargo build

# Build in release mode
build-release:
    cargo build --release

# Build with native CPU optimizations
build-native:
    RUSTFLAGS="-C target-cpu=native" cargo build --release

# Create release build with debugging information for profiling
build-release-debug:
    cargo build --profile=release-with-debug

# Install to ~/.cargo/bin
install:
    cargo install --path .

# ==================== RUNNING ====================

# Run sweeprs
run *args:
    cargo run -- {{args}}

# Run with release build
run-release *args:
    cargo run --release -- {{args}}

# ==================== DEVELOPMENT TOOLS ====================

# Watch for changes and run command (uses watchexec)
watch *args='':
    watchexec --no-vcs-ignore {{args}}

# Watch and rebuild
watch-build:
    watchexec -e rs,toml 'cargo build'

# Watch and run tests
watch-test:
    watchexec -e rs,toml 'cargo nextest run'

# Watch and run clippy
watch-lint:
    watchexec -e rs,toml 'cargo clippy'

# ==================== CODE QUALITY ====================

# Check for typos in source code
typos:
    typos

# Fix typos automatically
typos-fix:
    typos -w

# Find unused dependencies
shear:
    cargo shear

# Fix unused dependencies (removes them from Cargo.toml)
shear-fix:
    cargo shear --fix || true

# ==================== SECURITY ====================

# Run security audit
audit:
    cargo audit

# Check dependencies with cargo-deny
deny:
    cargo deny check

# Detect unsafe code usage
geiger:
    cargo geiger --all-features

# Comprehensive security check
security-check: audit deny geiger
    @echo "Security checks complete!"

# ==================== PERFORMANCE ====================

# Analyze binary size
bloat:
    cargo bloat --release --crates

# Profile with flamegraph
flamegraph:
    cargo flamegraph --bin sweeprs

# Check binary sizes
size:
    @echo "Debug build:"
    @ls -lh target/debug/sweeprs 2>/dev/null || echo "Debug binary not found"
    @echo ""
    @echo "Release build:"
    @ls -lh target/release/sweeprs 2>/dev/null || echo "Release binary not found"

# ==================== COVERAGE ====================

# Fast code coverage with llvm-cov
coverage:
    cargo llvm-cov --html --open

# ==================== DEPENDENCIES ====================

# Update dependencies
update:
    cargo update

# Check for outdated dependencies
outdated:
    cargo outdated

# ==================== CHANGELOG ====================

# Generate changelog
changelog:
    git cliff --output CHANGELOG.md

# Generate changelog for unreleased changes
changelog-unreleased:
    git cliff --unreleased --tag unreleased --output CHANGELOG.md

# ==================== CROSS-COMPILATION ====================

# Build for all supported platforms (macOS only - uses objc2/macOS-specific APIs)
cross-build-all:
    @echo "Building for all macOS platforms..."
    cargo build --release --target x86_64-apple-darwin
    cargo build --release --target aarch64-apple-darwin

cross-size:
    @find target -name "sweeprs*" -path "*/release/*" -type f 2>/dev/null | xargs ls -lh 2>/dev/null || echo "No binaries found"

cross-clean:
    rm -rf target/x86_64-apple-darwin target/aarch64-apple-darwin

# ==================== VERSION & RELEASE ====================

# Bump version (major, minor, patch)
bump-version TYPE:
    scripts/bump-version.sh {{TYPE}}

# Generate changelog for a version
gh-changelog VERSION:
    scripts/gh-changelog.sh {{VERSION}}

# Package binaries for GitHub release
gh-package VERSION:
    scripts/gh-package.sh {{VERSION}}

# Create GitHub release
gh-release VERSION:
    scripts/gh-release.sh {{VERSION}}

# Build all binaries and package for GitHub release
gh-build-release VERSION:
    @echo "Building all platform binaries for release..."
    @just cross-build-all
    @echo ""
    @just gh-package {{VERSION}}

# Complete release workflow: bump, changelog, tag, build, and GitHub release
release TYPE:
    scripts/release.sh {{TYPE}}

# ==================== CI ====================

# CI pipeline (most comprehensive)
ci: ready audit deny
    @echo "CI checks complete!"

# Clean build artifacts
clean:
    cargo clean
