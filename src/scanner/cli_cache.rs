use std::process::Command;
use std::sync::LazyLock;

use rustc_hash::FxHashMap;

/// Result of a prefetched CLI command.
#[derive(Debug, Clone)]
pub struct CliResult {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

/// All CLI commands that rules need, prefetched in parallel on first access.
///
/// The 7 commands run concurrently via `std::thread::scope`, so total latency
/// equals max(individual latencies) instead of their sum.
pub static CLI_CACHE: LazyLock<FxHashMap<&'static str, CliResult>> =
    LazyLock::new(prefetch_all);

fn prefetch_all() -> FxHashMap<&'static str, CliResult> {
    let commands: &[(&str, &[&str])] = &[
        ("rustup_active_toolchain", &["rustup", "show", "active-toolchain"]),
        ("node_version", &["node", "--version"]),
        ("python3_version", &["python3", "--version"]),
        ("ruby_version", &["ruby", "--version"]),
        ("java_version", &["java", "--version"]),
        ("docker_system_df", &["docker", "system", "df"]),
        ("tmutil_snapshots", &["tmutil", "listlocalsnapshots", "/"]),
        ("brew_cleanup", &["brew", "cleanup", "-n"]),
        ("brew_autoremove", &["brew", "autoremove", "--dry-run"]),
    ];

    let mut results = FxHashMap::default();

    std::thread::scope(|s| {
        let handles: Vec<_> = commands
            .iter()
            .map(|&(key, args)| {
                s.spawn(move || {
                    let result = Command::new(args[0])
                        .args(&args[1..])
                        .output();

                    let cli_result = match result {
                        Ok(output) => CliResult {
                            success: output.status.success(),
                            stdout: String::from_utf8_lossy(&output.stdout).to_string(),
                            stderr: String::from_utf8_lossy(&output.stderr).to_string(),
                        },
                        Err(_) => CliResult {
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
