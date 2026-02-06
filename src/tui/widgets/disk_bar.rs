use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::widgets::{Block, Widget};

use crate::scanner::entry::DiskInfo;
use crate::tui::theme;
use crate::util;

pub struct DiskBar<'a> {
    disk_info: &'a DiskInfo,
    total_scanned: u64,
}

impl<'a> DiskBar<'a> {
    pub fn new(disk_info: &'a DiskInfo, total_scanned: u64) -> Self {
        Self {
            disk_info,
            total_scanned,
        }
    }
}

impl Widget for DiskBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let block = Block::bordered()
            .title(format!(" Disk: {} ", self.disk_info.name))
            .border_style(Style::default().fg(theme::BORDER));
        let inner = block.inner(area);
        block.render(area, buf);

        if inner.width == 0 || inner.height == 0 {
            return;
        }

        let total = self.disk_info.total_bytes;
        if total == 0 {
            return;
        }

        let purgeable = self.disk_info.purgeable_bytes.unwrap_or(0);
        let snapshot = self.disk_info.snapshot_bytes;
        let used = self.disk_info.used_bytes;
        let free = total.saturating_sub(used);

        // Segments: scanned (green), purgeable (cyan), snapshots (yellow), other used (dim), free (bg)
        let scanned = self.total_scanned.min(used);
        let other_used = used.saturating_sub(scanned).saturating_sub(purgeable).saturating_sub(snapshot);

        let bar_width = u64::from(inner.width);

        let seg_scanned = ((scanned as f64 / total as f64) * bar_width as f64).round() as u16;
        let seg_purgeable = ((purgeable as f64 / total as f64) * bar_width as f64).round() as u16;
        let seg_snapshot = ((snapshot as f64 / total as f64) * bar_width as f64).round() as u16;
        let seg_other = ((other_used as f64 / total as f64) * bar_width as f64).round() as u16;

        // Clamp total segments to bar_width
        let filled_total = (seg_scanned + seg_purgeable + seg_snapshot + seg_other).min(inner.width);
        let seg_free = inner.width.saturating_sub(filled_total);

        // Paint the bar segments
        let y = inner.y;
        let mut x = inner.x;

        let segments: &[(u16, ratatui::style::Color)] = &[
            (seg_scanned, theme::GREEN),
            (seg_purgeable, ratatui::style::Color::Cyan),
            (seg_snapshot, theme::YELLOW),
            (seg_other, theme::DIM),
            (seg_free, theme::SURFACE),
        ];

        for &(width, color) in segments {
            for dx in 0..width {
                if x + dx < inner.x + inner.width {
                    buf[(x + dx, y)]
                        .set_char('\u{2588}')
                        .set_fg(color);
                }
            }
            x += width;
        }

        // Build label
        let label = if purgeable > 0 || snapshot > 0 {
            let mut parts = vec![
                format!("{} used", util::human_size(used)),
            ];
            if self.total_scanned > 0 {
                parts.push(format!("{} reclaimable", util::human_size(self.total_scanned)));
            }
            if purgeable > 0 {
                parts.push(format!("{} purgeable", util::human_size(purgeable)));
            }
            if snapshot > 0 {
                parts.push(format!("{} snapshots", util::human_size(snapshot)));
            }
            format!("{} - {} free", parts.join(", "), util::human_size(free))
        } else {
            format!(
                "{} / {} ({:.1}%) - {} free",
                util::human_size(used),
                util::human_size(total),
                self.disk_info.usage_percent,
                util::human_size(free),
            )
        };

        // Center the label on the bar
        let label_width = label.len() as u16;
        let label_x = if label_width < inner.width {
            inner.x + (inner.width - label_width) / 2
        } else {
            inner.x
        };

        let label_display: String = if label_width > inner.width {
            label.chars().take(inner.width as usize).collect()
        } else {
            label
        };

        for (i, ch) in label_display.chars().enumerate() {
            let cx = label_x + i as u16;
            if cx < inner.x + inner.width {
                buf[(cx, y)].set_char(ch).set_fg(theme::FG);
            }
        }
    }
}
