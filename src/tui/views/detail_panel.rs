use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::output;
use crate::scanner::entry::SafetyLevel;
use crate::tui::app::App;
use crate::tui::theme;
use crate::tui::tree::{EntryNode, RowRef, Tree};
use crate::util;
use crate::virtual_entry;

/// Entries listed under "Largest" for a category or group.
const LARGEST: usize = 6;
const LABEL_COL: usize = 10;

pub fn render(f: &mut Frame, area: Rect, app: &mut App) {
    let block = Block::default()
        .title(" Details ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER));

    let visible = app.tree.visible_rows();
    let total = app.tree.total_reclaimable();
    let Some(row) = visible.get(app.cursor) else {
        let empty = Paragraph::new("No item selected")
            .style(Style::default().fg(theme::DIM))
            .block(block);
        f.render_widget(empty, area);
        return;
    };

    // Two for the border, two for the indent.
    let width = usize::from(area.width).saturating_sub(4);
    let lines = match *row {
        RowRef::Category(ci) => category_detail(&app.tree, ci, total, width),
        RowRef::Group(ci, gi) => group_detail(&app.tree, ci, gi, width),
        RowRef::Entry(ci, gi, ei) => entry_detail(&app.tree, ci, gi, ei, width),
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

/// What choosing to delete at this level costs.
fn safety_meaning(safety: SafetyLevel) -> &'static str {
    match safety {
        SafetyLevel::Safe => "Rebuilt or re-fetched automatically when needed.",
        SafetyLevel::Caution => "Can be restored, but at the cost of a download or rebuild.",
        SafetyLevel::Danger => "Has no other copy. Deleting it loses it.",
        SafetyLevel::Error => "Kept: the description says why. It cannot be selected.",
    }
}

fn title(text: &str) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(
            format!(" {text}"),
            Style::default().fg(theme::ACCENT).bold(),
        )),
        Line::from(""),
    ]
}

fn fact(label: &str, value: Span<'static>) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {label:<LABEL_COL$}"),
            Style::default().fg(theme::DIM),
        ),
        value,
    ])
}

fn plain(value: String) -> Span<'static> {
    Span::styled(value, Style::default().fg(theme::FG).bold())
}

fn safety_span(safety: SafetyLevel) -> Span<'static> {
    Span::styled(
        safety.to_string(),
        Style::default().fg(theme::safety_color(safety)),
    )
}

/// One line per safety level present, with the bytes at that level.
/// `text` word-wrapped to `width`, every line indented under the value column.
fn indented(text: &str, width: usize) -> Vec<Line<'static>> {
    let lead = " ".repeat(2 + LABEL_COL);
    let room = width.saturating_sub(LABEL_COL + 1).max(10);
    let mut lines = Vec::new();
    let mut line = String::new();
    for word in text.split_whitespace() {
        if !line.is_empty() && line.len() + 1 + word.len() > room {
            lines.push(std::mem::take(&mut line));
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(word);
    }
    if !line.is_empty() {
        lines.push(line);
    }
    lines
        .into_iter()
        .map(|l| {
            Line::from(Span::styled(
                format!("{lead}{l}"),
                Style::default().fg(theme::DIM),
            ))
        })
        .collect()
}

fn safety_lines<'a>(
    entries: impl Iterator<Item = &'a EntryNode>,
    width: usize,
) -> Vec<Line<'static>> {
    let entries: Vec<&EntryNode> = entries.collect();
    let levels = [
        SafetyLevel::Safe,
        SafetyLevel::Caution,
        SafetyLevel::Danger,
        SafetyLevel::Error,
    ];
    let present: Vec<(SafetyLevel, u64, usize)> = levels
        .into_iter()
        .filter_map(|level| {
            let at: Vec<_> = entries.iter().filter(|e| e.safety == level).collect();
            (!at.is_empty()).then(|| (level, at.iter().map(|e| e.size).sum(), at.len()))
        })
        .collect();

    if let [(level, _, _)] = present.as_slice() {
        let mut lines = vec![fact("Safety", safety_span(*level))];
        lines.extend(indented(safety_meaning(*level), width));
        return lines;
    }
    present
        .into_iter()
        .enumerate()
        .map(|(i, (level, size, count))| {
            let label = if i == 0 { "Safety" } else { "" };
            fact(
                label,
                Span::styled(
                    format!(
                        "{:<11}{} ({})",
                        util::human_size(size),
                        level,
                        output::plural(count, "item", "items")
                    ),
                    Style::default().fg(theme::safety_color(level)),
                ),
            )
        })
        .collect()
}

/// The largest entries, each as its size and its path.
fn largest<'a>(entries: impl Iterator<Item = &'a EntryNode>, width: usize) -> Vec<Line<'static>> {
    let mut entries: Vec<&EntryNode> = entries.collect();
    entries.sort_by_key(|e| std::cmp::Reverse(e.size));
    let path_room = width.saturating_sub(13);

    let mut lines = vec![Line::from(""), fact("Largest", Span::raw(""))];
    for entry in entries.iter().take(LARGEST) {
        lines.push(Line::from(vec![
            Span::styled(
                format!("  {:>10}  ", util::human_size(entry.size)),
                Style::default().fg(theme::safety_color(entry.safety)),
            ),
            Span::styled(
                output::truncate_start(&virtual_entry::display(&entry.path), path_room),
                Style::default().fg(theme::FG),
            ),
        ]));
    }
    if entries.len() > LARGEST {
        let rest = &entries[LARGEST..];
        lines.push(Line::from(Span::styled(
            format!(
                "  {:>10}  + {} more",
                util::human_size(rest.iter().map(|e| e.size).sum()),
                rest.len()
            ),
            Style::default().fg(theme::DIM),
        )));
    }
    lines
}

fn category_detail(tree: &Tree, ci: usize, total: u64, width: usize) -> Vec<Line<'static>> {
    let cat = &tree.categories[ci];
    let entries = || cat.groups.iter().flat_map(|g| g.entries.iter());
    let share = if total > 0 {
        cat.total_size as f64 / total as f64 * 100.0
    } else {
        0.0
    };

    let mut lines = title(&cat.category.to_string());
    lines.push(fact(
        "Size",
        plain(format!(
            "{} · {share:.0}% of total",
            util::human_size(cat.total_size)
        )),
    ));
    lines.push(fact(
        "Items",
        plain(output::plural(cat.entry_count, "item", "items")),
    ));
    lines.extend(safety_lines(entries(), width));
    lines.extend(largest(entries(), width));
    lines
}

fn group_detail(tree: &Tree, ci: usize, gi: usize, width: usize) -> Vec<Line<'static>> {
    let cat = &tree.categories[ci];
    let group = &cat.groups[gi];

    let mut lines = title(&group.name);
    lines.push(fact("Size", plain(util::human_size(group.total_size))));
    lines.push(fact(
        "Items",
        plain(output::plural(group.entries.len(), "item", "items")),
    ));
    lines.push(fact(
        "Category",
        Span::styled(cat.category.to_string(), Style::default().fg(theme::FG)),
    ));
    lines.extend(safety_lines(group.entries.iter(), width));
    match group.entries.as_slice() {
        [only] => lines.extend(path_lines(only)),
        entries => lines.extend(largest(entries.iter(), width)),
    }
    lines
}

fn entry_detail(tree: &Tree, ci: usize, gi: usize, ei: usize, width: usize) -> Vec<Line<'static>> {
    let entry = &tree.categories[ci].groups[gi].entries[ei];

    let mut lines = title(&entry.description);
    lines.push(fact("Size", plain(util::human_size(entry.size))));
    if let Some(count) = entry.item_count {
        lines.push(fact(
            "Contains",
            plain(output::plural(count, "item", "items")),
        ));
    }
    if !virtual_entry::is_virtual(&entry.path)
        && let Ok(modified) = entry.path.symlink_metadata().and_then(|m| m.modified())
    {
        let when: chrono::DateTime<chrono::Local> = modified.into();
        lines.push(fact("Modified", plain(when.format("%Y-%m-%d").to_string())));
    }
    lines.extend(safety_lines(std::iter::once(entry), width));
    if let Some(restore) = crate::journal::restore_command(&entry.path) {
        lines.push(Line::from(""));
        lines.push(fact("Restore", Span::raw("")));
        lines.push(Line::from(Span::styled(
            format!("  {restore}"),
            Style::default().fg(theme::FG),
        )));
    }
    lines.extend(path_lines(entry));
    lines
}

fn path_lines(entry: &EntryNode) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        fact("Path", Span::raw("")),
        Line::from(Span::styled(
            format!("  {}", virtual_entry::display(&entry.path)),
            Style::default().fg(theme::FG),
        )),
    ]
}
