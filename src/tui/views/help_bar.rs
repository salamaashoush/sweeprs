use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::tui::theme;

pub fn render(f: &mut Frame, area: Rect) {
    let spans = vec![
        Span::styled(" j/k", Style::default().fg(theme::ACCENT)),
        Span::styled(" navigate  ", Style::default().fg(theme::DIM)),
        Span::styled("space", Style::default().fg(theme::ACCENT)),
        Span::styled(" select  ", Style::default().fg(theme::DIM)),
        Span::styled("l/h", Style::default().fg(theme::ACCENT)),
        Span::styled(" expand/collapse  ", Style::default().fg(theme::DIM)),
        Span::styled("d", Style::default().fg(theme::ACCENT)),
        Span::styled(" delete  ", Style::default().fg(theme::DIM)),
        Span::styled("r", Style::default().fg(theme::ACCENT)),
        Span::styled(" rescan  ", Style::default().fg(theme::DIM)),
        Span::styled("g/G", Style::default().fg(theme::ACCENT)),
        Span::styled(" top/bottom  ", Style::default().fg(theme::DIM)),
        Span::styled("q", Style::default().fg(theme::ACCENT)),
        Span::styled(" quit", Style::default().fg(theme::DIM)),
    ];

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    f.render_widget(paragraph, area);
}
