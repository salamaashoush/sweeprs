use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::tui::app::App;
use crate::tui::explore::covered_by;
use crate::tui::theme;
use crate::util;

const BAR_WIDTH: usize = 12;

pub fn render(f: &mut Frame, area: Rect, app: &mut App) {
    let Some(explorer) = app.explorer.as_mut() else {
        return;
    };
    let size = format!(
        " · {}{} ",
        util::human_size(explorer.total()),
        if explorer.measuring() { " so far" } else { "" }
    );
    let room = (area.width as usize).saturating_sub(size.chars().count() + 4);
    let title = format!(
        " {}{size}",
        keep_end(&util::tilde_path(&explorer.dir), room)
    );
    let block = Block::default()
        .borders(Borders::ALL)
        .title(title)
        .border_style(Style::default().fg(theme::BORDER));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let height = inner.height as usize;
    explorer.clamp_scroll(height);
    if explorer.children.is_empty() {
        f.render_widget(
            Paragraph::new("Empty, or not readable.").style(Style::default().fg(theme::DIM)),
            inner,
        );
        return;
    }

    let largest = explorer
        .children
        .iter()
        .filter_map(|c| c.size)
        .max()
        .unwrap_or(0);
    let end = (explorer.scroll_offset + height).min(explorer.children.len());
    let lines: Vec<Line<'_>> = explorer.children[explorer.scroll_offset..end]
        .iter()
        .enumerate()
        .map(|(i, child)| {
            let is_cursor = explorer.scroll_offset + i == explorer.cursor;
            let size = child
                .size
                .map_or_else(|| "measuring".to_owned(), util::human_size);
            let filled = match (child.size, largest) {
                (Some(size), largest) if largest > 0 => {
                    usize::try_from(size.saturating_mul(BAR_WIDTH as u64) / largest).unwrap_or(0)
                }
                _ => 0,
            };
            let bar = format!(
                "{}{}",
                "\u{2588}".repeat(filled),
                "\u{2591}".repeat(BAR_WIDTH - filled.min(BAR_WIDTH))
            );
            let name = if child.is_dir {
                format!("{}/", child.name)
            } else {
                child.name.clone()
            };
            let style = if is_cursor {
                Style::default().fg(theme::FG).bg(theme::SURFACE).bold()
            } else {
                Style::default().fg(theme::FG)
            };
            Line::from(vec![
                Span::styled(format!(" {size:>10} "), Style::default().fg(theme::GREEN)),
                Span::styled(bar, Style::default().fg(theme::BAR_FILL)),
                Span::styled(format!(" {name}"), style),
            ])
        })
        .collect();
    f.render_widget(Paragraph::new(lines), inner);
}

/// The end of `text` in at most `room` characters: the part of a path that
/// says where you are.
fn keep_end(text: &str, room: usize) -> String {
    let len = text.chars().count();
    if len <= room {
        return text.to_owned();
    }
    let tail: String = text.chars().skip(len + 1 - room.max(1)).collect();
    format!("\u{2026}{tail}")
}

/// The side panel while exploring: where the item is, and what the scan knows.
pub fn render_detail(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .title(" Details ")
        .border_style(Style::default().fg(theme::BORDER));
    let Some(child) = app.explorer.as_ref().and_then(|e| e.current()) else {
        f.render_widget(block, area);
        return;
    };

    let label = |text: &str| Span::styled(format!("{text:<10}"), Style::default().fg(theme::DIM));
    let mut lines = vec![
        Line::from(vec![
            label("Path"),
            Span::raw(util::tilde_path(&child.path)),
        ]),
        Line::from(vec![
            label("Size"),
            Span::raw(
                child
                    .size
                    .map_or_else(|| "measuring".to_owned(), util::human_size),
            ),
        ]),
    ];
    if let Ok(modified) = child.path.symlink_metadata().and_then(|m| m.modified()) {
        let when: chrono::DateTime<chrono::Local> = modified.into();
        lines.push(Line::from(vec![
            label("Modified"),
            Span::raw(when.format("%Y-%m-%d %H:%M").to_string()),
        ]));
    }
    lines.push(Line::raw(""));
    match covered_by(&app.result.entries, &child.path) {
        Some(entry) => {
            lines.push(Line::from(vec![
                label("Rule"),
                Span::styled(
                    entry.description.clone(),
                    Style::default().fg(theme::safety_color(entry.safety)),
                ),
            ]));
            if entry.path != child.path {
                lines.push(Line::from(vec![
                    label("Covers"),
                    Span::raw(util::tilde_path(&entry.path)),
                ]));
            }
        }
        None => lines.push(Line::styled(
            "No rule offers this. Deleting it here is your call (Danger).",
            Style::default().fg(theme::YELLOW),
        )),
    }
    f.render_widget(
        Paragraph::new(lines)
            .wrap(ratatui::widgets::Wrap { trim: false })
            .block(block),
        area,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn long_paths_keep_their_end() {
        assert_eq!(keep_end("~/short", 20), "~/short");
        assert_eq!(
            keep_end("/a/very/long/path/to/models", 10),
            "\u{2026}to/models"
        );
        assert_eq!(
            keep_end("/a/very/long/path/to/models", 10).chars().count(),
            10
        );
    }
}
