use anyhow::Result;
use mac_notification_sys::{MainButton, Notification, NotificationResponse};

/// Send a warning notification with a "Clean Now" action button.
/// Returns `true` if the user clicked "Clean Now".
pub fn send_warning(title: &str, message: &str) -> Result<bool> {
    let mut notification = Notification::new();
    notification
        .title(title)
        .message(message)
        .main_button(MainButton::SingleAction("Clean Now"))
        .close_button("Dismiss")
        .sound(mac_notification_sys::Sound::Default);

    let response = notification.send()?;

    let clicked_clean = matches!(response, NotificationResponse::ActionButton(ref s) if s == "Clean Now")
        || matches!(response, NotificationResponse::Click);

    Ok(clicked_clean)
}
