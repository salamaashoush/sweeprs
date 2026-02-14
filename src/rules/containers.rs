use std::path::Path;

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

/// Scan for Podman, Lima, and Colima container runtime data.
pub struct PodmanRule;
pub struct LimaRule;
pub struct ColimaRule;

impl CleanupRule for PodmanRule {
    fn name(&self) -> &'static str {
        "Podman data"
    }

    fn category(&self) -> Category {
        Category::Docker
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let mut entries = Vec::new();

        // Podman stores container data in ~/.local/share/containers
        let containers_dir = home.join(".local/share/containers");
        if containers_dir.exists() {
            let size = walker::dir_size(&containers_dir);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: containers_dir,
                    size,
                    category: Category::Docker,
                    safety: SafetyLevel::Caution,
                    description: "Podman container data".to_owned(),
                    item_count: None,
                });
            }
        }

        // Podman cache
        let cache_dir = home.join(".cache/containers");
        if cache_dir.exists() {
            let size = walker::dir_size(&cache_dir);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: cache_dir,
                    size,
                    category: Category::Docker,
                    safety: SafetyLevel::Safe,
                    description: "Podman cache".to_owned(),
                    item_count: None,
                });
            }
        }

        entries
    }
}

impl CleanupRule for LimaRule {
    fn name(&self) -> &'static str {
        "Lima VMs"
    }

    fn category(&self) -> Category {
        Category::Docker
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let lima_dir = home.join(".lima");

        if !lima_dir.exists() {
            return Vec::new();
        }

        let colima_migrated = home.join(".colima").exists();
        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&lima_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();

                // Skip _config and _cache dirs
                if name_str.starts_with('_') {
                    continue;
                }

                // Detect orphaned Colima data left in the old ~/.lima/colima/ location.
                // Colima moved its diffdisk from ~/.lima/colima/diffdisk to
                // ~/.colima/_lima/colima/diffdisk. The old file was never removed,
                // commonly 50+ GB of wasted space.
                if name_str == "colima" && colima_migrated {
                    let diffdisk = path.join("diffdisk");
                    if diffdisk.exists() {
                        let dd_size = file_size(&diffdisk);
                        if dd_size > 0 {
                            entries.push(ScannedEntry {
                                path: diffdisk,
                                size: dd_size,
                                category: Category::Docker,
                                safety: SafetyLevel::Safe,
                                description: "Orphaned Colima diffdisk (old location, Colima migrated to ~/.colima)".to_owned(),
                                item_count: None,
                            });
                        }
                    }

                    // Report the rest of the old colima dir (basedisk, etc.)
                    // Use shallow size to avoid walking into huge dirs
                    let rest_size = shallow_dir_size(&path, &["diffdisk"]);
                    if rest_size > 0 {
                        entries.push(ScannedEntry {
                            path,
                            size: rest_size,
                            category: Category::Docker,
                            safety: SafetyLevel::Caution,
                            description: "Orphaned Colima VM dir (old ~/.lima/colima location)".to_owned(),
                            item_count: None,
                        });
                    }
                    continue;
                }

                // For regular Lima VMs, just report diffdisk + basedisk sizes
                // (avoids expensive recursive walk of VM internals)
                let size = vm_instance_size(&path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::Docker,
                        safety: SafetyLevel::Caution,
                        description: format!("Lima VM: {name_str}"),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

impl CleanupRule for ColimaRule {
    fn name(&self) -> &'static str {
        "Colima data"
    }

    fn category(&self) -> Category {
        Category::Docker
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let colima_dir = home.join(".colima");

        if !colima_dir.exists() {
            return Vec::new();
        }

        let mut entries = Vec::new();

        // Scan VM instances inside ~/.colima/_lima/
        // Layout: ~/.colima/_lima/<instance>/{diffdisk,basedisk,...}
        let lima_dir = colima_dir.join("_lima");
        if lima_dir.exists() {
            if let Ok(read_dir) = std::fs::read_dir(&lima_dir) {
                for entry in read_dir.flatten() {
                    let path = entry.path();
                    if !path.is_dir() {
                        continue;
                    }
                    let name = entry.file_name();
                    let name_str = name.to_string_lossy().to_string();

                    // Skip internal Lima dirs (_config, _disks, _networks, etc.)
                    if name_str.starts_with('_') {
                        continue;
                    }

                    // Report diffdisk separately -- it's the writable layer and typically largest
                    let diffdisk = path.join("diffdisk");
                    if diffdisk.exists() {
                        let dd_size = file_size(&diffdisk);
                        if dd_size > 0 {
                            entries.push(ScannedEntry {
                                path: diffdisk,
                                size: dd_size,
                                category: Category::Docker,
                                safety: SafetyLevel::Caution,
                                description: format!("Colima VM disk: {name_str}"),
                                item_count: None,
                            });
                        }
                    }

                    // Report rest of instance (basedisk, cidata, logs, config)
                    let rest = shallow_dir_size(&path, &["diffdisk"]);
                    if rest > 0 {
                        entries.push(ScannedEntry {
                            path,
                            size: rest,
                            category: Category::Docker,
                            safety: SafetyLevel::Caution,
                            description: format!("Colima VM base: {name_str}"),
                            item_count: None,
                        });
                    }
                }
            }
        }

        // Report top-level Colima dirs/files outside _lima
        // (profiles, _store, _templates, ssh_config, etc.)
        if let Ok(read_dir) = std::fs::read_dir(&colima_dir) {
            for entry in read_dir.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();
                if name_str == "_lima" {
                    continue;
                }
                let path = entry.path();
                let size = if path.is_dir() {
                    walker::dir_size(&path)
                } else {
                    file_size(&path)
                };
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::Docker,
                        safety: SafetyLevel::Caution,
                        description: format!("Colima: {name_str}"),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

/// Get size of a single file via metadata (no recursive walk).
fn file_size(path: &Path) -> u64 {
    path.metadata().map(|m| m.len()).unwrap_or(0)
}

/// Sum sizes of direct children in a directory, excluding named files.
/// Only stats immediate entries -- never recurses. Fast for dirs with
/// large files like disk images.
fn shallow_dir_size(dir: &Path, exclude: &[&str]) -> u64 {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut total = 0u64;
    for entry in read_dir.flatten() {
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if exclude.contains(&name_str.as_ref()) {
            continue;
        }
        if let Ok(meta) = entry.metadata() {
            if meta.is_file() {
                total += meta.len();
            }
            // Skip subdirs to keep it fast -- we only care about
            // the obvious big files (basedisk, cidata, etc.)
        }
    }
    total
}

/// Estimate VM instance size by summing known large files.
/// Much faster than walking the entire directory tree.
fn vm_instance_size(instance_dir: &Path) -> u64 {
    let known_files = ["diffdisk", "basedisk", "cidata.iso"];
    let mut total = 0u64;
    for name in &known_files {
        let p = instance_dir.join(name);
        if p.exists() {
            total += file_size(&p);
        }
    }
    // If none of the known files exist, fall back to shallow scan
    if total == 0 {
        total = shallow_dir_size(instance_dir, &[]);
    }
    total
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(PodmanRule),
        Box::new(LimaRule),
        Box::new(ColimaRule),
    ]
}
