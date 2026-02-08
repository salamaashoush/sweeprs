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
        Span::styled("o", Style::default().fg(theme::ACCENT)),
        Span::styled(" reveal  ", Style::default().fg(theme::DIM)),
        Span::styled("y", Style::default().fg(theme::ACCENT)),
        Span::styled(" copy path  ", Style::default().fg(theme::DIM)),
        Span::styled("/", Style::default().fg(theme::ACCENT)),
        Span::styled(" search  ", Style::default().fg(theme::DIM)),
        Span::styled("q", Style::default().fg(theme::ACCENT)),
        Span::styled(" quit", Style::default().fg(theme::DIM)),
    ];

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    f.render_widget(paragraph, area);
}

pub fn render_with_filter(f: &mut Frame, area: Rect, query: &str) {
    let spans = vec![
        Span::styled(
            format!(" Filter: \"{query}\""),
            Style::default().fg(theme::ACCENT),
        ),
        Span::styled("  ", Style::default()),
        Span::styled("/", Style::default().fg(theme::ACCENT)),
        Span::styled(" edit filter  ", Style::default().fg(theme::DIM)),
        Span::styled("Esc", Style::default().fg(theme::ACCENT)),
        Span::styled(" clear  ", Style::default().fg(theme::DIM)),
    ];

    let line = Line::from(spans);
    let paragraph = Paragraph::new(line);
    f.render_widget(paragraph, area);
}
