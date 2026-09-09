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
    /// Modification time when it was indexed, used to validate the cache.
    #[serde(default)]
    pub mtime: u64,
}

/// Pre-built index of all directories under the standard search roots, walked once.
///
/// `BuildArtifactRule`, `InstalledDepsRule`, and `GitignoredRule` all filter over
/// this shared index instead of each doing their own depth-6 walk.
pub static PROJECT_INDEX: LazyLock<ProjectIndex> = LazyLock::new(ProjectIndex::build);

const MAX_SCAN_DEPTH: usize = 6;

/// Backstop age for a cached index. Structural changes are caught by the mtime
/// check below well before this expires; this only bounds how long a cache can
/// survive something that check cannot see.
const CACHE_TTL_SECS: u64 = 3600; // 1 hour

/// Bumped whenever the walk changes what it records, so a cache written by an
/// older build is discarded instead of silently narrowing a scan.
const CACHE_SCHEMA_VERSION: u32 = 4;

/// Default project search roots (relative to home directory).
const DEFAULT_SEARCH_ROOTS: &[&str] = &["Workspace", "Projects", "Developer", "Code", "src", "dev"];

/// Directories that are indexed but never descended into.
///
/// Each is either disposable in its entirety or dense enough that walking it
/// dominates index time. The directory itself is still recorded, so rules that
/// match on its name keep working -- only the walk stops at its boundary.
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
        ".next",
        ".nuxt",
        ".output",
        ".svelte-kit",
        ".astro",
        ".parcel-cache",
        ".turbo",
        ".angular",
        ".docusaurus",
        ".expo",
        ".serverless",
        ".rollup.cache",
        ".gradle",
        ".cxx",
        ".kotlin",
        ".terraform",
        "Pods",
        "DerivedData",
        ".pytest_cache",
        ".mypy_cache",
        ".ruff_cache",
        ".nyc_output",
        "htmlcov",
        "coverage",
        "dist",
        ".svn",
    ]
    .into_iter()
    .collect()
});

pub struct ProjectIndex {
    /// All directories found during the walk.
    dirs: Vec<IndexedDir>,
    /// Every working tree, whether a plain clone or a linked worktree.
    git_roots: Vec<PathBuf>,
    /// One entry per distinct object store, deduplicated.
    object_stores: Vec<PathBuf>,
}

/// Serializable cache format for the project index.
#[derive(Serialize, Deserialize)]
struct CachedIndex {
    #[serde(default)]
    version: u32,
    dirs: Vec<IndexedDir>,
    git_roots: Vec<PathBuf>,
    #[serde(default)]
    object_stores: Vec<PathBuf>,
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

        if cached.version != CACHE_SCHEMA_VERSION {
            return None;
        }

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

        // The search roots alone are not enough. Creating `<project>/target`
        // changes `<project>`'s mtime and nothing above it, so a cache keyed
        // only on the roots stays "valid" while missing the largest directory
        // on the disk -- a scan that silently under-reports, which is worse than
        // a slow one. Re-stat every directory the walk descended into; a
        // directory it stopped at cannot have gained an indexed child.
        let unchanged = cached
            .dirs
            .par_iter()
            .filter(|dir| !SKIP_DIRS.contains(dir.name.as_str()))
            .all(|dir| dir_mtime(&dir.path) == dir.mtime);
        if !unchanged {
            return None;
        }

        Some(Self {
            dirs: cached.dirs,
            git_roots: cached.git_roots,
            object_stores: cached.object_stores,
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
            version: CACHE_SCHEMA_VERSION,
            dirs: self
                .dirs
                .iter()
                .map(|d| IndexedDir {
                    path: d.path.clone(),
                    name: d.name.clone(),
                    mtime: d.mtime,
                })
                .collect(),
            git_roots: self.git_roots.clone(),
            object_stores: self.object_stores.clone(),
            timestamp: now,
            root_mtimes,
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

        let mut object_stores: Vec<PathBuf> = Vec::new();
        let mut seen = FxHashSet::default();
        for root in &all_git_roots {
            if let Some(store) = resolve_git_dir(root) {
                if seen.insert(store.clone()) {
                    object_stores.push(store);
                }
            }
        }

        let index = Self {
            dirs: all_dirs,
            git_roots: all_git_roots,
            object_stores,
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

    /// Every working tree found, plain clones and linked worktrees alike.
    ///
    /// Rules about the contents of a project -- gitignored output, staleness,
    /// empty directories -- want this: each worktree has its own files.
    pub fn git_roots(&self) -> &[PathBuf] {
        &self.git_roots
    }

    /// One path per distinct object store.
    ///
    /// Rules about a repository's history -- gc, LFS, rerere -- want this
    /// instead. A stack of worktrees shares a single object store, so keying
    /// off working trees would run the same repack once per worktree and count
    /// the same reclaimable bytes that many times over.
    pub fn object_stores(&self) -> &[PathBuf] {
        &self.object_stores
    }
}

/// The git directory a working tree uses, and the store it ultimately shares.
///
/// For a plain clone both are `<root>/.git`. For a linked worktree the `.git`
/// file points at `<common>/worktrees/<name>`, whose parent's parent is the
/// common store every sibling worktree writes objects to.
pub fn resolve_git_dir(root: &Path) -> Option<PathBuf> {
    let dot_git = root.join(".git");
    let meta = dot_git.symlink_metadata().ok()?;

    if meta.is_dir() {
        return Some(dot_git);
    }

    let contents = std::fs::read_to_string(&dot_git).ok()?;
    let target = contents.trim().strip_prefix("gitdir:")?.trim();
    let target = if Path::new(target).is_absolute() {
        PathBuf::from(target)
    } else {
        root.join(target)
    };

    // `<common>/worktrees/<name>` -> `<common>`
    let common = target
        .parent()
        .filter(|p| p.file_name().is_some_and(|n| n == "worktrees"))
        .and_then(Path::parent)
        .map(Path::to_path_buf);

    let store = common.unwrap_or(target);
    // A relative `gitdir:` leaves `.` components in the path. Two worktrees
    // spelling the same store differently would then look like two stores, and
    // the deduplication that keeps one gc per repository would not hold.
    Some(store.canonicalize().unwrap_or(store))
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

        let name = entry.file_name();
        let name_str = name.to_string_lossy();

        // In a linked worktree `.git` is a file holding `gitdir: <path>`, not a
        // directory. Testing for a directory first skipped every worktree on
        // the machine, and with it every rule that works from a project root.
        if name_str.as_ref() == ".git" {
            has_git = true;
            continue;
        }

        if !ft.is_dir() {
            continue;
        }

        let path = entry.path();
        let name_owned = name_str.to_string();

        dirs.push(IndexedDir {
            path: path.clone(),
            name: name_owned.clone(),
            mtime: entry.metadata().ok().map_or(0, |m| {
                m.modified()
                    .ok()
                    .and_then(|t| t.duration_since(SystemTime::UNIX_EPOCH).ok())
                    .map_or(0, |d| d.as_secs())
            }),
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
