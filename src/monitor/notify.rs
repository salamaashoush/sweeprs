use anyhow::Result;
use mac_notification_sys::send_notification;

pub fn send_warning(title: &str, message: &str) -> Result<()> {
    send_notification(title, None, message, None)?;
    Ok(())
}
