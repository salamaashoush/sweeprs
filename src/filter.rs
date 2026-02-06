use std::path::Path;

use anyhow::{Context, Result};
use globset::{Glob, GlobSet, GlobSetBuilder};

use crate::scanner::entry::ScannedEntry;

/// Post-scan filter that narrows results by glob pattern and/or minimum size.
///
/// Patterns without `/` are matched against just the file/directory name (basename).
/// Patterns with `/` (including after `~` expansion) are matched against the full path.
pub struct EntryFilter {
    /// Basename-only patterns (no `/` in original input).
    include_basename: Option<GlobSet>,
    /// Full-path patterns (contain `/`).
    include_path: Option<GlobSet>,
    exclude_basename: Option<GlobSet>,
    exclude_path: Option<GlobSet>,
    min_size: u64,
}

impl EntryFilter {
    pub fn new(
        include_patterns: &[String],
        exclude_patterns: &[String],
        min_size: u64,
    ) -> Result<Self> {
        let home_str = dirs::home_dir()
            .map(|h| h.display().to_string())
            .unwrap_or_default();

        let (inc_base, inc_path) =
            split_patterns(include_patterns, &home_str).context("invalid --filter pattern")?;
        let (exc_base, exc_path) =
            split_patterns(exclude_patterns, &home_str).context("invalid --exclude pattern")?;

        Ok(Self {
            include_basename: inc_base,
            include_path: inc_path,
            exclude_basename: exc_base,
            exclude_path: exc_path,
            min_size,
        })
    }

    /// Returns true if this filter would actually change results.
    pub fn is_active(&self) -> bool {
        self.include_basename.is_some()
            || self.include_path.is_some()
            || self.exclude_basename.is_some()
            || self.exclude_path.is_some()
            || self.min_size > 0
    }

    /// Test whether a single entry passes the filter.
    pub fn matches(&self, entry: &ScannedEntry) -> bool {
        if entry.size < self.min_size {
            return false;
        }

        // Include: entry must match at least one include pattern (if any are set).
        // Basename and path patterns are ORed together.
        if self.include_basename.is_some() || self.include_path.is_some() {
            let matches_basename = self
                .include_basename
                .as_ref()
                .is_some_and(|gs| match_basename(gs, &entry.path));
            let matches_path = self
                .include_path
                .as_ref()
                .is_some_and(|gs| gs.is_match(&entry.path));
            if !matches_basename && !matches_path {
                return false;
            }
        }

        // Exclude: entry must NOT match any exclude pattern.
        if let Some(ref gs) = self.exclude_basename {
            if match_basename(gs, &entry.path) {
                return false;
            }
        }
        if let Some(ref gs) = self.exclude_path {
            if gs.is_match(&entry.path) {
                return false;
            }
        }

        true
    }

    /// Filter a slice of entries, returning references to those that pass.
    pub fn apply<'a>(&self, entries: &'a [ScannedEntry]) -> Vec<&'a ScannedEntry> {
        entries.iter().filter(|e| self.matches(e)).collect()
    }
}

/// Match a glob set against just the file name component of a path.
fn match_basename(gs: &GlobSet, path: &Path) -> bool {
    path.file_name()
        .is_some_and(|name| gs.is_match(Path::new(name)))
}

/// Split patterns into basename and path glob sets based on whether they contain `/`.
fn split_patterns(
    patterns: &[String],
    home: &str,
) -> Result<(Option<GlobSet>, Option<GlobSet>)> {
    let mut base_builder = GlobSetBuilder::new();
    let mut path_builder = GlobSetBuilder::new();
    let mut has_base = false;
    let mut has_path = false;

    for raw in patterns {
        let expanded = expand_tilde(raw, home);

        if expanded.contains('/') {
            let glob = Glob::new(&expanded)
                .with_context(|| format!("bad glob pattern: {expanded}"))?;
            path_builder.add(glob);
            has_path = true;
        } else {
            let glob = Glob::new(&expanded)
                .with_context(|| format!("bad glob pattern: {expanded}"))?;
            base_builder.add(glob);
            has_base = true;
        }
    }

    let base_set = if has_base {
        Some(base_builder.build()?)
    } else {
        None
    };
    let path_set = if has_path {
        Some(path_builder.build()?)
    } else {
        None
    };

    Ok((base_set, path_set))
}

/// Expand leading `~` to the home directory.
fn expand_tilde(raw: &str, home: &str) -> String {
    if let Some(rest) = raw.strip_prefix("~/") {
        format!("{home}/{rest}")
    } else if raw == "~" {
        home.to_string()
    } else {
        raw.to_string()
    }
}

/// Parse a human-readable size string like "1G", "500M", "100K" into bytes.
pub fn parse_size(s: &str) -> Result<u64> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(0);
    }

    let (num_str, multiplier) = if let Some(n) = s.strip_suffix("GiB") {
        (n, 1u64 << 30)
    } else if let Some(n) = s.strip_suffix("MiB") {
        (n, 1u64 << 20)
    } else if let Some(n) = s.strip_suffix("KiB") {
        (n, 1u64 << 10)
    } else if let Some(n) = s.strip_suffix('G') {
        (n, 1u64 << 30)
    } else if let Some(n) = s.strip_suffix('M') {
        (n, 1u64 << 20)
    } else if let Some(n) = s.strip_suffix('K') {
        (n, 1u64 << 10)
    } else if let Some(n) = s.strip_suffix('B') {
        (n, 1)
    } else {
        // Bare number = bytes
        (s, 1)
    };

    let num: f64 = num_str
        .trim()
        .parse()
        .with_context(|| format!("invalid size: {s}"))?;

    Ok((num * multiplier as f64) as u64)
}
