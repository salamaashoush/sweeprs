//! Files a running process holds open.
//!
//! Deleting an open file unlinks its name but returns no space: the blocks stay
//! allocated until the last process closes it. A running program's own binary
//! and libraries behave the same way, and removing them breaks the program the
//! next time it loads a sibling file. A clean skips both.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

pub struct OpenFiles(BTreeSet<PathBuf>);

impl OpenFiles {
    /// Every file open in a process this user can inspect, or `None` if the
    /// system would not say.
    pub fn snapshot() -> Option<Self> {
        collect().map(Self)
    }

    /// An open file at or below `path`, if there is one.
    ///
    /// The system reports resolved paths, so `/var/folders/...` is looked up as
    /// `/private/var/folders/...` too.
    pub fn held_under(&self, path: &Path) -> Option<&Path> {
        self.first_under(path).or_else(|| {
            path.canonicalize()
                .ok()
                .and_then(|real| self.first_under(&real))
        })
    }

    fn first_under(&self, path: &Path) -> Option<&Path> {
        // Paths order component by component, so everything below `path`
        // sorts directly after it.
        self.0
            .range(path.to_path_buf()..)
            .next()
            .filter(|open| open.starts_with(path))
            .map(PathBuf::as_path)
    }
}

#[cfg(target_os = "macos")]
fn collect() -> Option<BTreeSet<PathBuf>> {
    let output = crate::util::run_with_timeout(
        &["lsof", "-n", "-P", "-w", "-F", "fn"],
        std::time::Duration::from_secs(10),
    )
    .success()?;
    Some(parse_lsof(&String::from_utf8_lossy(&output.stdout)))
}

/// Parse `lsof -F fn`: an `f` line names the descriptor, the `n` line after it
/// the file. A working directory holds no data open, so it does not count.
#[cfg(any(target_os = "macos", test))]
fn parse_lsof(output: &str) -> BTreeSet<PathBuf> {
    let mut open = BTreeSet::new();
    let mut descriptor = "";
    for line in output.lines() {
        if let Some(fd) = line.strip_prefix('f') {
            descriptor = fd;
        } else if let Some(name) = line.strip_prefix('n')
            && name.starts_with('/')
            && !matches!(descriptor, "cwd" | "rtd")
        {
            open.insert(PathBuf::from(name));
        }
    }
    open
}

#[cfg(target_os = "linux")]
fn collect() -> Option<BTreeSet<PathBuf>> {
    let mut open = BTreeSet::new();
    for process in std::fs::read_dir("/proc").ok()?.flatten() {
        let pid_dir = process.path();
        if !process
            .file_name()
            .to_string_lossy()
            .bytes()
            .all(|b| b.is_ascii_digit())
        {
            continue;
        }
        if let Ok(exe) = std::fs::read_link(pid_dir.join("exe")) {
            open.insert(exe);
        }
        let Ok(fds) = std::fs::read_dir(pid_dir.join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            if let Ok(target) = std::fs::read_link(fd.path())
                && target.is_absolute()
            {
                open.insert(target);
            }
        }
    }
    Some(open)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lsof_output_keeps_open_files_and_ignores_working_directories() {
        let output = "p42\nfcwd\nn/Users/sashoush/Workspace/proj/node_modules\nftxt\nn/Users/sashoush/.local/share/mise/installs/node/24.22.2/bin/node\nf3\nn/private/tmp/app/state.db\nf4\nn127.0.0.1:8080\n";
        let open = parse_lsof(output);
        assert_eq!(
            open.into_iter().collect::<Vec<_>>(),
            vec![
                PathBuf::from("/Users/sashoush/.local/share/mise/installs/node/24.22.2/bin/node"),
                PathBuf::from("/private/tmp/app/state.db"),
            ]
        );
    }

    #[test]
    fn a_tree_is_held_when_anything_inside_it_is_open() {
        let files = OpenFiles(
            [
                "/Users/sashoush/.local/share/mise/installs/node/24.22.2/bin/node",
                "/Users/sashoush/Library/Caches/app0/blob",
            ]
            .into_iter()
            .map(PathBuf::from)
            .collect(),
        );
        assert!(
            files
                .held_under(Path::new(
                    "/Users/sashoush/.local/share/mise/installs/node/24.22.2"
                ))
                .is_some()
        );
        assert!(
            files
                .held_under(Path::new(
                    "/Users/sashoush/.local/share/mise/installs/node/24.22.1"
                ))
                .is_none()
        );
        assert!(
            files
                .held_under(Path::new("/Users/sashoush/Library/Caches/app"))
                .is_none()
        );
    }

    #[test]
    fn a_file_this_process_holds_open_is_found() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let path = tmp.path().join("held.db");
        let _held = std::fs::File::create(&path).expect("create");
        let files = OpenFiles::snapshot().expect("snapshot");
        assert!(
            files.held_under(tmp.path()).is_some(),
            "expected {} to be reported open",
            path.display()
        );
    }
}
