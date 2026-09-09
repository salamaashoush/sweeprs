//! Entries whose path is a command to run, not a file to delete.
//!
//! Docker images, Homebrew downloads and simulator runtimes are owned by a
//! daemon or a package manager: removing their backing directories leaves the
//! owner's registry pointing at things that no longer exist. Those entries
//! carry a `<prefix>:<argument>` marker instead of a real path, and cleaning
//! them means invoking the owning tool.
//!
//! Every consumer that deletes, archives or reveals an entry must route through
//! this module. A dispatch site that only knows some of the prefixes treats the
//! rest as relative paths, which silently does nothing.

use std::io;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::config::Config;
use crate::rules::{brew, docker, empty_dirs, git_data, ide};
use crate::util;

const PREFIXES: &[&str] = &[
    "git-gc:",
    "docker:",
    "brew:",
    "journal:",
    "pacman:",
    "apt:",
    "dnf:",
    "simctl-runtime:",
    "simctl-device:",
    "empty-dirs:",
    "editor-history:",
];

pub fn is_virtual(path: &Path) -> bool {
    let path_str = path.display().to_string();
    PREFIXES.iter().any(|p| path_str.starts_with(p))
}

/// How an entry should be shown to the user.
///
/// A virtual entry's raw marker is an argument list, not a location -- the
/// unavailable-device sweep carries every UDID it will delete -- so show the
/// command it stands for instead.
pub fn display(path: &Path) -> String {
    if is_virtual(path) {
        label(&path.display().to_string())
    } else {
        util::tilde_path(path)
    }
}

/// The command the entry stands for, shown while it runs.
pub fn label(path_str: &str) -> String {
    if let Some(repo) = path_str.strip_prefix("git-gc:") {
        return format!("git gc {}", util::tilde_path(&PathBuf::from(repo)));
    }
    if let Some(kind) = path_str.strip_prefix("docker:") {
        return format!("docker prune {kind}");
    }
    if let Some(kind) = path_str.strip_prefix("brew:") {
        return format!("brew {kind}");
    }
    if let Some(id) = path_str.strip_prefix("simctl-runtime:") {
        return format!("xcrun simctl runtime delete {id}");
    }
    if let Some(udids) = path_str.strip_prefix("simctl-device:") {
        let count = udids.split(',').count();
        return if count == 1 {
            format!("xcrun simctl delete {udids}")
        } else {
            format!("xcrun simctl delete ({count} devices)")
        };
    }
    if let Some(history) = path_str.strip_prefix("editor-history:") {
        return format!(
            "prune local file history in {}",
            util::tilde_path(&PathBuf::from(history))
        );
    }
    if let Some(root) = path_str.strip_prefix("empty-dirs:") {
        return format!(
            "remove empty directories in {}",
            util::tilde_path(&PathBuf::from(root))
        );
    }
    if path_str.starts_with("journal:") {
        return "journalctl --vacuum-size=100M".to_owned();
    }
    if path_str.starts_with("pacman:") {
        return "paccache -r -k 2".to_owned();
    }
    if path_str.starts_with("apt:") {
        return "apt clean".to_owned();
    }
    if path_str.starts_with("dnf:") {
        return "dnf clean all".to_owned();
    }
    path_str.to_owned()
}

/// Run the command an entry stands for.
///
/// The `Ok` payload is the number of bytes actually reclaimed, when the
/// underlying tool can be asked; `None` means only the scan-time estimate is
/// available.
pub fn clean(path_str: &str, config: &Config, git_gc_timeout: Duration) -> io::Result<Option<u64>> {
    if path_str.starts_with("docker:") {
        return docker::clean_docker_entry(path_str);
    }
    if path_str.starts_with("brew:") {
        return brew::clean_brew_entry(path_str);
    }
    if path_str.starts_with("simctl-runtime:") || path_str.starts_with("simctl-device:") {
        return crate::rules::simulator::clean_simulator_entry(path_str).map(|()| None);
    }
    if path_str.starts_with("git-gc:") {
        return git_data::clean_git_gc(path_str, git_gc_timeout);
    }
    if path_str.starts_with("empty-dirs:") {
        return empty_dirs::clean_empty_dirs(path_str);
    }
    if path_str.starts_with("editor-history:") {
        return ide::clean_editor_history(path_str, config);
    }
    if path_str.starts_with("journal:") {
        return linux_clean(LinuxCleanup::Journal).map(|()| None);
    }
    if path_str.starts_with("pacman:") {
        return linux_clean(LinuxCleanup::Pacman).map(|()| None);
    }
    if path_str.starts_with("apt:") {
        return linux_clean(LinuxCleanup::Apt).map(|()| None);
    }
    if path_str.starts_with("dnf:") {
        return linux_clean(LinuxCleanup::Dnf).map(|()| None);
    }

    Err(io::Error::new(
        io::ErrorKind::InvalidInput,
        format!("not a virtual entry: {path_str}"),
    ))
}

#[derive(Clone, Copy)]
enum LinuxCleanup {
    Journal,
    Pacman,
    Apt,
    Dnf,
}

#[cfg(target_os = "linux")]
fn linux_clean(which: LinuxCleanup) -> io::Result<()> {
    match which {
        LinuxCleanup::Journal => crate::rules::linux::clean_journal(),
        LinuxCleanup::Pacman => crate::rules::linux::clean_pacman(),
        LinuxCleanup::Apt => crate::rules::linux::clean_apt(),
        LinuxCleanup::Dnf => crate::rules::linux::clean_dnf(),
    }
}

#[cfg(not(target_os = "linux"))]
fn linux_clean(_which: LinuxCleanup) -> io::Result<()> {
    Err(io::Error::other("not supported on this platform"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recognises_every_prefix_it_can_clean() {
        for prefix in PREFIXES {
            let path = PathBuf::from(format!("{prefix}x"));
            assert!(is_virtual(&path), "{prefix} not recognised");
        }
    }

    #[test]
    fn real_paths_are_not_virtual() {
        assert!(!is_virtual(Path::new("/Users/me/.android/avd/Pixel_7.avd")));
        assert!(!is_virtual(Path::new("relative/cache")));
    }

    #[test]
    fn cleaning_a_real_path_is_rejected_rather_than_guessed() {
        assert!(
            clean(
                "/Users/me/Library/Caches/foo",
                &Config::default(),
                Duration::from_secs(1)
            )
            .is_err()
        );
    }

    #[test]
    fn multi_device_delete_is_labelled_by_count() {
        assert_eq!(label("simctl-device:AAAA"), "xcrun simctl delete AAAA");
        assert_eq!(
            label("simctl-device:AAAA,BBBB,CCCC"),
            "xcrun simctl delete (3 devices)"
        );
    }
}
