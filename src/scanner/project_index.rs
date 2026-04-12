use std::path::{Path, PathBuf};
use std::sync::LazyLock;
use std::time::SystemTime;

use rayon::prelude::*;
use rustc_hash::FxHashSet;
use serde::{Deserialize, Serialize};

/// A directory discovered during the shared walk of project search roots.
#[derive(Debug, Serialize, Deserialize)]
pub struct IndexedDir {
    pub path: PathBuf,
    /// The directory's own name (e.g. "target", "`node_modules`", ".git").
    pub name: String,
}

/// Pre-built index of all directories under the standard search roots, walked once.
///
/// `BuildArtifactRule`, `InstalledDepsRule`, and `GitignoredRule` all filter over
/// this shared index instead of each doing their own depth-6 walk.
pub static PROJECT_INDEX: LazyLock<ProjectIndex> = LazyLock::new(ProjectIndex::build);

const MAX_SCAN_DEPTH: usize = 6;

/// Maximum age of a cached index before it is rebuilt.
const CACHE_TTL_SECS: u64 = 3600; // 1 hour

/// Default project search roots (relative to home directory).
const DEFAULT_SEARCH_ROOTS: &[&str] = &["Workspace", "Projects", "Developer", "Code", "src", "dev"];

/// Heavy directories to never recurse into during indexing.
static SKIP_DIRS: LazyLock<FxHashSet<&'static str>> = LazyLock::new(|| {
    [
        "node_modules",
        "target",
        ".build",
        ".venv",
        "venv",
        ".tox",
        "vendor",
        "build",
        "_build",
        ".dart_tool",
        ".git",
        "__pycache__",
    ]
    .into_iter()
    .collect()
});

pub struct ProjectIndex {
    /// All directories found during the walk.
    dirs: Vec<IndexedDir>,
    /// All project roots (directories that contain `.git`).
    git_roots: Vec<PathBuf>,
}

/// Serializable cache format for the project index.
#[derive(Serialize, Deserialize)]
struct CachedIndex {
    dirs: Vec<IndexedDir>,
    git_roots: Vec<PathBuf>,
    /// Unix timestamp when the cache was written.
    timestamp: u64,
    /// Modification times (as unix secs) of search root directories at cache time.
    /// Used to detect when roots have changed and cache should be invalidated.
    root_mtimes: Vec<(PathBuf, u64)>,
}

impl ProjectIndex {
    fn build() -> Self {
        Self::build_with_roots(DEFAULT_SEARCH_ROOTS)
    }

    fn cache_path() -> PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| PathBuf::from("/tmp"))
            .join("sweeprs")
            .join("project_index.json")
    }

    /// Try to load a cached index. Returns None if cache is stale, missing, or invalid.
    fn load_cache(search_roots: &[PathBuf]) -> Option<Self> {
        let cache_path = Self::cache_path();
        let data = std::fs::read_to_string(&cache_path).ok()?;
        let cached: CachedIndex = serde_json::from_str(&data).ok()?;

        // Check TTL
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        if now.saturating_sub(cached.timestamp) > CACHE_TTL_SECS {
            return None;
        }

        // Check that the search roots haven't changed
        if cached.root_mtimes.len() != search_roots.len() {
            return None;
        }
        for (cached_root, cached_mtime) in &cached.root_mtimes {
            if !search_roots.contains(cached_root) {
                return None;
            }
            let current_mtime = dir_mtime(cached_root);
            if current_mtime != *cached_mtime {
                return None;
            }
        }

        Some(Self {
            dirs: cached.dirs,
            git_roots: cached.git_roots,
        })
    }

    /// Save the current index to the cache file.
    fn save_cache(&self, search_roots: &[PathBuf]) {
        let now = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        let root_mtimes: Vec<(PathBuf, u64)> = search_roots
            .iter()
            .map(|r| (r.clone(), dir_mtime(r)))
            .collect();

        let cached = CachedIndex {
            dirs: Vec::new(), // We'll serialize from self
            git_roots: Vec::new(),
            timestamp: now,
            root_mtimes,
        };

        // Build the full cached struct with actual data
        // (serde needs owned data, so we clone)
        let cached = CachedIndex {
            dirs: self
                .dirs
                .iter()
                .map(|d| IndexedDir {
                    path: d.path.clone(),
                    name: d.name.clone(),
                })
                .collect(),
            git_roots: self.git_roots.clone(),
            timestamp: cached.timestamp,
            root_mtimes: cached.root_mtimes,
        };

        let cache_path = Self::cache_path();
        if let Some(parent) = cache_path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(&cached) {
            let _ = std::fs::write(&cache_path, json);
        }
    }

    /// Build the index with custom search root names (relative to home).
    pub fn build_with_roots(root_names: &[&str]) -> Self {
        let home = dirs::home_dir().unwrap_or_default();
        let search_roots: Vec<PathBuf> = root_names
            .iter()
            .map(|name| {
                // Support both relative names and absolute/tilde-expanded paths
                if name.starts_with('/') {
                    PathBuf::from(name)
                } else if let Some(stripped) = name.strip_prefix("~/") {
                    home.join(stripped)
                } else {
                    home.join(name)
                }
            })
            .filter(|p| p.exists())
            .collect();

        // Try loading from cache first
        if let Some(cached) = Self::load_cache(&search_roots) {
            return cached;
        }

        // Walk each root in parallel via rayon.
        let results: Vec<(Vec<IndexedDir>, Vec<PathBuf>)> = search_roots
            .par_iter()
            .map(|root| {
                let mut dirs = Vec::new();
                let mut git_roots = Vec::new();
                walk_dir(root, 0, &mut dirs, &mut git_roots);
                (dirs, git_roots)
            })
            .collect();

        let mut all_dirs = Vec::new();
        let mut all_git_roots = Vec::new();
        for (dirs, roots) in results {
            all_dirs.extend(dirs);
            all_git_roots.extend(roots);
        }

        let index = Self {
            dirs: all_dirs,
            git_roots: all_git_roots,
        };

        // Save to cache for next time
        index.save_cache(&search_roots);

        index
    }

    /// Find directories matching build/deps marker patterns.
    ///
    /// For each `(dir_name, marker_file, label)` marker, returns paths where:
    /// - The directory's name matches `dir_name`
    /// - The parent directory contains `marker_file`
    pub fn find_matching_dirs(
        &self,
        markers: &[(&str, &str, &'static str)],
    ) -> Vec<(PathBuf, &'static str)> {
        let mut found = Vec::new();
        for entry in &self.dirs {
            for marker_tuple in markers {
                let dir_name: &str = marker_tuple.0;
                let marker_file: &str = marker_tuple.1;
                let label: &'static str = marker_tuple.2;
                if entry.name == dir_name {
                    if let Some(parent) = entry.path.parent() {
                        if parent.join(marker_file).exists() {
                            found.push((entry.path.clone(), label));
                            break;
                        }
                    }
                }
            }
        }
        found
    }

    /// Find all directories with a given name.
    pub fn find_dirs_by_name(&self, name: &str) -> Vec<PathBuf> {
        self.dirs
            .iter()
            .filter(|d| d.name == name)
            .map(|d| d.path.clone())
            .collect()
    }

    /// Return all discovered git project roots.
    pub fn git_roots(&self) -> &[PathBuf] {
        &self.git_roots
    }
}

/// Get the modification time of a directory as unix seconds. Returns 0 on error.
fn dir_mtime(path: &Path) -> u64 {
    path.metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
        .map_or(0, |d| d.as_secs())
}

fn walk_dir(dir: &Path, depth: usize, dirs: &mut Vec<IndexedDir>, git_roots: &mut Vec<PathBuf>) {
    if depth >= MAX_SCAN_DEPTH {
        return;
    }

    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };

    let mut has_git = false;
    let mut subdirs = Vec::new();

    for entry in entries.flatten() {
        let Ok(ft) = entry.file_type() else { continue };
        if !ft.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        if name_str.as_ref() == ".git" {
            has_git = true;
            continue;
        }

        let path = entry.path();
        let name_owned = name_str.to_string();

        dirs.push(IndexedDir {
            path: path.clone(),
            name: name_owned.clone(),
        });

        // Don't recurse into known heavy dirs (O(1) HashSet lookup).
        if !SKIP_DIRS.contains(name_owned.as_str()) {
            subdirs.push(path);
        }
    }

    if has_git {
        git_roots.push(dir.to_path_buf());
    }

    for subdir in subdirs {
        walk_dir(&subdir, depth + 1, dirs, git_roots);
    }
}
