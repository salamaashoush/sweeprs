use std::path::{Path, PathBuf};

use crate::config::Config;
use crate::rules::CleanupRule;
use crate::scanner::cli_cache;
use crate::scanner::entry::{Category, SafetyLevel, ScannedEntry};
use crate::scanner::walker;

/// Runtime disk images live under the system-wide `CoreSimulator` root, not the
/// per-user one.
const CORE_SIMULATOR_ROOT: &str = "/Library/Developer/CoreSimulator";

/// Devices below this size aren't worth listing individually; the runtimes
/// they belong to dominate the total by orders of magnitude.
const MIN_DEVICE_SIZE: u64 = 10 * 1_048_576;

pub struct SimulatorRuntimeRule;
pub struct SimulatorDeviceRule;
pub struct AndroidAvdRule;
pub struct AndroidSystemImageRule;

/// A runtime as reported by `simctl runtime list`.
struct Runtime {
    name: String,
    build: String,
    identifier: String,
}

/// Parse the human-readable output of `xcrun simctl runtime list`.
///
/// `simctl runtime list -j` returns an empty object on current Xcode, so the
/// text form is the only source. Lines of interest look like:
///
/// ```text
/// iOS 26.0 (23A343) - 1F9185C3-8A4F-4A05-AC2D-1F11C4431CD0 (Ready)
/// ```
fn parse_runtime_list(stdout: &str) -> Vec<Runtime> {
    let mut runtimes = Vec::new();

    for line in stdout.lines() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with("==")
            || line.starts_with("--")
            || line.starts_with("Total")
        {
            continue;
        }

        let Some((left, right)) = line.split_once(" - ") else {
            continue;
        };

        let Some((name, build)) = left.rsplit_once(" (") else {
            continue;
        };
        let Some(build) = build.strip_suffix(')') else {
            continue;
        };

        let identifier = right
            .split_whitespace()
            .next()
            .unwrap_or_default()
            .to_owned();
        if identifier.is_empty() {
            continue;
        }

        runtimes.push(Runtime {
            name: name.trim().to_owned(),
            build: build.trim().to_owned(),
            identifier,
        });
    }

    runtimes
}

/// A runtime occupies disk exactly once, as the `.dmg` it was installed from.
///
/// `Volumes/<platform>_<build>` looks like a second copy but is only that dmg's
/// mount point, so walking it would double-count every runtime -- and cost a
/// recursive walk of tens of thousands of read-only files on every scan.
/// Deleting the runtime reclaims the dmg's bytes and nothing more.
fn runtime_size(runtime: &Runtime) -> u64 {
    walker::file_size(
        &Path::new(CORE_SIMULATOR_ROOT)
            .join("Images")
            .join(format!("{}.dmg", runtime.identifier)),
    )
}

impl CleanupRule for SimulatorRuntimeRule {
    fn name(&self) -> &'static str {
        "Simulator runtimes"
    }

    fn category(&self) -> Category {
        Category::Simulator
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let Some(result) = cli_cache::get_raw("simctl_runtime_list") else {
            return Vec::new();
        };
        if !result.success {
            return Vec::new();
        }

        parse_runtime_list(&result.stdout)
            .into_iter()
            .filter_map(|runtime| {
                let size = runtime_size(&runtime);
                if size == 0 {
                    return None;
                }
                let description = format!(
                    "{} ({}) runtime -- re-downloading costs ~{}",
                    runtime.name,
                    runtime.build,
                    crate::util::human_size(size)
                );
                Some(ScannedEntry {
                    path: PathBuf::from(format!("simctl-runtime:{}", runtime.identifier)),
                    size,
                    category: Category::Simulator,
                    safety: SafetyLevel::Caution,
                    description,
                    item_count: None,
                })
            })
            .collect()
    }
}

/// One device, as it appears in `simctl list devices -j`.
struct Device {
    udid: String,
    name: String,
    size: u64,
    available: bool,
    booted: bool,
}

fn parse_device_list(stdout: &str) -> Vec<(String, Vec<Device>)> {
    let Ok(json) = serde_json::from_str::<serde_json::Value>(stdout) else {
        return Vec::new();
    };
    let Some(runtimes) = json.get("devices").and_then(serde_json::Value::as_object) else {
        return Vec::new();
    };

    let mut out = Vec::new();

    for (runtime_id, devices) in runtimes {
        let Some(devices) = devices.as_array() else {
            continue;
        };

        let parsed = devices
            .iter()
            .filter_map(|device| {
                let udid = device.get("udid").and_then(serde_json::Value::as_str)?;
                Some(Device {
                    udid: udid.to_owned(),
                    name: device
                        .get("name")
                        .and_then(serde_json::Value::as_str)
                        .unwrap_or("Unknown")
                        .to_owned(),
                    size: device
                        .get("dataPathSize")
                        .and_then(serde_json::Value::as_u64)
                        .unwrap_or(0),
                    available: device
                        .get("isAvailable")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(true),
                    booted: device.get("state").and_then(serde_json::Value::as_str)
                        == Some("Booted"),
                })
            })
            .collect();

        out.push((runtime_id.clone(), parsed));
    }

    out
}

/// `simctl` is the only safe way to remove a device -- deleting the directories
/// behind its back leaves `CoreSimulator`'s registry pointing at nothing. When it
/// cannot be queried we therefore have nothing to offer, but staying silent
/// would read as "no simulators to reclaim" on a machine holding tens of GB of
/// them. Report the shortfall instead, as an entry cleaning skips.
fn simctl_unavailable() -> Vec<ScannedEntry> {
    let devices = dirs::home_dir()
        .unwrap_or_default()
        .join("Library/Developer/CoreSimulator/Devices");

    let size = walker::dir_size(&devices);
    if size == 0 {
        return Vec::new();
    }

    vec![ScannedEntry {
        path: devices,
        size: 0,
        category: Category::Simulator,
        safety: SafetyLevel::Error,
        description: format!(
            "simctl could not be queried -- {} of simulator devices left unexamined",
            crate::util::human_size(size)
        ),
        item_count: None,
    }]
}

impl CleanupRule for SimulatorDeviceRule {
    fn name(&self) -> &'static str {
        "Simulator devices"
    }

    fn category(&self) -> Category {
        Category::Simulator
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let Some(result) = cli_cache::get_raw("simctl_devices_json") else {
            return simctl_unavailable();
        };
        if !result.success {
            return simctl_unavailable();
        }

        let mut entries = Vec::new();
        let mut stale: Vec<Device> = Vec::new();

        for (runtime_id, devices) in parse_device_list(&result.stdout) {
            for device in devices {
                // A booted device is in use, and `simctl delete` refuses it anyway.
                if device.booted {
                    continue;
                }

                if !device.available {
                    stale.push(device);
                    continue;
                }

                if device.size < MIN_DEVICE_SIZE {
                    continue;
                }

                entries.push(ScannedEntry {
                    path: PathBuf::from(format!("simctl-device:{}", device.udid)),
                    size: device.size,
                    category: Category::Simulator,
                    safety: SafetyLevel::Caution,
                    description: format!(
                        "Simulator: {} ({})",
                        device.name,
                        short_runtime(&runtime_id)
                    ),
                    item_count: None,
                });
            }
        }

        // Devices whose runtime is gone can never boot again -- always safe.
        //
        // The UDIDs are pinned here rather than cleaned with `simctl delete
        // unavailable`, which resolves its victims when it runs: deleting a
        // runtime earlier in the same batch would make that runtime's devices
        // unavailable too, and the sweep would take them along unasked.
        if !stale.is_empty() {
            let udids: Vec<&str> = stale.iter().map(|d| d.udid.as_str()).collect();
            entries.push(ScannedEntry {
                path: PathBuf::from(format!("simctl-device:{}", udids.join(","))),
                size: stale.iter().map(|d| d.size).sum(),
                category: Category::Simulator,
                safety: SafetyLevel::Safe,
                description: format!(
                    "{} unavailable simulator device(s) -- runtime no longer installed",
                    stale.len()
                ),
                item_count: Some(stale.len()),
            });
        }

        entries
    }
}

/// `com.apple.CoreSimulator.SimRuntime.iOS-17-5` -> `iOS 17.5`
fn short_runtime(identifier: &str) -> String {
    identifier
        .rsplit_once('.')
        .map_or(identifier, |(_, tail)| tail)
        .replacen('-', " ", 1)
        .replace('-', ".")
}

/// System images are laid out as `system-images/<api>/<vendor>/<abi>`.
const SYSTEM_IMAGE_DEPTH: usize = 3;

/// Collect the directories exactly `depth` levels below `root`.
fn leaf_dirs(root: &Path, depth: usize) -> Vec<PathBuf> {
    let mut level = vec![root.to_path_buf()];

    for _ in 0..depth {
        let mut next = Vec::new();
        for dir in &level {
            let Ok(children) = std::fs::read_dir(dir) else {
                continue;
            };
            next.extend(children.flatten().map(|c| c.path()).filter(|p| p.is_dir()));
        }
        level = next;
    }

    level
}

fn avd_dir() -> PathBuf {
    if let Ok(dir) = std::env::var("ANDROID_AVD_HOME") {
        return PathBuf::from(dir);
    }
    if let Ok(dir) = std::env::var("ANDROID_SDK_HOME") {
        return PathBuf::from(dir).join(".android/avd");
    }
    dirs::home_dir().unwrap_or_default().join(".android/avd")
}

impl CleanupRule for AndroidAvdRule {
    fn name(&self) -> &'static str {
        "Android AVDs"
    }

    fn category(&self) -> Category {
        Category::Simulator
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        scan_avd_dir(&avd_dir())
    }
}

fn scan_avd_dir(dir: &Path) -> Vec<ScannedEntry> {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return Vec::new();
    };

    let mut entries = Vec::new();

    for entry in read_dir.flatten() {
        let path = entry.path();
        if !path.is_dir() || path.extension().is_none_or(|ext| ext != "avd") {
            continue;
        }

        // An AVD is the `.avd` payload plus a sibling `.ini` registering it.
        // Both go together on delete, so both count toward the size.
        let size = walker::dir_size(&path)
            + avd_ini(&path).map_or(0, |i| {
                use std::os::unix::fs::MetadataExt;
                i.blocks() * 512
            });
        if size == 0 {
            continue;
        }

        let name = path.file_stem().unwrap_or_default().to_string_lossy();

        entries.push(ScannedEntry {
            path: path.clone(),
            size,
            category: Category::Simulator,
            safety: SafetyLevel::Caution,
            description: format!("Android AVD: {name}"),
            item_count: None,
        });
    }

    entries
}

/// The `<name>.ini` that sits beside `<name>.avd` and registers it with the
/// emulator, if it exists.
fn avd_ini(avd_path: &Path) -> Option<std::fs::Metadata> {
    let name = avd_path.file_stem()?;
    let ini = avd_path.with_file_name(format!("{}.ini", name.to_string_lossy()));
    ini.metadata().ok()
}

/// Remove the `.ini` registering an AVD that has just been deleted.
///
/// The emulator enumerates AVDs from these files, so an `.ini` left behind
/// makes `emulator -list-avds` offer a device that can no longer launch.
pub fn remove_avd_ini(avd_path: &Path) {
    let Some(name) = avd_path.file_stem() else {
        return;
    };
    let ini = avd_path.with_file_name(format!("{}.ini", name.to_string_lossy()));
    if ini.is_file() {
        let _ = std::fs::remove_file(ini);
    }
}

/// Does this path name an Android AVD payload directory?
pub fn is_avd_payload(path: &Path) -> bool {
    path.extension().is_some_and(|ext| ext == "avd")
}

impl CleanupRule for AndroidSystemImageRule {
    fn name(&self) -> &'static str {
        "Android system images"
    }

    fn category(&self) -> Category {
        Category::Simulator
    }

    fn scan(&self, _config: &Config) -> Vec<ScannedEntry> {
        let home = dirs::home_dir().unwrap_or_default();

        let roots = [
            home.join("Library/Android/sdk/system-images"),
            home.join("Android/Sdk/system-images"),
        ];

        let mut entries = Vec::new();

        for root in roots {
            if !root.exists() {
                continue;
            }

            // Reporting whole API levels would hide which ABI actually costs
            // the space, so descend to the leaves.
            for image in leaf_dirs(&root, SYSTEM_IMAGE_DEPTH) {
                let size = walker::dir_size(&image);
                if size == 0 {
                    continue;
                }

                let label = image
                    .strip_prefix(&root)
                    .unwrap_or(&image)
                    .components()
                    .map(|c| c.as_os_str().to_string_lossy())
                    .collect::<Vec<_>>()
                    .join(" ");

                entries.push(ScannedEntry {
                    path: image,
                    size,
                    category: Category::Simulator,
                    safety: SafetyLevel::Caution,
                    description: format!("Android system image: {label}"),
                    item_count: None,
                });
            }
        }

        entries
    }
}

fn simctl(args: &[&str]) -> Result<(), std::io::Error> {
    let output = std::process::Command::new("xcrun")
        .arg("simctl")
        .args(args)
        .output()?;

    if output.status.success() {
        return Ok(());
    }

    Err(std::io::Error::other(format!(
        "simctl {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    )))
}

/// Clean a virtual simulator entry.
///
/// iOS runtimes and devices are owned by the `CoreSimulator` service: removing
/// their directories directly leaves the service's registry pointing at devices
/// that no longer exist, and leaves runtime volumes mounted. `simctl` unmounts
/// and deregisters them properly, and needs no elevated privileges.
pub fn clean_simulator_entry(entry_path: &str) -> Result<(), std::io::Error> {
    if let Some(identifier) = entry_path.strip_prefix("simctl-runtime:") {
        return simctl(&["runtime", "delete", identifier]);
    }

    if let Some(udids) = entry_path.strip_prefix("simctl-device:") {
        let mut args = vec!["delete"];
        args.extend(udids.split(',').filter(|u| !u.is_empty()));
        if args.len() == 1 {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidInput,
                "no device UDIDs to delete",
            ));
        }
        return simctl(&args);
    }

    Err(std::io::Error::new(
        std::io::ErrorKind::InvalidInput,
        format!("unknown simulator entry: {entry_path}"),
    ))
}

// CoreSimulator is macOS-only; Android tooling runs on both platforms.
#[cfg(target_os = "macos")]
pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![
        Box::new(SimulatorRuntimeRule),
        Box::new(SimulatorDeviceRule),
        Box::new(AndroidAvdRule),
        Box::new(AndroidSystemImageRule),
    ]
}

#[cfg(not(target_os = "macos"))]
pub fn rules() -> Vec<Box<dyn CleanupRule>> {
    vec![Box::new(AndroidAvdRule), Box::new(AndroidSystemImageRule)]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_runtime_list() {
        let stdout = "== Disk Images ==\n\
                      -- iOS --\n\
                      iOS 17.5 (21F79) - 51E4E540-BA87-460E-A97A-E00CCDA2C70D (Ready)\n\
                      iOS 26.0 (23A343) - 1F9185C3-8A4F-4A05-AC2D-1F11C4431CD0 (Ready)\n\
                      \n\
                      Total Disk Images: 2 (14.0G)\n";

        let runtimes = parse_runtime_list(stdout);

        assert_eq!(runtimes.len(), 2);
        assert_eq!(runtimes[0].name, "iOS 17.5");
        assert_eq!(runtimes[0].build, "21F79");
        assert_eq!(
            runtimes[0].identifier,
            "51E4E540-BA87-460E-A97A-E00CCDA2C70D"
        );
        assert_eq!(runtimes[1].build, "23A343");
    }

    #[test]
    fn ignores_headers_and_totals() {
        assert!(
            parse_runtime_list("== Disk Images ==\n-- iOS --\nTotal Disk Images: 0 (0B)\n")
                .is_empty()
        );
    }

    #[test]
    fn shortens_runtime_identifier() {
        assert_eq!(
            short_runtime("com.apple.CoreSimulator.SimRuntime.iOS-17-5"),
            "iOS 17.5"
        );
        assert_eq!(
            short_runtime("com.apple.CoreSimulator.SimRuntime.iOS-26-0"),
            "iOS 26.0"
        );
    }

    #[test]
    fn booted_devices_are_never_offered() {
        let stdout = r#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-17-5":[
            {"udid":"AAAA","name":"iPhone 15","state":"Booted","isAvailable":true,"dataPathSize":900000000}
        ]}}"#;

        let parsed = parse_device_list(stdout);
        assert!(parsed[0].1[0].booted);
    }

    #[test]
    fn unavailable_devices_are_pinned_by_udid_not_swept_by_keyword() {
        let stdout = r#"{"devices":{"com.apple.CoreSimulator.SimRuntime.iOS-17-5":[
            {"udid":"AAAA","name":"iPhone 15","state":"Shutdown","isAvailable":false,"dataPathSize":1000},
            {"udid":"BBBB","name":"iPhone 16","state":"Shutdown","isAvailable":false,"dataPathSize":2000}
        ]}}"#;

        let devices = parse_device_list(stdout);
        let stale: Vec<&Device> = devices[0].1.iter().filter(|d| !d.available).collect();
        let path = format!(
            "simctl-device:{}",
            stale
                .iter()
                .map(|d| d.udid.as_str())
                .collect::<Vec<_>>()
                .join(",")
        );

        // A literal `unavailable` keyword would resolve at clean time and could
        // take devices a runtime deletion had just orphaned.
        assert_eq!(path, "simctl-device:AAAA,BBBB");
        assert!(!path.contains("unavailable"));
    }

    #[test]
    fn avds_keep_their_real_path_so_excludes_and_archiving_still_apply() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();

        std::fs::create_dir(dir.join("Pixel_7.avd")).expect("avd dir");
        std::fs::write(dir.join("Pixel_7.avd/userdata.img"), vec![0u8; 2048]).expect("payload");
        std::fs::write(dir.join("Pixel_7.ini"), "path=/somewhere\n").expect("ini");

        // Not an AVD: must be ignored rather than reported.
        std::fs::write(dir.join("README.txt"), "x").expect("stray file");

        let entries = scan_avd_dir(dir);

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path, dir.join("Pixel_7.avd"));
        // Payload plus the .ini, not the payload alone.
        assert!(entries[0].size > 2048);
    }

    #[test]
    fn avd_dir_without_avds_yields_nothing() {
        let tmp = tempfile::tempdir().expect("tempdir");
        assert!(scan_avd_dir(tmp.path()).is_empty());
        assert!(scan_avd_dir(&tmp.path().join("missing")).is_empty());
    }

    #[test]
    fn removing_an_avd_takes_the_ini_with_it() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let dir = tmp.path();

        std::fs::create_dir(dir.join("Pixel_7.avd")).expect("avd dir");
        std::fs::write(dir.join("Pixel_7.ini"), "path=/somewhere\n").expect("ini");
        std::fs::write(dir.join("Pixel_9.ini"), "path=/elsewhere\n").expect("other ini");

        remove_avd_ini(&dir.join("Pixel_7.avd"));

        // Leaving the .ini behind would make the emulator list a broken AVD.
        assert!(!dir.join("Pixel_7.ini").exists());
        assert!(dir.join("Pixel_9.ini").exists());
    }

    #[test]
    fn avd_payloads_are_recognised_by_extension() {
        assert!(is_avd_payload(Path::new("/x/.android/avd/Pixel_7.avd")));
        assert!(!is_avd_payload(Path::new("/x/Library/Caches/foo")));
    }

    #[test]
    fn device_entry_without_udids_is_rejected_rather_than_deleting_everything() {
        assert!(clean_simulator_entry("simctl-device:").is_err());
    }

    #[test]
    fn unknown_virtual_entry_is_rejected() {
        assert!(clean_simulator_entry("docker:Images").is_err());
    }

    #[test]
    fn leaf_dirs_descends_exactly_the_requested_depth() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path();

        // system-images/<api>/<vendor>/<abi>
        std::fs::create_dir_all(root.join("android-34/google_apis/arm64-v8a")).expect("image a");
        std::fs::create_dir_all(root.join("android-34/google_apis/x86_64")).expect("image b");
        std::fs::create_dir_all(root.join("android-33/default/arm64-v8a")).expect("image c");

        let leaves = leaf_dirs(root, SYSTEM_IMAGE_DEPTH);

        assert_eq!(leaves.len(), 3);
        assert!(leaves.iter().all(|p| p.starts_with(root)));
        // Depth is exact: the vendor level must not be reported as a leaf.
        assert!(!leaves.contains(&root.join("android-34/google_apis")));
    }
}
