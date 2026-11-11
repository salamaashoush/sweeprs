//! The last check before anything is removed from disk.
//!
//! Rules decide what is reclaimable; this decides what may never be removed,
//! whatever a rule says. A rule once handed the cleaner `/private/tmp` itself,
//! and nothing between the rule and `remove_dir_all` noticed.

use std::io;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};

const SYSTEM_ROOTS: &[&str] = &[
    "/Applications",
    "/Library",
    "/Library/Logs",
    "/System",
    "/Users",
    "/Volumes",
    "/bin",
    "/boot",
    "/dev",
    "/etc",
    "/home",
    "/lib",
    "/lib64",
    "/media",
    "/mnt",
    "/nix",
    "/opt",
    "/private",
    "/private/etc",
    "/private/tmp",
    "/private/var",
    "/private/var/folders",
    "/private/var/log",
    "/private/var/tmp",
    "/proc",
    "/root",
    "/run",
    "/sbin",
    "/srv",
    "/sys",
    "/tmp",
    "/usr",
    "/usr/local",
    "/var",
    "/var/cache",
    "/var/folders",
    "/var/lib",
    "/var/log",
    "/var/tmp",
];

const HOME_ROOTS: &[&str] = &[
    ".Trash",
    ".cache",
    ".config",
    ".gnupg",
    ".local",
    ".local/share",
    ".local/share/Trash",
    ".local/state",
    ".ssh",
    "Applications",
    "Desktop",
    "Documents",
    "Downloads",
    "Library",
    "Library/Application Support",
    "Library/Caches",
    "Library/Containers",
    "Library/Group Containers",
    "Library/Logs",
    "Library/Mobile Documents",
    "Library/Preferences",
    "Movies",
    "Music",
    "Pictures",
    "Public",
];

const ENV_ROOTS: &[&str] = &[
    "TMPDIR",
    "XDG_CACHE_HOME",
    "XDG_CONFIG_HOME",
    "XDG_DATA_HOME",
    "XDG_RUNTIME_DIR",
    "XDG_STATE_HOME",
];

fn protected_roots(home: Option<&Path>) -> Vec<PathBuf> {
    let mut roots: Vec<PathBuf> = SYSTEM_ROOTS.iter().map(PathBuf::from).collect();
    if let Some(home) = home {
        roots.push(home.to_path_buf());
        roots.extend(HOME_ROOTS.iter().map(|rel| home.join(rel)));
    }
    roots.extend(
        ENV_ROOTS
            .iter()
            .filter_map(std::env::var_os)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute()),
    );
    roots.extend(
        [dirs::cache_dir(), dirs::config_dir(), dirs::data_dir()]
            .into_iter()
            .flatten(),
    );
    let resolved: Vec<PathBuf> = roots.iter().filter_map(|r| r.canonicalize().ok()).collect();
    roots.extend(resolved);
    roots
}

/// Where removing `path` actually lands: its parent resolved, its own name kept.
///
/// Removing a symlink removes the link, not its target, so only the parent is
/// resolved. `/tmp/x` on macOS is `/private/tmp/x`.
fn real_location(path: &Path) -> Option<PathBuf> {
    let parent = path.parent()?.canonicalize().ok()?;
    Some(parent.join(path.file_name()?))
}

/// A per-user temp, cache or state root under `/var/folders/<xx>/<id>/`.
fn is_var_folders_root(path: &Path) -> bool {
    ["/private/var/folders", "/var/folders"]
        .iter()
        .filter_map(|base| path.strip_prefix(base).ok())
        .any(|rest| rest.components().count() <= 3)
}

fn is_home_dir(path: &Path) -> bool {
    path.parent()
        .is_some_and(|p| p == Path::new("/Users") || p == Path::new("/home"))
}

fn is_mount_point(path: &Path) -> bool {
    let (Ok(meta), Some(parent)) = (path.symlink_metadata(), path.parent()) else {
        return false;
    };
    meta.is_dir() && parent.metadata().is_ok_and(|p| p.dev() != meta.dev())
}

fn refusal(path: &Path, reason: &str) -> io::Error {
    io::Error::other(format!("refusing to delete {}: {reason}", path.display()))
}

/// Refuse to remove `path` if it is, or contains, something no rule may delete.
pub fn check(path: &Path) -> io::Result<()> {
    check_with_home(path, dirs::home_dir().as_deref())
}

fn check_with_home(path: &Path, home: Option<&Path>) -> io::Result<()> {
    if !path.is_absolute() {
        return Err(refusal(path, "not an absolute path"));
    }
    if path.components().any(|c| c == Component::ParentDir) {
        return Err(refusal(path, "contains `..`"));
    }
    let roots = protected_roots(home);
    let candidates: Vec<PathBuf> = std::iter::once(path.to_path_buf())
        .chain(real_location(path))
        .collect();
    for candidate in &candidates {
        if let Some(root) = roots.iter().find(|root| root.starts_with(candidate)) {
            return Err(if root == candidate {
                refusal(path, "a protected directory")
            } else {
                refusal(path, &format!("contains the protected {}", root.display()))
            });
        }
        if is_var_folders_root(candidate) {
            return Err(refusal(path, "a per-user temp or cache root"));
        }
        if is_home_dir(candidate) {
            return Err(refusal(path, "a home directory"));
        }
    }
    if is_mount_point(path) {
        return Err(refusal(path, "a mount point"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn refused(path: &str, home: &Path) -> bool {
        check_with_home(Path::new(path), Some(home)).is_err()
    }

    #[test]
    fn system_and_temp_roots_are_refused() {
        let home = Path::new("/Users/sashoush");
        for path in [
            "/",
            "/tmp",
            "/private/tmp",
            "/var/tmp",
            "/var/log",
            "/usr",
            "/private/var/folders/0c/mg2sx8hd5x17_yctc6cc3v440000gn/T",
            "/var/folders/0c/mg2sx8hd5x17_yctc6cc3v440000gn",
            "/private/var/folders/0c",
        ] {
            assert!(refused(path, home), "{path} should be refused");
        }
    }

    #[test]
    fn home_and_its_standard_folders_are_refused() {
        let home = Path::new("/Users/sashoush");
        for path in [
            "/Users/sashoush",
            "/Users/someone-else",
            "/home/sashoush",
            "/Users/sashoush/Library",
            "/Users/sashoush/Library/Caches",
            "/Users/sashoush/Downloads",
            "/Users/sashoush/.Trash",
            "/Users/sashoush/.cache",
            "/Users/sashoush/.local/share",
        ] {
            assert!(refused(path, home), "{path} should be refused");
        }
    }

    #[test]
    fn what_rules_actually_offer_is_allowed() {
        let home = Path::new("/Users/sashoush");
        for path in [
            "/Users/sashoush/Library/Caches/Google/Chrome",
            "/Users/sashoush/.cache/mise",
            "/Users/sashoush/.local/share/mise/installs/node/24.22.2",
            "/Users/sashoush/Downloads/installer.dmg",
            "/Users/sashoush/.Trash/old.zip",
            "/private/var/folders/0c/mg2sx8hd5x17_yctc6cc3v440000gn/T/leftover",
            "/private/tmp/build-output",
        ] {
            assert!(!refused(path, home), "{path} should be allowed");
        }
    }

    #[test]
    fn relative_and_dot_dot_paths_are_refused() {
        let home = Path::new("/Users/sashoush");
        assert!(refused("Library/Caches/x", home));
        assert!(refused("/Users/sashoush/Library/Caches/x/../..", home));
    }

    #[test]
    fn a_symlinked_parent_is_judged_by_where_it_points() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let protected = tmp.path().join("home");
        std::fs::create_dir_all(protected.join("Library")).expect("home");
        let link = tmp.path().join("alias");
        std::os::unix::fs::symlink(&protected, &link).expect("symlink");

        assert!(check_with_home(&link.join("Library"), Some(&protected)).is_err());
        std::fs::create_dir(protected.join("Library/Junk")).expect("junk");
        assert!(check_with_home(&link.join("Library/Junk"), Some(&protected)).is_ok());
    }
}
