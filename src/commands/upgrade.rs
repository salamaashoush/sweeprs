use anyhow::{Context, Result, bail};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

const REPO: &str = "salamaashoush/sweeprs";
const BINARY_NAME: &str = "sweeprs";

/// Execute the upgrade command.
pub fn execute() -> Result<()> {
    eprintln!("sweeprs self-upgrade");
    eprintln!();

    let current_binary = get_current_binary_path()?;
    eprintln!("  Current binary: {}", current_binary.display());

    let current_version = env!("CARGO_PKG_VERSION");
    eprintln!("  Current version: {current_version}");

    eprintln!("  Checking for updates...");
    let latest_tag = get_latest_tag()?;
    let latest_version = latest_tag.trim_start_matches('v');
    eprintln!("  Latest version: {latest_version}");
    eprintln!();

    if current_version == latest_version {
        eprintln!("Already up to date!");
        return Ok(());
    }

    eprintln!("Upgrading {current_version} -> {latest_version}");

    let (temp_dir, binary_path) = download_release(&latest_tag)?;
    eprintln!("  Download complete");

    replace_binary(&current_binary, &binary_path)?;
    eprintln!("  Binary replaced");

    // Clean up temp dir (best effort)
    drop(temp_dir);

    eprintln!();
    eprintln!("Successfully upgraded to {latest_version}");

    Ok(())
}

/// Resolve the path to the currently running binary (follows symlinks).
fn get_current_binary_path() -> Result<PathBuf> {
    let exe = env::current_exe().context("failed to get current executable path")?;
    let canonical = fs::canonicalize(&exe).unwrap_or(exe);
    Ok(canonical)
}

/// Fetch the latest release tag from the public GitHub API.
fn get_latest_tag() -> Result<String> {
    let url = format!("https://api.github.com/repos/{REPO}/releases/latest");

    let output = Command::new("curl")
        .args(["-sfL", "--max-time", "15", &url])
        .output()
        .context("failed to run curl -- is it installed?")?;

    if !output.status.success() {
        bail!(
            "failed to fetch latest release from GitHub: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let body = String::from_utf8_lossy(&output.stdout);
    // Parse tag_name from JSON without pulling in a full JSON parser dependency.
    // The response is well-formed from GitHub so a simple extract is fine.
    let tag = extract_json_string(&body, "tag_name")
        .context("could not find tag_name in GitHub release response")?;

    if tag.is_empty() {
        bail!("no releases found for {REPO}");
    }

    Ok(tag)
}

/// Minimal JSON string field extractor -- avoids needing `serde_json` just for this.
/// Looks for `"key": "value"` and returns `value`.
fn extract_json_string(json: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\"");
    let idx = json.find(&needle)?;
    let rest = &json[idx + needle.len()..];
    // skip whitespace and colon
    let rest = rest.trim_start();
    let rest = rest.strip_prefix(':')?;
    let rest = rest.trim_start();
    let rest = rest.strip_prefix('"')?;
    let end = rest.find('"')?;
    Some(rest[..end].to_string())
}

/// Detect the current platform target triple fragment.
fn detect_target() -> Result<String> {
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else {
        bail!("unsupported architecture");
    };

    let os = if cfg!(target_os = "macos") {
        "apple-darwin"
    } else if cfg!(target_os = "linux") {
        "unknown-linux-gnu"
    } else {
        bail!("unsupported operating system");
    };

    Ok(format!("{arch}-{os}"))
}

/// Download the release tarball and extract the binary.
/// Returns the temp directory (keep alive until done) and path to extracted binary.
fn download_release(tag: &str) -> Result<(tempfile::TempDir, PathBuf)> {
    let target = detect_target()?;
    let filename = format!("{BINARY_NAME}-{tag}-{target}.tar.gz");
    let url = format!("https://github.com/{REPO}/releases/download/{tag}/{filename}");

    let temp_dir =
        tempfile::tempdir().context("failed to create temp directory")?;

    let archive_path = temp_dir.path().join(&filename);

    // Download the tarball
    let output = Command::new("curl")
        .args([
            "-fSL",
            "--max-time",
            "120",
            "-o",
            archive_path.to_str().expect("temp path should be valid UTF-8"),
            &url,
        ])
        .output()
        .context("failed to run curl for download")?;

    if !output.status.success() {
        bail!(
            "download failed ({}): {}",
            url,
            String::from_utf8_lossy(&output.stderr)
        );
    }

    // Extract the tarball
    let extract_dir = temp_dir.path().join("extracted");
    fs::create_dir_all(&extract_dir).context("failed to create extraction directory")?;

    let output = Command::new("tar")
        .args([
            "-xzf",
            archive_path.to_str().expect("archive path should be valid UTF-8"),
            "-C",
            extract_dir.to_str().expect("extract dir should be valid UTF-8"),
        ])
        .output()
        .context("failed to run tar")?;

    if !output.status.success() {
        bail!(
            "extraction failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let binary_path = extract_dir.join(BINARY_NAME);
    if !binary_path.exists() {
        bail!(
            "binary not found after extraction: {}",
            binary_path.display()
        );
    }

    // Make executable
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&binary_path)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&binary_path, perms)?;
    }

    Ok((temp_dir, binary_path))
}

/// Replace the current binary with the new one, with backup/restore on failure.
fn replace_binary(current: &Path, new: &Path) -> Result<()> {
    let backup = current.with_extension("backup");

    // Remove stale backup
    if backup.exists() {
        fs::remove_file(&backup).ok();
    }

    // Create backup
    fs::copy(current, &backup).context("failed to back up current binary")?;

    // Attempt replacement
    let result = fs::copy(new, current).context("failed to replace binary");

    // Set permissions
    #[cfg(unix)]
    if result.is_ok() {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(current)?.permissions();
        perms.set_mode(0o755);
        fs::set_permissions(current, perms)?;
    }

    match result {
        Ok(_) => {
            fs::remove_file(&backup).ok();

            #[cfg(target_os = "macos")]
            fix_macos_security(current);

            Ok(())
        }
        Err(e) => {
            // Restore from backup
            if backup.exists() {
                fs::copy(&backup, current).ok();
                fs::remove_file(&backup).ok();
            }
            Err(e)
        }
    }
}

/// Remove quarantine, add provenance marker, and ad-hoc codesign on macOS.
#[cfg(target_os = "macos")]
fn fix_macos_security(binary: &Path) {
    let path_str = binary.to_str().expect("binary path should be valid UTF-8");

    // Remove quarantine attribute
    Command::new("xattr")
        .args(["-d", "com.apple.quarantine", path_str])
        .output()
        .ok();

    // Add provenance attribute (same as Homebrew -- marks as locally built)
    let provenance_data = b"\x00\x02\x0a";
    Command::new("xattr")
        .args([
            "-w",
            "com.apple.provenance",
            &String::from_utf8_lossy(provenance_data),
            path_str,
        ])
        .output()
        .ok();

    // Ad-hoc codesign
    Command::new("codesign")
        .args(["--force", "--deep", "--sign", "-", path_str])
        .output()
        .ok();
}
