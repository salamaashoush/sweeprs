//! Browse any directory by size, for space no rule accounts for.
//!
//! Rules know what is disposable; they cannot say what the rest of a 100 GB
//! directory is. The explorer lists a directory's children largest first, the
//! way `ncdu` does, measured on a background thread so the listing appears at
//! once and fills in.

use std::path::{Path, PathBuf};
use std::sync::mpsc;

use rayon::prelude::*;

use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

pub struct Child {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
    /// `None` until measured.
    pub size: Option<u64>,
}

pub struct Explorer {
    pub dir: PathBuf,
    pub children: Vec<Child>,
    pub cursor: usize,
    pub scroll_offset: usize,
    sizes: Option<mpsc::Receiver<(PathBuf, u64)>>,
}

impl Explorer {
    pub fn open(dir: PathBuf) -> Self {
        let mut explorer = Self {
            dir,
            children: Vec::new(),
            cursor: 0,
            scroll_offset: 0,
            sizes: None,
        };
        explorer.list();
        explorer
    }

    fn list(&mut self) {
        self.children = std::fs::read_dir(&self.dir)
            .map(|read_dir| {
                read_dir
                    .flatten()
                    .map(|entry| {
                        let path = entry.path();
                        let is_dir = entry.file_type().is_ok_and(|t| t.is_dir());
                        Child {
                            name: entry.file_name().to_string_lossy().into_owned(),
                            path,
                            is_dir,
                            size: None,
                        }
                    })
                    .collect()
            })
            .unwrap_or_default();
        self.children.sort_by(|a, b| a.name.cmp(&b.name));
        self.cursor = 0;
        self.scroll_offset = 0;

        let (tx, rx) = mpsc::channel();
        let targets: Vec<(PathBuf, bool)> = self
            .children
            .iter()
            .map(|c| (c.path.clone(), c.is_dir))
            .collect();
        std::thread::spawn(move || {
            targets
                .into_par_iter()
                .for_each_with(tx, |tx, (path, is_dir)| {
                    let size = if is_dir {
                        walker::dir_size_uncached(&path)
                    } else {
                        walker::file_size(&path)
                    };
                    let _ = tx.send((path, size));
                });
        });
        self.sizes = Some(rx);
    }

    /// Take in sizes measured since the last call. Returns true if any arrived.
    ///
    /// The list is kept largest first; the cursor stays on the same child.
    pub fn poll(&mut self) -> bool {
        let Some(rx) = &self.sizes else {
            return false;
        };
        let mut changed = false;
        loop {
            match rx.try_recv() {
                Ok((path, size)) => {
                    if let Some(child) = self.children.iter_mut().find(|c| c.path == path) {
                        child.size = Some(size);
                        changed = true;
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.sizes = None;
                    break;
                }
            }
        }
        if changed {
            let current = self.current().map(|c| c.path.clone());
            self.children.sort_by(|a, b| {
                b.size
                    .unwrap_or(0)
                    .cmp(&a.size.unwrap_or(0))
                    .then_with(|| a.name.cmp(&b.name))
            });
            if let Some(current) = current {
                self.cursor = self
                    .children
                    .iter()
                    .position(|c| c.path == current)
                    .unwrap_or(0);
            }
        }
        changed
    }

    pub fn measuring(&self) -> bool {
        self.sizes.is_some()
    }

    pub fn total(&self) -> u64 {
        self.children.iter().filter_map(|c| c.size).sum()
    }

    pub fn current(&self) -> Option<&Child> {
        self.children.get(self.cursor)
    }

    pub fn down(&mut self) {
        if self.cursor + 1 < self.children.len() {
            self.cursor += 1;
        }
    }

    pub fn up(&mut self) {
        self.cursor = self.cursor.saturating_sub(1);
    }

    /// Into the directory under the cursor.
    pub fn enter(&mut self) {
        if let Some(child) = self.current()
            && child.is_dir
        {
            self.dir = child.path.clone();
            self.list();
        }
    }

    /// Up to the parent, with the cursor on the directory just left.
    pub fn leave(&mut self) {
        let Some(parent) = self.dir.parent().map(Path::to_path_buf) else {
            return;
        };
        let left = std::mem::replace(&mut self.dir, parent);
        self.list();
        if let Some(pos) = self.children.iter().position(|c| c.path == left) {
            self.cursor = pos;
        }
    }

    pub fn refresh(&mut self) {
        let current = self.current().map(|c| c.path.clone());
        self.list();
        if let Some(current) = current
            && let Some(pos) = self.children.iter().position(|c| c.path == current)
        {
            self.cursor = pos;
        }
    }

    /// The child under the cursor as something the deletion pipeline takes.
    ///
    /// Nothing vouches for an arbitrary directory, so it is `Danger`; the
    /// guard, the in-use check and the clean log apply to it like any entry.
    pub fn deletion_entry(&self) -> Option<ScannedEntry> {
        let child = self.current()?;
        Some(ScannedEntry {
            path: child.path.clone(),
            size: child.size.unwrap_or(0),
            category: Category::LargeFile,
            safety: SafetyLevel::Danger,
            description: format!("Explored: {}", child.name),
            item_count: None,
        })
    }

    pub fn clamp_scroll(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            return;
        }
        if self.cursor < self.scroll_offset {
            self.scroll_offset = self.cursor;
        } else if self.cursor >= self.scroll_offset + viewport_height {
            self.scroll_offset = self.cursor + 1 - viewport_height;
        }
    }
}

/// What the scan said about `path`: a rule that offers it, or one that offers
/// something inside it.
pub fn covered_by<'a>(entries: &'a [ScannedEntry], path: &Path) -> Option<&'a ScannedEntry> {
    entries
        .iter()
        .find(|e| e.path == path)
        .or_else(|| entries.iter().find(|e| path.starts_with(&e.path)))
        .or_else(|| {
            entries
                .iter()
                .filter(|e| e.path.starts_with(path))
                .max_by_key(|e| e.size)
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn settle(explorer: &mut Explorer) {
        while explorer.measuring() {
            explorer.poll();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        explorer.poll();
    }

    #[test]
    fn children_are_listed_largest_first_and_navigation_keeps_its_place() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().to_path_buf();
        std::fs::create_dir(root.join("big")).expect("big");
        std::fs::write(root.join("big/blob"), vec![1u8; 256 * 1024]).expect("blob");
        std::fs::write(root.join("small.txt"), b"x").expect("small");

        let mut explorer = Explorer::open(root.clone());
        settle(&mut explorer);
        let names: Vec<_> = explorer.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(names, ["big", "small.txt"]);
        assert!(explorer.total() >= 256 * 1024);

        explorer.enter();
        assert_eq!(explorer.dir, root.join("big"));
        settle(&mut explorer);
        explorer.leave();
        assert_eq!(explorer.dir, root);
        assert_eq!(explorer.current().map(|c| c.name.as_str()), Some("big"));
    }

    #[test]
    fn coverage_is_found_on_the_path_its_parent_or_inside_it() {
        let entry = |path: &str, size: u64| ScannedEntry {
            path: PathBuf::from(path),
            size,
            category: Category::AppCache,
            safety: SafetyLevel::Safe,
            description: path.to_owned(),
            item_count: None,
        };
        let entries = [
            entry("/home/u/.cache/zig", 10),
            entry("/home/u/proj/target", 5),
        ];
        assert_eq!(
            covered_by(&entries, Path::new("/home/u/.cache/zig/o")).map(|e| e.size),
            Some(10)
        );
        assert_eq!(
            covered_by(&entries, Path::new("/home/u/proj")).map(|e| e.size),
            Some(5)
        );
        assert!(covered_by(&entries, Path::new("/home/u/docs")).is_none());
    }
}
