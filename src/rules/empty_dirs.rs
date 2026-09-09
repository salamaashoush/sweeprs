use std::io;
use std::path::{Path, PathBuf};

use rayon::prelude::*;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::project_index::PROJECT_INDEX;

const MAX_DEPTH: usize = 4;

/// Directories whose contents belong to a package manager or VCS, not the project.
const SKIP: &[&str] = &[
    "node_modules",
    "target",
    ".git",
    ".build",
    "vendor",
    ".venv",
];

pub struct EmptyDirsRule;

impl CleanupRule for EmptyDirsRule {
    fn name(&self) -> &'static str {
        "Empty directories"
    }

    fn category(&self) -> Category {
        Category::SystemJunk
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        PROJECT_INDEX
            .git_roots()
            .par_iter()
            .filter_map(|root| {
                let count = collect_empty_dirs(root, 0).len();
                if count == 0 {
                    return None;
                }
                let project = root
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
                Some(ScannedEntry {
                    // Removing these frees no measurable space, so the entry
                    // stands for the command rather than for bytes on disk.
                    path: PathBuf::from(format!("empty-dirs:{}", root.display())),
                    size: 0,
                    category: Category::SystemJunk,
                    // An empty directory is sometimes a placeholder a tool needs
                    // to exist -- Rails `tmp/`, a mount point, an output dir a
                    // build script writes into without creating.
                    safety: SafetyLevel::Caution,
                    description: format!("{count} empty directories in {project}"),
                    item_count: Some(count),
                })
            })
            .collect()
    }
}

/// Directories under `dir` that hold no files at any depth.
///
/// A directory whose only contents are themselves empty directories counts too:
/// removing the leaf empties it in turn, and reporting only the leaf would
/// undercount what a clean actually removes.
fn collect_empty_dirs(dir: &Path, depth: usize) -> Vec<PathBuf> {
    collect_empty(dir, depth).1
}

/// Returns whether `dir` holds nothing but empty directories, along with every
/// removable directory beneath it.
fn collect_empty(dir: &Path, depth: usize) -> (bool, Vec<PathBuf>) {
    if depth > MAX_DEPTH {
        return (false, Vec::new());
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (false, Vec::new());
    };

    let mut found = Vec::new();
    let mut all_children_removable = true;

    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|ft| ft.is_dir()) {
            all_children_removable = false;
            continue;
        }

        let name = entry.file_name();
        if SKIP.contains(&name.to_string_lossy().as_ref()) {
            all_children_removable = false;
            continue;
        }

        let path = entry.path();
        let (removable, nested) = collect_empty(&path, depth + 1);
        found.extend(nested);
        if removable {
            found.push(path);
        } else {
            all_children_removable = false;
        }
    }

    (all_children_removable, found)
}

/// Remove the empty directories under the root named by an `empty-dirs:` entry.
pub fn clean_empty_dirs(entry_path: &str) -> io::Result<Option<u64>> {
    let root = entry_path.strip_prefix("empty-dirs:").unwrap_or(entry_path);
    let root = Path::new(root);

    let mut dirs = collect_empty_dirs(root, 0);
    // Deepest first, so a parent left empty by its last child goes too.
    dirs.sort_by_key(|p| std::cmp::Reverse(p.components().count()));

    let mut removed = 0usize;
    let mut first_error = None;
    for dir in &dirs {
        // `remove_dir` refuses a non-empty directory, which is the guard we want
        // if something wrote into it between the scan and now.
        match std::fs::remove_dir(dir) {
            Ok(()) => removed += 1,
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
    }

    match first_error {
        Some(e) if removed == 0 && !dirs.is_empty() => Err(e),
        _ => Ok(Some(0)),
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(EmptyDirsRule)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_directories_with_nothing_in_them_are_collected() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::create_dir(root.join("empty")).expect("empty");
        std::fs::create_dir(root.join("full")).expect("full");
        std::fs::write(root.join("full/file.txt"), b"x").expect("file");
        std::fs::create_dir_all(root.join("node_modules/inner")).expect("skipped");

        let found = collect_empty_dirs(root, 0);
        assert_eq!(found, vec![root.join("empty")]);
    }

    #[test]
    fn cleaning_removes_nested_empties_without_touching_used_dirs() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();
        std::fs::create_dir_all(root.join("a/b/c")).expect("nested");
        std::fs::create_dir(root.join("keep")).expect("keep");
        std::fs::write(root.join("keep/file.txt"), b"x").expect("file");

        clean_empty_dirs(&format!("empty-dirs:{}", root.display())).expect("clean");

        assert!(!root.join("a/b/c").exists());
        assert!(!root.join("a").exists());
        assert!(root.join("keep/file.txt").exists());
    }
}
