pub mod daemon;
pub mod notify;

use std::process::Command;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Result, anyhow};

use crate::config::Config;

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

pub fn run_foreground() -> Result<()> {
    let config = Config::load()?;
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
    daemon::run_loop(&config, &shutdown);

    let _ = std::fs::remove_file(&pid_path);
    Ok(())
}

fn is_process_running(pid: &str) -> bool {
    Command::new("kill")
        .args(["-0", pid])
        .status()
        .is_ok_and(|s| s.success())
}
