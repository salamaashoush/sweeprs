use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph};

use crate::scanner::entry::SafetyLevel;
use crate::tui::app::App;
use crate::tui::theme;
use crate::tui::tree::{CheckState, RowRef};
use crate::util;
use crate::virtual_entry;

const BAR_WIDTH: usize = 10;

pub fn render(f: &mut Frame, area: Rect, app: &mut App) {
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER));
    let inner = block.inner(area);
    f.render_widget(block, area);

    let viewport_height = inner.height as usize;
    if viewport_height == 0 {
        return;
    }

    app.clamp_scroll_to_viewport(viewport_height);

    // Compute all mutable-access data upfront, then borrow tree immutably for rendering.
    let visible = app.tree.visible_rows();
    let total_reclaimable = app.tree.total_reclaimable();

    if visible.is_empty() {
        let empty = Paragraph::new("No items found. Press 'r' to scan.")
            .style(Style::default().fg(theme::DIM));
        f.render_widget(empty, inner);
        return;
    }

    // Pre-compute check states for all visible categories and groups in this window.
    let end = (app.scroll_offset + viewport_height).min(visible.len());
    let window = &visible[app.scroll_offset..end];

    // Ensure check states are cached before we borrow tree immutably.
    app.tree.ensure_check_cache();

    let lines: Vec<Line<'_>> = window
        .iter()
        .enumerate()
        .map(|(vi, row)| {
            let abs_index = app.scroll_offset + vi;
            let is_cursor = abs_index == app.cursor;

            match *row {
                RowRef::Category(ci) => {
                    render_category_row(&app.tree, ci, is_cursor, total_reclaimable)
                }
                RowRef::Group(ci, gi) => render_group_row(&app.tree, ci, gi, is_cursor),
                RowRef::Entry(ci, gi, ei) => render_entry_row(&app.tree, ci, gi, ei, is_cursor),
            }
        })
        .collect();

    let paragraph = Paragraph::new(lines);
    f.render_widget(paragraph, inner);
}

fn size_bar(
    size: u64,
    max_size: u64,
    width: usize,
    fill_color: ratatui::style::Color,
) -> Span<'static> {
    if max_size == 0 {
        return Span::styled(
            "\u{2591}".repeat(width),
            Style::default().fg(theme::BAR_FILL),
        );
    }

    let ratio = size as f64 / max_size as f64;
    let filled = (ratio * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);

    let mut bar = "\u{2588}".repeat(filled);
    bar.push_str(&"\u{2591}".repeat(empty));

    Span::styled(bar, Style::default().fg(fill_color))
}

fn render_category_row(
    tree: &crate::tui::tree::Tree,
    ci: usize,
    is_cursor: bool,
    total_reclaimable: u64,
) -> Line<'static> {
    let cat = &tree.categories[ci];
    let arrow = if cat.expanded { "v" } else { ">" };
    let check = match tree.cached_category_check_state(ci) {
        CheckState::Checked => "[x]",
        CheckState::Partial => "[-]",
        CheckState::Unchecked => "[ ]",
    };
    let safety_color = safety_to_color(cat.category.default_safety());

    let mut spans = vec![
        Span::styled(
            format!(" {arrow} {check} "),
            Style::default().fg(theme::ACCENT),
        ),
        Span::styled(
            format!("{}", cat.category),
            Style::default().fg(theme::FG).bold(),
        ),
    ];

    let size_str = util::human_size(cat.total_size);
    let count_str = format!("  ({} items)", cat.entry_count);
    spans.push(Span::styled(count_str, Style::default().fg(theme::DIM)));
    spans.push(Span::styled(
        format!("  {size_str}  "),
        Style::default().fg(safety_color).bold(),
    ));
    spans.push(size_bar(
        cat.total_size,
        total_reclaimable,
        BAR_WIDTH,
        safety_color,
    ));

    let style = if is_cursor {
        Style::default().bg(theme::SURFACE)
    } else {
        Style::default()
    };

    Line::from(spans).style(style)
}

fn render_group_row(
    tree: &crate::tui::tree::Tree,
    ci: usize,
    gi: usize,
    is_cursor: bool,
) -> Line<'static> {
    let cat = &tree.categories[ci];
    let group = &cat.groups[gi];
    let arrow = if group.expanded { "v" } else { ">" };
    let check = match tree.cached_group_check_state(ci, gi) {
        CheckState::Checked => "[x]",
        CheckState::Partial => "[-]",
        CheckState::Unchecked => "[ ]",
    };
    let safety_color = safety_to_color(group.safety);

    let mut spans = vec![
        Span::styled(
            format!("   {arrow} {check} "),
            Style::default().fg(theme::ACCENT),
        ),
        Span::styled(group.name.clone(), Style::default().fg(theme::FG)),
    ];

    let count_str = format!("  ({})", group.entries.len());
    let size_str = util::human_size(group.total_size);
    spans.push(Span::styled(count_str, Style::default().fg(theme::DIM)));
    spans.push(Span::styled(
        format!("  {size_str}  "),
        Style::default().fg(safety_color).bold(),
    ));
    spans.push(size_bar(
        group.total_size,
        cat.total_size,
        BAR_WIDTH,
        safety_color,
    ));

    let style = if is_cursor {
        Style::default().bg(theme::SURFACE)
    } else {
        Style::default()
    };

    Line::from(spans).style(style)
}

fn render_entry_row(
    tree: &crate::tui::tree::Tree,
    ci: usize,
    gi: usize,
    ei: usize,
    is_cursor: bool,
) -> Line<'static> {
    let group = &tree.categories[ci].groups[gi];
    let entry = &group.entries[ei];
    let check = if entry.checked { "[x]" } else { "[ ]" };
    let safety_color = safety_to_color(entry.safety);

    let label = virtual_entry::display(&entry.path);

    let size_str = util::human_size(entry.size);

    let spans = vec![
        Span::styled(
            format!("       {check} "),
            Style::default().fg(safety_color),
        ),
        Span::styled(label, Style::default().fg(theme::FG)),
        Span::styled(format!("  {size_str}  "), Style::default().fg(safety_color)),
        size_bar(entry.size, group.total_size, BAR_WIDTH, safety_color),
    ];

    let style = if is_cursor {
        Style::default().bg(theme::SURFACE)
    } else {
        Style::default()
    };

    Line::from(spans).style(style)
}

fn safety_to_color(safety: SafetyLevel) -> ratatui::style::Color {
    match safety {
        SafetyLevel::Safe => theme::SAFE_COLOR,
        SafetyLevel::Caution => theme::CAUTION_COLOR,
        SafetyLevel::Danger => theme::DANGER_COLOR,
        SafetyLevel::Error => theme::ERROR_COLOR,
    }
}
