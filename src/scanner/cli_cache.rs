use std::process::Command;
use std::sync::LazyLock;
use std::time::{Duration, Instant};

use rustc_hash::FxHashMap;

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

/// Run a command with a timeout. Returns None if the command times out or fails to spawn.
fn run_with_timeout(args: &[&str], timeout: Duration) -> Option<std::process::Output> {
    let mut child = Command::new(args[0])
        .args(&args[1..])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .ok()?;

    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = child.stdout.take().map_or_else(Vec::new, |mut s| {
                    let mut buf = Vec::new();
                    std::io::Read::read_to_end(&mut s, &mut buf).ok();
                    buf
                });
                let stderr = child.stderr.take().map_or_else(Vec::new, |mut s| {
                    let mut buf = Vec::new();
                    std::io::Read::read_to_end(&mut s, &mut buf).ok();
                    buf
                });
                return Some(std::process::Output {
                    status,
                    stdout,
                    stderr,
                });
            }
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    return None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(_) => return None,
        }
    }
}

fn prefetch_all() -> FxHashMap<&'static str, CliResult> {
    let commands: &[(&str, &[&str])] = &[
        (
            "rustup_active_toolchain",
            &["rustup", "show", "active-toolchain"],
        ),
        ("node_version", &["node", "--version"]),
        ("python3_version", &["python3", "--version"]),
        ("ruby_version", &["ruby", "--version"]),
        ("java_version", &["java", "--version"]),
        ("docker_system_df", &["docker", "system", "df"]),
        ("tmutil_snapshots", &["tmutil", "listlocalsnapshots", "/"]),
        ("brew_cleanup", &["brew", "cleanup", "-n"]),
        ("brew_autoremove", &["brew", "autoremove", "--dry-run"]),
        ("diskutil_apfs_list", &["diskutil", "apfs", "list"]),
    ];

    let mut results = FxHashMap::default();

    std::thread::scope(|s| {
        let handles: Vec<_> = commands
            .iter()
            .map(|&(key, args)| {
                s.spawn(move || {
                    let cli_result = match run_with_timeout(args, COMMAND_TIMEOUT) {
                        Some(output) => CliResult {
                            success: output.status.success(),
                            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                        },
                        None => CliResult {
                            success: false,
                            stdout: String::new(),
                            stderr: String::new(),
                        },
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
