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
        let Some(result) = cli_cache::get("docker_system_df") else {
            return Vec::new();
        };

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
            let reclaimable_str = cols.last().unwrap_or(&"0B").trim_end_matches(['%', ')']);

            if let Some(size) = parse_docker_size(size_str) {
                if size > 0 {
                    entries.push(ScannedEntry {
                        path: std::path::PathBuf::from(format!("docker:{type_name}")),
                        size,
                        category: Category::Docker,
                        safety: SafetyLevel::Caution,
                        description: format!("Docker {type_name} (reclaimable: {reclaimable_str})"),
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
pub fn clean_docker_entry(entry_path: &str) -> Result<(), std::io::Error> {
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

    let status = std::process::Command::new("docker")
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other(format!(
            "docker {} failed with exit code {}",
            args.join(" "),
            status
        )))
    }
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
