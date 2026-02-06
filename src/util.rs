use std::path::Path;

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
