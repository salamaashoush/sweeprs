use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::app::App;
use crate::tui::theme;
use crate::util;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let (sel_count, sel_size) = app.tree.selection_summary();
    let total = app.tree.total_reclaimable();

    let mut spans = Vec::new();

    if app.scanning {
        spans.push(Span::styled(
            format!(
                " Scanning {}/{} rules",
                app.scan_rules_done, app.scan_rules_total
            ),
            Style::default().fg(theme::ACCENT).bold(),
        ));
        spans.push(Span::styled(" | ", Style::default().fg(theme::DIM)));
    }

    if sel_count > 0 {
        spans.push(Span::styled(
            format!(
                " Selected: {sel_count} item{}",
                if sel_count == 1 { "" } else { "s" }
            ),
            Style::default().fg(theme::FG),
        ));
        spans.push(Span::styled(
            format!(" ({})", util::human_size(sel_size)),
            Style::default().fg(theme::GREEN).bold(),
        ));
    } else {
        spans.push(Span::styled(
            " No items selected",
            Style::default().fg(theme::DIM),
        ));
    }

    spans.push(Span::styled("  ", Style::default()));

    // Right-align total reclaimable
    let right = format!("Total reclaimable: {} ", util::human_size(total));
    let left_len: usize = spans.iter().map(Span::width).sum();
    let area_width = area.width as usize;
    let padding = area_width.saturating_sub(left_len + right.len());
    spans.push(Span::styled(" ".repeat(padding), Style::default()));
    spans.push(Span::styled(
        right,
        Style::default().fg(theme::GREEN).bold(),
    ));

    let line = Line::from(spans).style(Style::default().bg(theme::SURFACE));
    let paragraph = Paragraph::new(line);
    f.render_widget(paragraph, area);
}
