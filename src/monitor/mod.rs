pub mod daemon;
pub mod notify;

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, anyhow};

use crate::config::Config;

#[cfg(target_os = "macos")]
const LAUNCHD_LABEL: &str = "com.sweeprs.monitor";

#[cfg(target_os = "linux")]
const SYSTEMD_UNIT: &str = "sweeprs-monitor.service";

#[cfg(target_os = "macos")]
fn plist_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Library/LaunchAgents")
        .join(format!("{LAUNCHD_LABEL}.plist"))
}

#[cfg(target_os = "linux")]
fn systemd_unit_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join(".config/systemd/user")
        .join(SYSTEMD_UNIT)
}

pub fn start() -> Result<()> {
    let pid_path = daemon::pid_file_path();

    if pid_path.exists() {
        let existing_pid = std::fs::read_to_string(&pid_path)?;
        let existing_pid = existing_pid.trim();
        if is_process_running(existing_pid) {
            println!("Monitor already running (PID: {existing_pid})");
            return Ok(());
        }
        std::fs::remove_file(&pid_path)?;
    }

    let exe = std::env::current_exe()?;
    let child = Command::new(exe)
        .args(["monitor", "--foreground"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()?;

    let pid = child.id();
    std::fs::write(&pid_path, pid.to_string())?;
    println!("Monitor started (PID: {pid})");
    println!("PID file: {}", pid_path.display());

    Ok(())
}

pub fn stop() -> Result<()> {
    let pid_path = daemon::pid_file_path();

    if !pid_path.exists() {
        println!("No monitor running.");
        return Ok(());
    }

    let pid = std::fs::read_to_string(&pid_path)?.trim().to_owned();

    if is_process_running(&pid) {
        Command::new("kill")
            .arg(&pid)
            .status()
            .map_err(|e| anyhow!("Failed to kill process {pid}: {e}"))?;
        println!("Monitor stopped (PID: {pid})");
    } else {
        println!("Monitor was not running (stale PID file).");
    }

    std::fs::remove_file(&pid_path)?;
    Ok(())
}

pub fn status() -> Result<()> {
    let pid_path = daemon::pid_file_path();

    #[cfg(target_os = "macos")]
    {
        let launchd_installed = plist_path().exists();
        let launchd_loaded = if launchd_installed {
            Command::new("launchctl")
                .args(["list", LAUNCHD_LABEL])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .is_ok_and(|s| s.success())
        } else {
            false
        };

        if launchd_installed {
            println!(
                "Launch agent: installed ({})",
                if launchd_loaded {
                    "loaded"
                } else {
                    "not loaded"
                }
            );
        } else {
            println!(
                "Launch agent: not installed (run `sweeprs monitor --install` to start on login)"
            );
        }
    }

    #[cfg(target_os = "linux")]
    {
        let unit_installed = systemd_unit_path().exists();
        let unit_active = if unit_installed {
            Command::new("systemctl")
                .args(["--user", "is-active", "--quiet", SYSTEMD_UNIT])
                .status()
                .is_ok_and(|s| s.success())
        } else {
            false
        };

        if unit_installed {
            println!(
                "Systemd user service: installed ({})",
                if unit_active { "active" } else { "inactive" }
            );
        } else {
            println!(
                "Systemd user service: not installed (run `sweeprs monitor --install` to start on login)"
            );
        }
    }

    if !pid_path.exists() {
        println!("Monitor: not running");
        return Ok(());
    }

    let pid = std::fs::read_to_string(&pid_path)?.trim().to_owned();

    if is_process_running(&pid) {
        println!("Monitor: running (PID: {pid})");
    } else {
        println!("Monitor: not running (stale PID file)");
        std::fs::remove_file(&pid_path)?;
    }

    Ok(())
}

// --- Install ---

#[cfg(target_os = "macos")]
pub fn install() -> Result<()> {
    let exe = std::env::current_exe()?;
    let exe_str = exe.display().to_string();
    let plist = plist_path();

    let log_dir = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Library/Logs/sweeprs");
    std::fs::create_dir_all(&log_dir)?;

    let stdout_log = log_dir.join("monitor.log");
    let stderr_log = log_dir.join("monitor.err");

    let content = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{LAUNCHD_LABEL}</string>
    <key>ProgramArguments</key>
    <array>
        <string>{exe_str}</string>
        <string>monitor</string>
        <string>--foreground</string>
    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>KeepAlive</key>
    <true/>
    <key>StandardOutPath</key>
    <string>{}</string>
    <key>StandardErrorPath</key>
    <string>{}</string>
    <key>ProcessType</key>
    <string>Background</string>
</dict>
</plist>
"#,
        stdout_log.display(),
        stderr_log.display()
    );

    if let Some(parent) = plist.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&plist, content)?;

    let status = Command::new("launchctl")
        .args(["load", "-w"])
        .arg(&plist)
        .status()?;

    if status.success() {
        println!("Launch agent installed and loaded.");
        println!("Plist: {}", plist.display());
        println!("Logs:  {}", log_dir.display());
        println!("The monitor will now start automatically on login.");
    } else {
        println!("Launch agent written to: {}", plist.display());
        println!("Warning: `launchctl load` exited with {status}. Try loading manually.");
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub fn install() -> Result<()> {
    let exe = std::env::current_exe()?;
    let exe = std::fs::canonicalize(&exe).unwrap_or(exe);
    let exe_str = exe.display().to_string();
    let unit_path = systemd_unit_path();

    let content = format!(
        r"[Unit]
Description=sweeprs disk usage monitor
Documentation=https://github.com/salamaashoush/sweeprs

[Service]
Type=simple
ExecStart={exe_str} monitor --foreground
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
"
    );

    if let Some(parent) = unit_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&unit_path, content)?;

    // Reload systemd user daemon so it picks up the new unit
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();

    // Enable and start the service
    let enable_status = Command::new("systemctl")
        .args(["--user", "enable", "--now", SYSTEMD_UNIT])
        .status()?;

    if enable_status.success() {
        println!("Systemd user service installed and started.");
        println!("Unit: {}", unit_path.display());
        println!("The monitor will now start automatically on login.");
        println!();
        println!("Manage with:");
        println!("  systemctl --user status {SYSTEMD_UNIT}");
        println!("  journalctl --user -u {SYSTEMD_UNIT}");
    } else {
        println!("Unit file written to: {}", unit_path.display());
        println!(
            "Warning: `systemctl --user enable --now` failed. Try enabling manually."
        );
    }

    Ok(())
}

// --- Uninstall ---

#[cfg(target_os = "macos")]
pub fn uninstall() -> Result<()> {
    let plist = plist_path();

    if !plist.exists() {
        println!("Launch agent not installed.");
        return Ok(());
    }

    let _ = Command::new("launchctl")
        .args(["unload", "-w"])
        .arg(&plist)
        .status();

    std::fs::remove_file(&plist)?;
    println!("Launch agent uninstalled. Monitor will no longer start on login.");

    let pid_path = daemon::pid_file_path();
    if pid_path.exists() {
        let pid = std::fs::read_to_string(&pid_path)?.trim().to_owned();
        if is_process_running(&pid) {
            let _ = Command::new("kill").arg(&pid).status();
            println!("Stopped running monitor (PID: {pid}).");
        }
        let _ = std::fs::remove_file(&pid_path);
    }

    Ok(())
}

#[cfg(target_os = "linux")]
pub fn uninstall() -> Result<()> {
    let unit_path = systemd_unit_path();

    if !unit_path.exists() {
        println!("Systemd user service not installed.");
        return Ok(());
    }

    // Stop and disable the service
    let _ = Command::new("systemctl")
        .args(["--user", "disable", "--now", SYSTEMD_UNIT])
        .status();

    std::fs::remove_file(&unit_path)?;

    // Reload so systemd forgets the unit
    let _ = Command::new("systemctl")
        .args(["--user", "daemon-reload"])
        .status();

    println!("Systemd user service uninstalled. Monitor will no longer start on login.");

    let pid_path = daemon::pid_file_path();
    if pid_path.exists() {
        let pid = std::fs::read_to_string(&pid_path)?.trim().to_owned();
        if is_process_running(&pid) {
            let _ = Command::new("kill").arg(&pid).status();
            println!("Stopped running monitor (PID: {pid}).");
        }
        let _ = std::fs::remove_file(&pid_path);
    }

    Ok(())
}

// --- Foreground ---

pub fn run_foreground(auto_clean: bool) -> Result<()> {
    let mut config = Config::load()?;
    if auto_clean {
        config.monitor.auto_clean = true;
    }

    let pid_path = daemon::pid_file_path();
    let pid = std::process::id();

    if let Some(parent) = pid_path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&pid_path, pid.to_string())?;

    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_signal = Arc::clone(&shutdown);

    ctrlc::set_handler(move || {
        shutdown_signal.store(true, Ordering::Relaxed);
    })?;

    eprintln!("[sweeprs monitor] Starting (PID: {pid})");
    if config.monitor.auto_clean {
        eprintln!("[sweeprs monitor] Auto-clean enabled");
    }
    daemon::run_loop(&config, &shutdown);

    let _ = std::fs::remove_file(&pid_path);
    Ok(())
}

/// Check if a process is running AND is actually a sweeprs instance.
fn is_process_running(pid: &str) -> bool {
    let alive = Command::new("kill")
        .args(["-0", pid])
        .status()
        .is_ok_and(|s| s.success());

    if !alive {
        return false;
    }

    let output = Command::new("ps").args(["-p", pid, "-o", "comm="]).output();

    match output {
        Ok(out) => {
            let comm = String::from_utf8_lossy(&out.stdout);
            let comm = comm.trim();
            comm.ends_with("sweeprs") || comm.contains("sweeprs")
        }
        Err(_) => false,
    }
}
