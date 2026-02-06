use ratatui::Frame;
use ratatui::layout::{Constraint, Flex, Layout, Rect};
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use crate::scanner::entry::{SafetyLevel, ScannedEntry};
use crate::tui::theme;
use crate::util;

pub fn render(f: &mut Frame, area: Rect, entries: &[ScannedEntry]) {
    // Center a dialog box
    let dialog_area = centered_rect(60, 60, area);

    f.render_widget(Clear, dialog_area);

    let total_size: u64 = entries.iter().map(|e| e.size).sum();
    let has_danger = entries.iter().any(|e| e.safety == SafetyLevel::Danger);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            "Are you sure you want to delete these items?",
            Style::default().fg(theme::FG).bold(),
        )),
        Line::from(""),
    ];

    for entry in entries.iter().take(15) {
        let safety_color = match entry.safety {
            SafetyLevel::Safe => theme::SAFE_COLOR,
            SafetyLevel::Caution => theme::CAUTION_COLOR,
            SafetyLevel::Danger => theme::DANGER_COLOR,
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!("  [{}]", entry.safety),
                Style::default().fg(safety_color),
            ),
            Span::styled(
                format!(" {:>10}", util::human_size(entry.size)),
                Style::default().fg(theme::FG),
            ),
            Span::styled(
                format!("  {}", entry.description),
                Style::default().fg(theme::DIM),
            ),
        ]));
    }

    if entries.len() > 15 {
        lines.push(Line::from(Span::styled(
            format!("  ... and {} more", entries.len() - 15),
            Style::default().fg(theme::DIM),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("Total: ", Style::default().fg(theme::DIM)),
        Span::styled(
            util::human_size(total_size),
            Style::default().fg(theme::GREEN).bold(),
        ),
        Span::styled(
            format!(" ({} items)", entries.len()),
            Style::default().fg(theme::DIM),
        ),
    ]));

    if has_danger {
        lines.push(Line::from(""));
        lines.push(Line::from(Span::styled(
            "  WARNING: This includes Danger-level items!",
            Style::default().fg(theme::RED).bold(),
        )));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(vec![
        Span::styled("  [y] Confirm  ", Style::default().fg(theme::GREEN).bold()),
        Span::styled("  [n/Esc] Cancel  ", Style::default().fg(theme::RED).bold()),
    ]));

    let border_color = if has_danger {
        theme::RED
    } else {
        theme::ACCENT
    };

    let dialog = Paragraph::new(lines).wrap(Wrap { trim: false }).block(
        Block::default()
            .title(" Confirm Deletion ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(border_color))
            .style(Style::default().bg(theme::BG)),
    );

    f.render_widget(dialog, dialog_area);
}

fn centered_rect(percent_x: u16, percent_y: u16, area: Rect) -> Rect {
    let vertical = Layout::vertical([Constraint::Percentage(percent_y)])
        .flex(Flex::Center)
        .split(area);
    Layout::horizontal([Constraint::Percentage(percent_x)])
        .flex(Flex::Center)
        .split(vertical[0])[0]
}
