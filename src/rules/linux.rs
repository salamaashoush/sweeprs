use std::path::PathBuf;

use crate::config::Config;
use crate::rules::{CleanupRule, cache_rule};
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

// --- Simple cache_rule! rules for XDG and system caches ---

cache_rule!(
    XdgThumbnailRule,
    "XDG thumbnails",
    Category::LinuxSpecific,
    SafetyLevel::Safe,
    ".cache/thumbnails"
);

cache_rule!(
    XdgFontCacheRule,
    "Fontconfig cache",
    Category::LinuxSpecific,
    SafetyLevel::Safe,
    ".cache/fontconfig"
);

cache_rule!(
    MesaShaderRule,
    "Mesa shader cache",
    Category::LinuxSpecific,
    SafetyLevel::Safe,
    ".cache/mesa_shader_cache",
    ".cache/mesa_shader_cache_db"
);

cache_rule!(
    ManCacheRule,
    "Man page cache",
    Category::LinuxSpecific,
    SafetyLevel::Safe,
    ".cache/man"
);

// --- systemd journal ---

pub struct SystemdJournalRule;

impl CleanupRule for SystemdJournalRule {
    fn name(&self) -> &'static str {
        "systemd journal logs"
    }

    fn category(&self) -> Category {
        Category::LinuxSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let journal_dir = PathBuf::from("/var/log/journal");

        if !journal_dir.exists() || !journal_dir.is_dir() {
            return Vec::new();
        }

        let size = if let Some(result) = crate::scanner::cli_cache::get("journalctl_disk_usage") {
            parse_journal_size(&result.stdout).unwrap_or_else(|| walker::dir_size(&journal_dir))
        } else {
            walker::dir_size(&journal_dir)
        };

        if size == 0 {
            return Vec::new();
        }

        // Use synthetic path so cleaner routes to journalctl --vacuum-size
        vec![ScannedEntry {
            path: PathBuf::from("journal:vacuum"),
            size,
            category: Category::LinuxSpecific,
            safety: SafetyLevel::Caution,
            description: "systemd journal logs".to_owned(),
            item_count: None,
        }]
    }
}

/// Clean systemd journal via journalctl --vacuum-size.
pub fn clean_journal() -> Result<(), std::io::Error> {
    let status = std::process::Command::new("sudo")
        .args(["journalctl", "--vacuum-size=100M"])
        .status()?;

    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("journalctl --vacuum-size failed"))
    }
}

/// Parse "Archived and active journals take up 1.2G in the file system." from journalctl output.
fn parse_journal_size(output: &str) -> Option<u64> {
    // Look for pattern like "take up 1.2G" or "take up 512.0M"
    let take_up_idx = output.find("take up ")?;
    let after = &output[take_up_idx + 8..];
    let end = after.find(|c: char| !c.is_ascii_digit() && c != '.')?;
    let num_str = &after[..end];
    let num: f64 = num_str.parse().ok()?;

    let suffix = after[end..].trim_start().chars().next()?;
    let multiplier = match suffix {
        'B' => 1u64,
        'K' => 1024,
        'M' => 1024 * 1024,
        'G' => 1024 * 1024 * 1024,
        'T' => 1024 * 1024 * 1024 * 1024,
        _ => return None,
    };

    Some((num * multiplier as f64) as u64)
}

// --- Package manager caches ---

pub struct AptCacheRule;

impl CleanupRule for AptCacheRule {
    fn name(&self) -> &'static str {
        "APT package cache"
    }

    fn category(&self) -> Category {
        Category::LinuxSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let cache_dir = PathBuf::from("/var/cache/apt/archives");
        if !cache_dir.exists() || !cache_dir.is_dir() {
            return Vec::new();
        }
        let (size, count) = walker::dir_size_and_count(&cache_dir);
        if size == 0 {
            return Vec::new();
        }
        vec![ScannedEntry {
            path: PathBuf::from("apt:clean"),
            size,
            category: Category::LinuxSpecific,
            safety: SafetyLevel::Safe,
            description: format!("APT package cache ({count} packages)"),
            item_count: Some(count),
        }]
    }
}

/// Clean APT cache via `apt clean`.
pub fn clean_apt() -> Result<(), std::io::Error> {
    let status = std::process::Command::new("sudo")
        .args(["apt", "clean"])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("apt clean failed"))
    }
}

pub struct DnfCacheRule;

impl CleanupRule for DnfCacheRule {
    fn name(&self) -> &'static str {
        "DNF package cache"
    }

    fn category(&self) -> Category {
        Category::LinuxSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let cache_dir = PathBuf::from("/var/cache/dnf");
        if !cache_dir.exists() || !cache_dir.is_dir() {
            return Vec::new();
        }
        let (size, count) = walker::dir_size_and_count(&cache_dir);
        if size == 0 {
            return Vec::new();
        }
        vec![ScannedEntry {
            path: PathBuf::from("dnf:clean"),
            size,
            category: Category::LinuxSpecific,
            safety: SafetyLevel::Safe,
            description: format!("DNF package cache ({count} packages)"),
            item_count: Some(count),
        }]
    }
}

/// Clean DNF cache via `dnf clean all`.
pub fn clean_dnf() -> Result<(), std::io::Error> {
    let status = std::process::Command::new("sudo")
        .args(["dnf", "clean", "all"])
        .status()?;
    if status.success() {
        Ok(())
    } else {
        Err(std::io::Error::other("dnf clean failed"))
    }
}

pub struct PacmanCacheRule;

impl CleanupRule for PacmanCacheRule {
    fn name(&self) -> &'static str {
        "Pacman package cache"
    }

    fn category(&self) -> Category {
        Category::LinuxSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let cache_dir = PathBuf::from("/var/cache/pacman/pkg");

        if !cache_dir.exists() || !cache_dir.is_dir() {
            return Vec::new();
        }

        let (size, count) = walker::dir_size_and_count(&cache_dir);
        if size == 0 {
            return Vec::new();
        }

        // Use synthetic path so cleaner routes to paccache
        vec![ScannedEntry {
            path: PathBuf::from("pacman:clean"),
            size,
            category: Category::LinuxSpecific,
            safety: SafetyLevel::Safe,
            description: format!("Pacman package cache ({count} packages)"),
            item_count: Some(count),
        }]
    }
}

/// Clean pacman cache via paccache (keeps last 2 versions).
pub fn clean_pacman() -> Result<(), std::io::Error> {
    // Try paccache first (from pacman-contrib, keeps last 2 versions)
    let status = std::process::Command::new("paccache")
        .args(["-r", "-k", "2"])
        .status();

    match status {
        Ok(s) if s.success() => Ok(()),
        _ => {
            // Fallback: pacman -Sc (clears uninstalled package cache)
            let status = std::process::Command::new("sudo")
                .args(["pacman", "-Sc", "--noconfirm"])
                .status()?;

            if status.success() {
                Ok(())
            } else {
                Err(std::io::Error::other("pacman cache cleanup failed"))
            }
        }
    }
}

pub struct ZyppCacheRule;

impl CleanupRule for ZyppCacheRule {
    fn name(&self) -> &'static str {
        "Zypper package cache"
    }

    fn category(&self) -> Category {
        Category::LinuxSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let cache_dir = PathBuf::from("/var/cache/zypp");
        scan_system_dir(
            &cache_dir,
            "Zypper package cache",
            Category::LinuxSpecific,
            SafetyLevel::Safe,
        )
    }
}

// --- Snap ---

pub struct SnapCacheRule;

impl CleanupRule for SnapCacheRule {
    fn name(&self) -> &'static str {
        "Snap cache and old revisions"
    }

    fn category(&self) -> Category {
        Category::LinuxSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let mut entries = Vec::new();

        // Snap download cache
        let snap_cache = PathBuf::from("/var/lib/snapd/cache");
        if snap_cache.exists() && std::fs::read_dir(&snap_cache).is_ok() {
            let size = walker::dir_size(&snap_cache);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: snap_cache,
                    size,
                    category: Category::LinuxSpecific,
                    safety: SafetyLevel::Safe,
                    description: "Snap download cache".to_owned(),
                    item_count: None,
                });
            }
        }

        // Disabled snap revisions in /snap/*/
        // Each snap keeps old revisions that are disabled but still take space.
        let snap_dir = PathBuf::from("/snap");
        if snap_dir.exists() {
            if let Ok(snaps) = std::fs::read_dir(&snap_dir) {
                for snap_entry in snaps.flatten() {
                    let snap_path = snap_entry.path();
                    if !snap_path.is_dir() {
                        continue;
                    }
                    let snap_name = snap_entry.file_name().to_string_lossy().to_string();
                    if snap_name == "bin" || snap_name.starts_with('.') {
                        continue;
                    }

                    // Find the "current" symlink target revision number
                    let current_link = snap_path.join("current");
                    let current_rev = current_link
                        .read_link()
                        .ok()
                        .and_then(|t| {
                            t.file_name()
                                .map(|f| f.to_string_lossy().to_string())
                        });

                    if let Ok(revisions) = std::fs::read_dir(&snap_path) {
                        for rev_entry in revisions.flatten() {
                            let rev_name = rev_entry.file_name().to_string_lossy().to_string();
                            // Skip non-numeric dirs (like "current" symlink)
                            if !rev_name.chars().all(|c| c.is_ascii_digit()) {
                                continue;
                            }
                            // Skip the current active revision
                            if current_rev.as_deref() == Some(&rev_name) {
                                continue;
                            }

                            let rev_path = rev_entry.path();
                            let size = walker::dir_size(&rev_path);
                            if size > 0 {
                                entries.push(ScannedEntry {
                                    path: rev_path,
                                    size,
                                    category: Category::LinuxSpecific,
                                    safety: SafetyLevel::Caution,
                                    description: format!(
                                        "Snap old revision: {snap_name} rev {rev_name}"
                                    ),
                                    item_count: None,
                                });
                            }
                        }
                    }
                }
            }
        }

        entries
    }
}

// --- Flatpak ---

pub struct FlatpakCacheRule;

impl CleanupRule for FlatpakCacheRule {
    fn name(&self) -> &'static str {
        "Flatpak app caches"
    }

    fn category(&self) -> Category {
        Category::LinuxSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let flatpak_apps = home.join(".var/app");

        if !flatpak_apps.exists() {
            return Vec::new();
        }

        let mut entries = Vec::new();

        if let Ok(apps) = std::fs::read_dir(&flatpak_apps) {
            for app_entry in apps.flatten() {
                let cache_dir = app_entry.path().join("cache");
                if !cache_dir.exists() || !cache_dir.is_dir() {
                    continue;
                }

                let size = walker::dir_size(&cache_dir);
                if size > 0 {
                    let app_name = app_entry.file_name().to_string_lossy().to_string();
                    entries.push(ScannedEntry {
                        path: cache_dir,
                        size,
                        category: Category::LinuxSpecific,
                        safety: SafetyLevel::Safe,
                        description: format!("Flatpak cache: {app_name}"),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

// --- Old kernels ---

pub struct OldKernelsRule;

impl CleanupRule for OldKernelsRule {
    fn name(&self) -> &'static str {
        "Old kernel modules"
    }

    fn category(&self) -> Category {
        Category::LinuxSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let modules_dir = PathBuf::from("/usr/lib/modules");
        if !modules_dir.exists() {
            return Vec::new();
        }

        // Get the running kernel version from CLI cache or fallback
        let running_kernel = crate::scanner::cli_cache::get("uname_r")
            .map(|r| r.stdout.trim().to_owned())
            .unwrap_or_default();

        if running_kernel.is_empty() {
            return Vec::new(); // Can't determine running kernel, skip to be safe
        }

        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&modules_dir) {
            for entry in read_dir.flatten() {
                let name = entry.file_name().to_string_lossy().to_string();
                let path = entry.path();

                if !path.is_dir() || name == running_kernel {
                    continue;
                }

                let size = walker::dir_size(&path);
                if size > 0 {
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::LinuxSpecific,
                        safety: SafetyLevel::Danger,
                        description: format!("Old kernel modules: {name}"),
                        item_count: None,
                    });
                }
            }
        }

        // Also check /boot for old vmlinuz/initramfs files
        let boot_dir = PathBuf::from("/boot");
        if boot_dir.exists() {
            if let Ok(read_dir) = std::fs::read_dir(&boot_dir) {
                for entry in read_dir.flatten() {
                    let name = entry.file_name().to_string_lossy().to_string();
                    let path = entry.path();

                    if !path.is_file() {
                        continue;
                    }

                    // Match vmlinuz-*, initramfs-*, initrd.img-*, System.map-*, config-*
                    let is_kernel_file = name.starts_with("vmlinuz-")
                        || name.starts_with("initramfs-")
                        || name.starts_with("initrd.img-")
                        || name.starts_with("System.map-")
                        || name.starts_with("config-");

                    if !is_kernel_file {
                        continue;
                    }

                    // Check if this file belongs to the running kernel
                    if name.contains(&running_kernel) {
                        continue;
                    }

                    if let Ok(meta) = path.metadata() {
                        let size = meta.len();
                        if size > 0 {
                            entries.push(ScannedEntry {
                                path,
                                size,
                                category: Category::LinuxSpecific,
                                safety: SafetyLevel::Danger,
                                description: format!("Old kernel file: {name}"),
                                item_count: None,
                            });
                        }
                    }
                }
            }
        }

        entries
    }
}

// --- Linux installer files in Downloads ---

pub struct LinuxInstallerRule;

impl CleanupRule for LinuxInstallerRule {
    fn name(&self) -> &'static str {
        "Linux installer files"
    }

    fn category(&self) -> Category {
        Category::LinuxSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let downloads_dir = home.join("Downloads");

        if !downloads_dir.exists() {
            return Vec::new();
        }

        let extensions = [".deb", ".rpm", ".appimage", ".flatpakref", ".snap"];
        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&downloads_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let name = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_lowercase();
                if extensions.iter().any(|ext| name.ends_with(ext)) {
                    if let Ok(meta) = path.metadata() {
                        let size = meta.len();
                        if size > 0 {
                            let file_name = path
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .to_string();
                            entries.push(ScannedEntry {
                                path,
                                size,
                                category: Category::LinuxSpecific,
                                safety: SafetyLevel::Caution,
                                description: format!("Installer: {file_name}"),
                                item_count: None,
                            });
                        }
                    }
                }
            }
        }

        entries
    }
}

// --- Coredumpctl ---

pub struct SystemdCoredumpRule;

impl CleanupRule for SystemdCoredumpRule {
    fn name(&self) -> &'static str {
        "systemd coredumps"
    }

    fn category(&self) -> Category {
        Category::LinuxSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let coredump_dir = PathBuf::from("/var/lib/systemd/coredump");

        if !coredump_dir.exists() || std::fs::read_dir(&coredump_dir).is_err() {
            return Vec::new();
        }

        let (size, count) = walker::dir_size_and_count(&coredump_dir);
        if size == 0 {
            return Vec::new();
        }

        vec![ScannedEntry {
            path: coredump_dir,
            size,
            category: Category::LinuxSpecific,
            safety: SafetyLevel::Safe,
            description: format!("systemd coredumps ({count} files)"),
            item_count: Some(count),
        }]
    }
}

// --- Helper ---

fn scan_system_dir(
    dir: &PathBuf,
    description: &str,
    category: Category,
    safety: SafetyLevel,
) -> Vec<ScannedEntry> {
    if !dir.exists() || !dir.is_dir() {
        return Vec::new();
    }

    // Check if we can actually read the directory
    if std::fs::read_dir(dir).is_err() {
        return Vec::new();
    }

    let (size, count) = walker::dir_size_and_count(dir);
    if size == 0 {
        return Vec::new();
    }

    vec![ScannedEntry {
        path: dir.clone(),
        size,
        category,
        safety,
        description: format!("{description} ({count} items)"),
        item_count: Some(count),
    }]
}

// --- Steam on Linux ---

cache_rule!(
    SteamShaderCacheRule,
    "Steam shader cache",
    Category::LinuxSpecific,
    SafetyLevel::Safe,
    ".local/share/Steam/steamapps/shadercache"
);

pub struct SteamCompatDataRule;

impl CleanupRule for SteamCompatDataRule {
    fn name(&self) -> &'static str {
        "Steam Proton compatdata"
    }

    fn category(&self) -> Category {
        Category::LinuxSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let compat_dir = home.join(".local/share/Steam/steamapps/compatdata");

        if !compat_dir.exists() {
            return Vec::new();
        }

        let mut entries = Vec::new();

        if let Ok(read_dir) = std::fs::read_dir(&compat_dir) {
            for entry in read_dir.flatten() {
                let path = entry.path();
                if !path.is_dir() {
                    continue;
                }

                let name = entry.file_name().to_string_lossy().to_string();
                let size = walker::dir_size(&path);
                if size > 50_000_000 {
                    // Only report prefixes > 50 MiB
                    entries.push(ScannedEntry {
                        path,
                        size,
                        category: Category::LinuxSpecific,
                        safety: SafetyLevel::Caution,
                        description: format!("Proton prefix: app {name}"),
                        item_count: None,
                    });
                }
            }
        }

        entries
    }
}

// --- AUR helper caches ---

cache_rule!(
    ParuCacheRule,
    "paru AUR cache",
    Category::LinuxSpecific,
    SafetyLevel::Safe,
    ".cache/paru"
);

cache_rule!(
    YayCacheRule,
    "yay AUR cache",
    Category::LinuxSpecific,
    SafetyLevel::Safe,
    ".cache/yay"
);

// --- Playwright/Electron cached browsers ---

cache_rule!(
    PlaywrightCacheRule,
    "Playwright browsers",
    Category::LinuxSpecific,
    SafetyLevel::Safe,
    ".cache/ms-playwright"
);

cache_rule!(
    ElectronCacheRule,
    "Electron framework cache",
    Category::LinuxSpecific,
    SafetyLevel::Safe,
    ".cache/electron"
);

// --- GPU shader caches ---

cache_rule!(
    NvidiaCacheRule,
    "NVIDIA shader cache",
    Category::LinuxSpecific,
    SafetyLevel::Safe,
    ".cache/nvidia",
    ".nv/ComputeCache"
);

cache_rule!(
    RadeonShaderRule,
    "AMD RADV shader cache",
    Category::LinuxSpecific,
    SafetyLevel::Safe,
    ".cache/radv_builtin_shaders"
);

// --- Wine ---

pub struct WinePrefixRule;

impl CleanupRule for WinePrefixRule {
    fn name(&self) -> &'static str {
        "Wine prefixes"
    }

    fn category(&self) -> Category {
        Category::LinuxSpecific
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();
        let mut entries = Vec::new();

        // Default wine prefix
        let wine_dir = home.join(".wine");
        if wine_dir.exists() {
            let size = walker::dir_size(&wine_dir);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: wine_dir,
                    size,
                    category: Category::LinuxSpecific,
                    safety: SafetyLevel::Caution,
                    description: "Wine default prefix".to_owned(),
                    item_count: None,
                });
            }
        }

        // Bottles (Flatpak wine manager)
        let bottles_dir = home.join(".local/share/bottles");
        if bottles_dir.exists() {
            let size = walker::dir_size(&bottles_dir);
            if size > 0 {
                let (_, count) = walker::dir_size_and_count(&bottles_dir);
                entries.push(ScannedEntry {
                    path: bottles_dir,
                    size,
                    category: Category::LinuxSpecific,
                    safety: SafetyLevel::Caution,
                    description: format!("Bottles wine prefixes ({count} items)"),
                    item_count: Some(count),
                });
            }
        }

        // Lutris
        let lutris_dir = home.join(".local/share/lutris");
        if lutris_dir.exists() {
            let size = walker::dir_size(&lutris_dir);
            if size > 0 {
                entries.push(ScannedEntry {
                    path: lutris_dir,
                    size,
                    category: Category::LinuxSpecific,
                    safety: SafetyLevel::Caution,
                    description: "Lutris gaming data".to_owned(),
                    item_count: None,
                });
            }
        }

        entries
    }
}

// --- Registration ---

pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(SystemdJournalRule),
        Box::new(AptCacheRule),
        Box::new(DnfCacheRule),
        Box::new(PacmanCacheRule),
        Box::new(ZyppCacheRule),
        Box::new(SnapCacheRule),
        Box::new(FlatpakCacheRule),
        Box::new(XdgThumbnailRule),
        Box::new(XdgFontCacheRule),
        Box::new(MesaShaderRule),
        Box::new(ManCacheRule),
        Box::new(OldKernelsRule),
        Box::new(LinuxInstallerRule),
        Box::new(SystemdCoredumpRule),
        Box::new(SteamShaderCacheRule),
        Box::new(SteamCompatDataRule),
        Box::new(ParuCacheRule),
        Box::new(YayCacheRule),
        Box::new(PlaywrightCacheRule),
        Box::new(ElectronCacheRule),
        Box::new(NvidiaCacheRule),
        Box::new(RadeonShaderRule),
        Box::new(WinePrefixRule),
    ]
}
