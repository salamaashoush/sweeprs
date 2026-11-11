use std::io;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use humansize::{BINARY, format_size};

pub fn human_size(bytes: u64) -> String {
    format_size(bytes, BINARY)
}

/// A rough, readable age: `42s`, `5m`, `3h`, `2d`.
pub fn human_duration(age: Duration) -> String {
    let secs = age.as_secs();
    match secs {
        0..60 => format!("{secs}s"),
        60..3600 => format!("{}m", secs / 60),
        3600..86_400 => format!("{}h", secs / 3600),
        _ => format!("{}d", secs / 86_400),
    }
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
    let mut command = Command::new(args[0]);
    command
        .args(&args[1..])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // Its own process group, so a timeout reaches the helpers it spawns
    // (`git gc` runs `repack` and `pack-objects`). Those inherit the pipes, and
    // killing only the parent leaves the drain threads waiting on them.
    #[cfg(unix)]
    std::os::unix::process::CommandExt::process_group(&mut command, 0);

    let mut child = match command.spawn() {
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
                    terminate(&mut child);
                    break None;
                }
                std::thread::sleep(Duration::from_millis(10));
            }
            Err(e) => {
                terminate(&mut child);
                return CommandOutcome::NotSpawned(e);
            }
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

/// Stop a child and everything in its process group.
///
/// SIGTERM first: git removes its `gc.pid` and temp packs on SIGTERM, but a
/// SIGKILL leaves `gc.pid` behind and the repo reads as busy afterwards.
#[cfg(unix)]
fn terminate(child: &mut std::process::Child) {
    use nix::sys::signal::{Signal, killpg};
    use nix::unistd::Pid;

    const GRACE: Duration = Duration::from_secs(3);

    let Ok(pid) = i32::try_from(child.id()) else {
        let _ = child.kill();
        let _ = child.wait();
        return;
    };
    let group = Pid::from_raw(pid);

    let _ = killpg(group, Signal::SIGTERM);
    let deadline = Instant::now() + GRACE;
    while Instant::now() < deadline {
        if matches!(child.try_wait(), Ok(Some(_))) {
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    // The leader may be gone while its helpers are still running.
    let _ = killpg(group, Signal::SIGKILL);
    let _ = child.wait();
}

#[cfg(not(unix))]
fn terminate(child: &mut std::process::Child) {
    let _ = child.kill();
    let _ = child.wait();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_timeout_is_not_held_up_by_a_grandchild_holding_the_pipes() {
        let started = Instant::now();
        let outcome = run_with_timeout(
            &["sh", "-c", "sleep 30 & sleep 30"],
            Duration::from_millis(200),
        );
        assert!(matches!(outcome, CommandOutcome::TimedOut));
        assert!(started.elapsed() < Duration::from_secs(10));
    }

    #[test]
    fn a_command_that_finishes_reports_its_output() {
        let output = run_with_timeout(&["sh", "-c", "echo hi"], Duration::from_secs(10))
            .success()
            .expect("sh ran");
        assert_eq!(output.stdout, b"hi\n");
    }
}
