use std::process::Command;

/// Send a desktop notification (fire-and-forget).
///
/// On macOS, uses `osascript` with `display notification` `AppleScript`.
/// On Linux, calls the freedesktop `org.freedesktop.Notifications` D-Bus interface
/// via `gdbus` (glib2, always present), falling back to `notify-send` (libnotify).
pub fn send_notification(title: &str, message: &str) {
    #[cfg(target_os = "macos")]
    {
        let script = format!(
            "display notification {message} with title {title}",
            message = applescript_quote(message),
            title = applescript_quote(title),
        );

        match Command::new("osascript").args(["-e", &script]).spawn() {
            Ok(_child) => {}
            Err(e) => {
                eprintln!("[sweeprs monitor] Failed to spawn notification: {e}");
            }
        }
    }

    #[cfg(target_os = "linux")]
    {
        // Try gdbus first (ships with glib2, present on virtually all graphical Linux)
        // This calls the freedesktop Desktop Notifications spec directly via D-Bus,
        // works on GNOME, KDE, XFCE, Sway, i3+dunst, Hyprland, etc.
        let gdbus_result = Command::new("gdbus")
            .args([
                "call",
                "--session",
                "--dest",
                "org.freedesktop.Notifications",
                "--object-path",
                "/org/freedesktop/Notifications",
                "--method",
                "org.freedesktop.Notifications.Notify",
                "sweeprs", // app_name
                "0",       // replaces_id
                "",        // app_icon
                title,     // summary
                message,   // body
                "[]",      // actions
                "{}",      // hints
                "5000",    // expire_timeout ms
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();

        if gdbus_result.is_ok() {
            return;
        }

        // Fallback: notify-send (requires libnotify, not always installed)
        match Command::new("notify-send")
            .args(["-a", "sweeprs", title, message])
            .spawn()
        {
            Ok(_child) => {}
            Err(e) => {
                eprintln!("[sweeprs monitor] No notification method available: {e}");
            }
        }
    }
}

#[cfg(target_os = "macos")]
fn applescript_quote(s: &str) -> String {
    let escaped = s.replace('\\', "\\\\").replace('"', "\\\"");
    format!("\"{escaped}\"")
}
