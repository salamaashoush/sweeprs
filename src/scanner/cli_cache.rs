use std::sync::LazyLock;
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

/// All CLI commands that rules need, prefetched in parallel on first access.
///
/// The commands run concurrently via `std::thread::scope`, so total latency
/// equals max(individual latencies) instead of their sum.
/// Each command has a 5-second timeout to prevent any single command from
/// blocking the entire cache.
pub static CLI_CACHE: LazyLock<FxHashMap<&'static str, CliResult>> = LazyLock::new(prefetch_all);

fn prefetch_all() -> FxHashMap<&'static str, CliResult> {
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

    let commands = commands;

    let mut results = FxHashMap::default();

    std::thread::scope(|s| {
        let handles: Vec<_> = commands
            .iter()
            .map(|&(key, args)| {
                s.spawn(move || {
                    let cli_result = match util::run_with_timeout(args, COMMAND_TIMEOUT) {
                        util::CommandOutcome::Completed(output) => CliResult {
                            success: output.status.success(),
                            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                        },
                        util::CommandOutcome::TimedOut | util::CommandOutcome::NotSpawned(_) => {
                            CliResult {
                                success: false,
                                stdout: String::new(),
                                stderr: String::new(),
                            }
                        }
                    };

                    (key, cli_result)
                })
            })
            .collect();

        for handle in handles {
            let (key, result) = handle.join().expect("CLI prefetch thread panicked");
            results.insert(key, result);
        }
    });

    results
}

/// Get a prefetched CLI result by key. Returns `None` if the command failed or wasn't found.
pub fn get(key: &str) -> Option<&'static CliResult> {
    CLI_CACHE.get(key).filter(|r| r.success)
}

/// Get the raw result regardless of success status.
pub fn get_raw(key: &str) -> Option<&'static CliResult> {
    CLI_CACHE.get(key)
}
