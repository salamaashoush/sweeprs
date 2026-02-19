use std::process::Command;

/// Send a macOS notification via osascript (fire-and-forget).
///
/// Uses `display notification` `AppleScript` which routes through Notification Center.
/// The child process is spawned and not waited on -- no thread, no `CFRunLoop`, no zombies.
pub fn send_notification(title: &str, message: &str) {
    let script = format!(
        "display notification {message} with title {title}",
        message = applescript_quote(message),
        title = applescript_quote(title),
    );

    match Command::new("osascript").args(["-e", &script]).spawn() {
        Ok(_child) => {
            // Fire-and-forget: child will be reaped when it exits.
            // We intentionally do not wait on it.
        }
        Err(e) => {
            eprintln!("[sweeprs monitor] Failed to spawn notification: {e}");
        }
    }
}

/// Escape a string for `AppleScript`: wrap in double quotes, escape backslashes and inner quotes.
fn applescript_quote(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}
