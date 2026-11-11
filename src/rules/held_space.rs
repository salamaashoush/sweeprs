//! Space held by files that were deleted while a process still had them open.
//!
//! Unlinking an open file removes its name, not its blocks: they come back only
//! when the last process holding it closes it. A long-running dev server, a
//! language server or an agent's tool server can sit on gigabytes this way,
//! invisible to every directory walk. The only way to return that space is to
//! quit the process, so the entry stands for doing that, and it is `Danger`:
//! nothing quits a process unless asked to by name.

use std::io;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::util;

pub const PREFIX: &str = "held-space:";

/// Below this a process is not worth quitting for its space.
const MIN_HELD: u64 = 16 * 1024 * 1024;

pub struct HeldSpaceRule;

impl CleanupRule for HeldSpaceRule {
    fn name(&self) -> &'static str {
        "Space held by deleted files"
    }

    fn category(&self) -> Category {
        Category::SystemJunk
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        held_by_process()
            .into_iter()
            .filter(|&(_, held)| held >= MIN_HELD)
            .filter_map(|(pid, held)| {
                let exe = exe_of(pid)?;
                if !is_developer_tool(&exe, &home) {
                    return None;
                }
                let name = exe.file_name()?.to_string_lossy().into_owned();
                let orphaned = parent_of(pid) == Some(1);
                Some(ScannedEntry {
                    path: PathBuf::from(format!("{PREFIX}{pid}:{}", exe.display())),
                    size: held,
                    category: Category::SystemJunk,
                    safety: SafetyLevel::Danger,
                    description: format!(
                        "Deleted files kept open by {name} (pid {pid}{})",
                        if orphaned { ", orphaned" } else { "" }
                    ),
                    item_count: None,
                })
            })
            .collect()
    }
}

/// Programs a user started for development. Quitting the window server, a
/// system agent or a desktop app to reclaim its cache is never the answer.
fn is_developer_tool(exe: &Path, home: &Path) -> bool {
    exe.starts_with(home)
        || ["/opt", "/usr/local", "/nix"]
            .iter()
            .any(|root| exe.starts_with(root))
}

#[cfg(target_os = "macos")]
fn held_by_process() -> FxHashMap<u32, u64> {
    let Some(output) = util::run_with_timeout(
        &["lsof", "-n", "-P", "-w", "+L1", "-F", "psn"],
        Duration::from_secs(10),
    )
    .success() else {
        return FxHashMap::default();
    };
    parse_lsof_unlinked(&String::from_utf8_lossy(&output.stdout))
}

/// Sum the `s` (size) of each `n` (name) under its `p` (pid) in `lsof -F psn`.
#[cfg(any(target_os = "macos", test))]
fn parse_lsof_unlinked(output: &str) -> FxHashMap<u32, u64> {
    let mut held: FxHashMap<u32, u64> = FxHashMap::default();
    let mut pid = None;
    let mut size = 0u64;
    for line in output.lines() {
        if let Some(p) = line.strip_prefix('p') {
            pid = p.parse().ok();
        } else if let Some(s) = line.strip_prefix('s') {
            size = s.parse().unwrap_or(0);
        } else if let Some(name) = line.strip_prefix('n')
            && name.starts_with('/')
            && let Some(pid) = pid
        {
            *held.entry(pid).or_default() += size;
            size = 0;
        }
    }
    held
}

#[cfg(target_os = "linux")]
fn held_by_process() -> FxHashMap<u32, u64> {
    let mut held: FxHashMap<u32, u64> = FxHashMap::default();
    let Ok(processes) = std::fs::read_dir("/proc") else {
        return held;
    };
    for process in processes.flatten() {
        let Ok(pid) = process.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let Ok(fds) = std::fs::read_dir(process.path().join("fd")) else {
            continue;
        };
        for fd in fds.flatten() {
            let deleted = std::fs::read_link(fd.path())
                .is_ok_and(|t| t.is_absolute() && t.to_string_lossy().ends_with(" (deleted)"));
            if deleted && let Ok(meta) = std::fs::metadata(fd.path()) {
                *held.entry(pid).or_default() += meta.len();
            }
        }
    }
    held
}

/// The executable image a process runs: the first `txt` file `lsof` lists.
/// `ps -o comm=` gives only a bare name for a program started through `PATH`.
#[cfg(target_os = "macos")]
fn exe_of(pid: u32) -> Option<PathBuf> {
    let output = util::run_with_timeout(
        &[
            "lsof",
            "-n",
            "-P",
            "-w",
            "-a",
            "-p",
            &pid.to_string(),
            "-d",
            "txt",
            "-F",
            "n",
        ],
        Duration::from_secs(5),
    )
    .success()?;
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            line.strip_prefix('n')
                .filter(|n| n.starts_with('/'))
                .map(PathBuf::from)
        })
}

#[cfg(target_os = "linux")]
fn exe_of(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

fn parent_of(pid: u32) -> Option<u32> {
    let output = util::run_with_timeout(
        &["ps", "-o", "ppid=", "-p", &pid.to_string()],
        Duration::from_secs(5),
    )
    .success()?;
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}

/// Quit the process an entry names, if it is still the same program.
///
/// A pid is reused once its process exits, so the executable recorded at scan
/// time must still be the one running under that pid.
pub fn clean(path_str: &str) -> io::Result<Option<u64>> {
    let spec = path_str
        .strip_prefix(PREFIX)
        .ok_or_else(|| io::Error::other("not a held-space entry"))?;
    let (pid, exe) = spec
        .split_once(':')
        .ok_or_else(|| io::Error::other("malformed held-space entry"))?;
    let pid: u32 = pid
        .parse()
        .map_err(|_| io::Error::other("malformed held-space pid"))?;
    if exe_of(pid).as_deref() != Some(Path::new(exe)) {
        return Err(io::Error::other(format!(
            "pid {pid} is no longer {exe}; left alone"
        )));
    }
    let target = nix::unistd::Pid::from_raw(
        i32::try_from(pid).map_err(|_| io::Error::other("pid out of range"))?,
    );
    nix::sys::signal::kill(target, nix::sys::signal::Signal::SIGTERM).map_err(io::Error::from)?;

    let deadline = Instant::now() + Duration::from_secs(5);
    while Instant::now() < deadline {
        if nix::sys::signal::kill(target, None).is_err() {
            return Ok(None);
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    Err(io::Error::other(format!(
        "pid {pid} was asked to quit and is still running"
    )))
}

pub fn label(path_str: &str) -> Option<String> {
    let (pid, exe) = path_str.strip_prefix(PREFIX)?.split_once(':')?;
    let name = Path::new(exe).file_name()?.to_string_lossy().into_owned();
    Some(format!("quit {name} (pid {pid})"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unlinked_sizes_are_summed_per_process() {
        let output = "p100\nf4\ns1048576\nn/private/tmp/a.log\nf7\ns2048\nn/private/tmp/b\np200\nf3\ns10\nn/x\n";
        let held = parse_lsof_unlinked(output);
        assert_eq!(held.get(&100), Some(&1_050_624));
        assert_eq!(held.get(&200), Some(&10));
    }

    #[test]
    fn only_developer_programs_are_offered() {
        let home = Path::new("/Users/sashoush");
        assert!(is_developer_tool(
            Path::new("/Users/sashoush/.local/share/mise/installs/node/24/bin/node"),
            home
        ));
        assert!(is_developer_tool(
            Path::new("/opt/homebrew/bin/postgres"),
            home
        ));
        assert!(!is_developer_tool(
            Path::new("/System/Library/CoreServices/WallpaperAgent"),
            home
        ));
        assert!(!is_developer_tool(
            Path::new("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
            home
        ));
        assert!(!is_developer_tool(Path::new("/usr/libexec/linkd"), home));
    }

    #[test]
    fn a_pid_running_something_else_is_left_alone() {
        let me = std::process::id();
        let err = clean(&format!("{PREFIX}{me}:/not/this/program")).expect_err("must refuse");
        assert!(err.to_string().contains("no longer"));
    }

    #[test]
    fn entries_are_labelled_by_program_and_pid() {
        assert_eq!(
            label("held-space:3280:/Users/sashoush/.local/bin/ferridriver").as_deref(),
            Some("quit ferridriver (pid 3280)")
        );
    }
}
