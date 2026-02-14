pub mod daemon;
pub mod notify;

use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, anyhow};

use crate::config::Config;

const LAUNCHD_LABEL: &str = "com.sweeprs.monitor";

fn plist_path() -> PathBuf {
    dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("~"))
        .join("Library/LaunchAgents")
        .join(format!("{LAUNCHD_LABEL}.plist"))
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

    // Check launchd service status
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
        println!("Launch agent: not installed (run `sweeprs monitor --install` to start on login)");
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

    // Load the service
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

pub fn uninstall() -> Result<()> {
    let plist = plist_path();

    if !plist.exists() {
        println!("Launch agent not installed.");
        return Ok(());
    }

    // Unload first
    let _ = Command::new("launchctl")
        .args(["unload", "-w"])
        .arg(&plist)
        .status();

    std::fs::remove_file(&plist)?;
    println!("Launch agent uninstalled. Monitor will no longer start on login.");

    // Also stop any running instance
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

pub fn run_foreground(auto_clean: bool) -> Result<()> {
    let mut config = Config::load()?;
    // CLI --auto-clean flag overrides config
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
/// This prevents false positives when a PID is reused by a different process.
fn is_process_running(pid: &str) -> bool {
    // First check if the process exists at all
    let alive = Command::new("kill")
        .args(["-0", pid])
        .status()
        .is_ok_and(|s| s.success());

    if !alive {
        return false;
    }

    // Verify the process is actually sweeprs by checking its command line
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
