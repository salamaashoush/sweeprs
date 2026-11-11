use std::sync::{LazyLock, OnceLock};
use std::time::Duration;

use rustc_hash::FxHashMap;

use crate::util;

/// Maximum time to wait for any single CLI command.
const COMMAND_TIMEOUT: Duration = Duration::from_secs(5);

/// Result of a prefetched CLI command.
#[derive(Debug, Clone)]
pub struct CliResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// Every CLI command a rule reads, each in its own slot.
///
/// `prefetch` starts them all at once at the beginning of a scan. A rule that
/// asks for one waits for that command alone: one slow command (`xcrun simctl`
/// takes most of a second) holds up only the rules that read it.
static CLI_CACHE: LazyLock<FxHashMap<&'static str, Slot>> = LazyLock::new(|| {
    commands()
        .into_iter()
        .map(|(key, args)| {
            (
                key,
                Slot {
                    args,
                    result: OnceLock::new(),
                },
            )
        })
        .collect()
});

struct Slot {
    args: &'static [&'static str],
    result: OnceLock<CliResult>,
}

impl Slot {
    fn result(&self) -> &CliResult {
        self.result.get_or_init(|| run(self.args))
    }
}

fn commands() -> Vec<(&'static str, &'static [&'static str])> {
    let mut commands: Vec<(&str, &[&str])> = vec![
        (
            "rustup_active_toolchain",
            &["rustup", "show", "active-toolchain"],
        ),
        ("rustup_toolchain_list", &["rustup", "toolchain", "list"]),
        ("node_version", &["node", "--version"]),
        ("python3_version", &["python3", "--version"]),
        ("ruby_version", &["ruby", "--version"]),
        ("java_version", &["java", "--version"]),
        ("docker_system_df", &["docker", "system", "df"]),
        ("mise_prunable", &["mise", "ls", "--prunable", "--json"]),
    ];

    #[cfg(target_os = "macos")]
    commands.extend_from_slice(&[
        ("tmutil_snapshots", &["tmutil", "listlocalsnapshots", "/"]),
        ("brew_cleanup", &["brew", "cleanup", "-n"]),
        ("brew_autoremove", &["brew", "autoremove", "--dry-run"]),
        ("diskutil_apfs_list", &["diskutil", "apfs", "list"]),
        ("diskutil_info_root", &["diskutil", "info", "-plist", "/"]),
        (
            "simctl_runtime_list",
            &["xcrun", "simctl", "runtime", "list"],
        ),
        (
            "simctl_devices_json",
            &["xcrun", "simctl", "list", "devices", "-j"],
        ),
    ]);

    #[cfg(target_os = "linux")]
    commands.extend_from_slice(&[
        ("journalctl_disk_usage", &["journalctl", "--disk-usage"]),
        ("uname_r", &["uname", "-r"]),
    ]);

    commands
}

fn run(args: &[&str]) -> CliResult {
    match util::run_with_timeout(args, COMMAND_TIMEOUT) {
        util::CommandOutcome::Completed(output) => CliResult {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        },
        util::CommandOutcome::TimedOut | util::CommandOutcome::NotSpawned(_) => CliResult {
            success: false,
            stdout: String::new(),
            stderr: String::new(),
        },
    }
}

/// Run every command concurrently; returns once all have finished.
pub fn prefetch() {
    std::thread::scope(|s| {
        for slot in CLI_CACHE.values() {
            s.spawn(|| {
                slot.result();
            });
        }
    });
}

/// Get a CLI result by key, running the command if nothing has yet. Returns
/// `None` if the command failed or the key is unknown.
pub fn get(key: &str) -> Option<&'static CliResult> {
    get_raw(key).filter(|r| r.success)
}

/// Get the raw result regardless of success status.
pub fn get_raw(key: &str) -> Option<&'static CliResult> {
    CLI_CACHE.get(key).map(Slot::result)
}
