use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

/// Scan for Vagrant, Multipass, Nix, asdf, and SDKMAN data.
pub struct VagrantBoxesRule;
pub struct MultipassRule;
pub struct NixStoreRule;
pub struct AsdfInstallsRule;
pub struct SdkmanRule;

impl CleanupRule for VagrantBoxesRule {
    fn name(&self) -> &'static str {
        "Vagrant boxes"
    }

    fn category(&self) -> Category {
        Category::Docker
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let boxes_dir = home.join(".vagrant.d/boxes");

        if !boxes_dir.exists() {
            return Vec::new();
        }

        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&boxes_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let name = entry.file_name();
                let name_str = name.to_string_lossy().to_string();

                let size = walker::dir_size(&path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::Docker,
                        safety: SafetyLevel::Caution,
                        description: format!("Vagrant box: {name_str}"),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

impl CleanupRule for MultipassRule {
    fn name(&self) -> &'static str {
        "Multipass VMs"
    }

    fn category(&self) -> Category {
        Category::Docker
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let multipass_dir = home.join(".multipass");

        if !multipass_dir.exists() {
            return Vec::new();
        }

        let size = walker::dir_size(&multipass_dir);
        if size == 0 {
            return Vec::new();
        }

        vec![ScannedEntry {
            path: multipass_dir,
            size,
            category: Category::Docker,
            safety: SafetyLevel::Caution,
            description: "Multipass VM data".to_owned(),
            item_count: None,
        }]
    }
}

impl CleanupRule for NixStoreRule {
    fn name(&self) -> &'static str {
        "Nix store"
    }

    fn category(&self) -> Category {
        Category::PackageCache
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let nix_store = std::path::PathBuf::from("/nix/store");

        if !nix_store.exists() {
            return Vec::new();
        }

        let size = walker::dir_size(&nix_store);
        if size == 0 {
            return Vec::new();
        }

        vec![ScannedEntry {
            path: nix_store,
            size,
            category: Category::PackageCache,
            safety: SafetyLevel::Caution,
            description: "Nix store (run `nix-collect-garbage` to clean)".to_owned(),
            item_count: None,
        }]
    }
}

impl CleanupRule for AsdfInstallsRule {
    fn name(&self) -> &'static str {
        "asdf installs"
    }

    fn category(&self) -> Category {
        Category::Toolchain
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let installs_dir = home.join(".asdf/installs");

        if !installs_dir.exists() {
            return Vec::new();
        }

        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&installs_dir) {
            for entry in read_dir.flatten() {
                let tool_path = entry.path();
                if !tool_path.is_dir() {
                    continue;
                }

                let tool_name = entry.file_name();
                let tool_name_str = tool_name.to_string_lossy().to_string();

                let size = walker::dir_size(&tool_path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path: tool_path,
                        size,
                        category: Category::Toolchain,
                        safety: SafetyLevel::Caution,
                        description: format!("asdf {tool_name_str} versions"),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

impl CleanupRule for SdkmanRule {
    fn name(&self) -> &'static str {
        "SDKMAN candidates"
    }

    fn category(&self) -> Category {
        Category::Toolchain
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let candidates_dir = home.join(".sdkman/candidates");

        if !candidates_dir.exists() {
            return Vec::new();
        }

        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&candidates_dir) {
            for entry in read_dir.flatten() {
                let candidate_path = entry.path();
                if !candidate_path.is_dir() {
                    continue;
                }

                let candidate_name = entry.file_name();
                let candidate_name_str = candidate_name.to_string_lossy().to_string();

                let size = walker::dir_size(&candidate_path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path: candidate_path,
                        size,
                        category: Category::Toolchain,
                        safety: SafetyLevel::Caution,
                        description: format!("SDKMAN {candidate_name_str} versions"),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(VagrantBoxesRule),
        Box::new(MultipassRule),
        Box::new(NixStoreRule),
        Box::new(AsdfInstallsRule),
        Box::new(SdkmanRule),
    ]
}
