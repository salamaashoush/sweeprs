use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::widgets::{Block, Gauge, Widget};

use crate::scanner::entry::DiskInfo;
use crate::tui::theme;
use crate::util;

pub struct DiskBar<'a> {
    disk_info: &'a DiskInfo,
}

impl<'a> DiskBar<'a> {
    pub fn new(disk_info: &'a DiskInfo) -> Self {
        Self { disk_info }
    }
}

impl Widget for DiskBar<'_> {
    fn render(self, area: Rect, buf: &mut Buffer) {
        let ratio = (self.disk_info.usage_percent / 100.0).min(1.0);
        let color = if self.disk_info.usage_percent >= 95.0 {
            theme::GAUGE_HIGH
        } else if self.disk_info.usage_percent >= 85.0 {
            theme::GAUGE_MED
        } else {
            theme::GAUGE_LOW
        };

        let label = format!(
            "{} / {} ({:.1}%) - {} free",
            util::human_size(self.disk_info.used_bytes),
            util::human_size(self.disk_info.total_bytes),
            self.disk_info.usage_percent,
            util::human_size(self.disk_info.available_bytes),
        );

        let gauge = Gauge::default()
            .block(
                Block::bordered()
                    .title(format!(" Disk: {} ", self.disk_info.name))
                    .border_style(Style::default().fg(theme::BORDER)),
            )
            .gauge_style(Style::default().fg(color).bg(theme::SURFACE))
            .ratio(ratio)
            .label(label.fg(theme::FG));

        gauge.render(area, buf);
    }
}
