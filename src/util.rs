use std::io;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use humansize::{BINARY, format_size};

pub fn human_size(bytes: u64) -> String {
    format_size(bytes, BINARY)
}

/// Replace the home directory prefix with `~` for display.
pub fn tilde_path(path: &Path) -> String {
    let s = path.display().to_string();
    if let Some(home) = dirs::home_dir() {
        let home_str = home.display().to_string();
        if let Some(rest) = s.strip_prefix(&home_str) {
            return format!("~{rest}");
        }
    }
    s
}

pub enum CommandOutcome {
    Completed(Output),
    TimedOut,
    NotSpawned(io::Error),
}

impl CommandOutcome {
    /// The output of a command that ran to completion with a zero exit status.
    pub fn success(self) -> Option<Output> {
        match self {
            Self::Completed(output) if output.status.success() => Some(output),
            _ => None,
        }
    }
}

/// Run a command, killing it if it outlives `timeout`.
///
/// `stdout` and `stderr` are drained on their own threads. Polling `try_wait`
/// without draining deadlocks as soon as a child fills the 64 KB pipe buffer,
/// which turns "this command is chatty" into "sweeprs hangs".
fn drain<R: io::Read + Send + 'static>(pipe: Option<R>) -> std::thread::JoinHandle<Vec<u8>> {
    std::thread::spawn(move || {
        let mut buf = Vec::new();
        if let Some(mut pipe) = pipe {
            let _: io::Result<usize> = pipe.read_to_end(&mut buf);
        }
        buf
    })
}

pub fn run_with_timeout(args: &[&str], timeout: Duration) -> CommandOutcome {
    let mut child = match Command::new(args[0])
        .args(&args[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    {
        Ok(child) => child,
        Err(e) => return CommandOutcome::NotSpawned(e),
    };

    let stdout_reader = drain(child.stdout.take());
    let stderr_reader = drain(child.stderr.take());

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) => {
                if Instant::now() >= deadline {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => return CommandOutcome::NotSpawned(e),
        }
    };

    let stdout = stdout_reader.join().unwrap_or_default();
    let stderr = stderr_reader.join().unwrap_or_default();

    match status {
        Some(status) => CommandOutcome::Completed(Output {
            status,
            stdout,
            stderr,
        }),
        None => CommandOutcome::TimedOut,
    }
}
