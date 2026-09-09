use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::cli_cache;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};

pub struct DockerRule;

/// Known Docker resource types from `docker system df`.
/// Each maps to a `docker <subcommand> prune` command for cleanup.
const DOCKER_TYPES: &[(&str, &[&str])] = &[
    ("Images", &["image", "prune", "-af"]),
    ("Containers", &["container", "prune", "-f"]),
    ("Local Volumes", &["volume", "prune", "-af"]),
    ("Build Cache", &["builder", "prune", "-af"]),
];

impl DockerRule {
    fn parse_docker_df() -> Vec<ScannedEntry> {
        // Check if docker command was attempted and failed (daemon not running)
        // vs simply not installed (no result at all)
        let raw = cli_cache::get_raw("docker_system_df");
        if let Some(result) = raw {
            if !result.success {
                // Docker is installed but daemon is not running -- surface a warning entry
                let stderr = result.stderr.trim();
                let msg = if stderr.contains("connect:") || stderr.contains("Cannot connect") {
                    "Docker daemon not running (start Colima/Docker Desktop to scan images)"
                } else {
                    "Docker command failed (daemon may not be running)"
                };
                return vec![ScannedEntry {
                    path: std::path::PathBuf::from("docker:warning"),
                    size: 0,
                    category: Category::Docker,
                    safety: SafetyLevel::Error,
                    description: msg.to_owned(),
                    item_count: None,
                }];
            }
        } else {
            // No result at all -- docker not installed or CLI_CACHE didn't run, skip silently
            return Vec::new();
        }

        let result = raw.unwrap();

        let mut entries = Vec::new();

        // `docker system df` output is fixed-width columns. Parse by matching
        // known type names at the start of each line, then extracting SIZE
        // from the fixed column position.
        //
        //   TYPE            TOTAL     ACTIVE    SIZE      RECLAIMABLE
        //   Images          6         0         4.608GB   4.608GB (100%)
        //   Local Volumes   0         0         0B        0B
        for line in result.stdout.lines().skip(1) {
            let trimmed = line.trim();
            if trimmed.is_empty() {
                continue;
            }

            // Match against known type names.
            let mut matched_type: Option<&str> = None;
            let mut remainder = "";
            for &(type_name, _) in DOCKER_TYPES {
                if let Some(rest) = trimmed.strip_prefix(type_name) {
                    matched_type = Some(type_name);
                    remainder = rest;
                    break;
                }
            }

            let Some(type_name) = matched_type else {
                continue;
            };

            // After stripping the type name, the remaining columns are:
            //   TOTAL  ACTIVE  SIZE  RECLAIMABLE
            let cols: Vec<&str> = remainder.split_whitespace().collect();
            if cols.len() < 3 {
                continue;
            }

            let size_str = cols[2]; // SIZE column

            if let Some(size) = parse_docker_size(size_str) {
                if size > 0 {
                    // Parse the actual reclaimable size (not the percentage)
                    let reclaim_size = if cols.len() >= 4 {
                        parse_docker_size(cols[3]).unwrap_or(size)
                    } else {
                        size
                    };

                    entries.push(ScannedEntry {
                        path: std::path::PathBuf::from(format!("docker:{type_name}")),
                        size: reclaim_size,
                        category: Category::Docker,
                        safety: SafetyLevel::Caution,
                        description: format!(
                            "Docker {type_name} (inside VM, reclaimable via prune)"
                        ),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

fn parse_docker_size(s: &str) -> Option<u64> {
    let s = s.trim();
    if s == "0B" || s.is_empty() {
        return Some(0);
    }

    let (num_str, unit) = if s.ends_with("GB") {
        (s.trim_end_matches("GB"), 1_000_000_000u64)
    } else if s.ends_with("MB") {
        (s.trim_end_matches("MB"), 1_000_000u64)
    } else if s.ends_with("kB") {
        (s.trim_end_matches("kB"), 1_000u64)
    } else if s.ends_with('B') {
        (s.trim_end_matches('B'), 1u64)
    } else {
        return None;
    };

    num_str
        .parse::<f64>()
        .ok()
        .map(|n| (n * unit as f64) as u64)
}

/// Run the appropriate `docker ... prune` command for a Docker entry.
///
/// The entry path is expected to be `docker:<type>` where `<type>` matches
/// one of the known types from `docker system df` (e.g. "Images", "Build Cache").
///
/// Returns `Ok(())` on success, or an error if the command fails.
pub fn clean_docker_entry(entry_path: &str) -> Result<Option<u64>, std::io::Error> {
    let type_key = entry_path.strip_prefix("docker:").unwrap_or(entry_path);

    let Some(args) = DOCKER_TYPES
        .iter()
        .find(|&&(name, _)| name == type_key)
        .map(|&(_, args)| args)
    else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("unknown docker type: {type_key}"),
        ));
    };

    // `docker system df` reports what the daemon believes is reclaimable; prune
    // reports what it actually removed. Report the second.
    let output = std::process::Command::new("docker")
        .args(args)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .output()?;

    if !output.status.success() {
        return Err(std::io::Error::other(format!(
            "docker {} failed with exit code {}",
            args.join(" "),
            output.status
        )));
    }

    Ok(parse_reclaimed(&String::from_utf8_lossy(&output.stdout)))
}

/// Pull the byte count out of prune's closing `Total reclaimed space: 4.608GB`.
fn parse_reclaimed(stdout: &str) -> Option<u64> {
    stdout
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("Total reclaimed space:"))
        .and_then(|value| parse_docker_size(value.trim()))
}

impl CleanupRule for DockerRule {
    fn name(&self) -> &'static str {
        "Docker"
    }

    fn category(&self) -> Category {
        Category::Docker
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        Self::parse_docker_df()
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(DockerRule)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prune_output_yields_the_bytes_it_actually_removed() {
        let stdout = "deleted: sha256:abc\ndeleted: sha256:def\n\nTotal reclaimed space: 4.608GB\n";
        assert_eq!(parse_reclaimed(stdout), Some(4_608_000_000));
    }

    #[test]
    fn a_prune_that_removed_nothing_reports_zero_rather_than_the_estimate() {
        assert_eq!(parse_reclaimed("Total reclaimed space: 0B\n"), Some(0));
    }

    #[test]
    fn output_without_the_summary_line_falls_back_to_the_estimate() {
        assert_eq!(parse_reclaimed("some other output\n"), None);
    }

    #[test]
    fn reclaimable_column_is_parsed_across_units() {
        assert_eq!(parse_docker_size("512MB"), Some(512_000_000));
        assert_eq!(parse_docker_size("1.5kB"), Some(1_500));
        assert_eq!(parse_docker_size("0B"), Some(0));
        assert_eq!(parse_docker_size("nonsense"), None);
    }
}
