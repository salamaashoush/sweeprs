use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::{Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Paragraph, Wrap};

use crate::scanner::entry::SafetyLevel;
use crate::tui::app::App;
use crate::tui::theme;
use crate::tui::tree::RowRef;
use crate::util;

pub fn render(f: &mut Frame, area: Rect, app: &App) {
    let block = Block::default()
        .title(" Details ")
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme::BORDER));

    let visible = app.tree.visible_rows();
    let Some(row) = visible.get(app.cursor) else {
        let empty = Paragraph::new("No item selected")
            .style(Style::default().fg(theme::DIM))
            .block(block);
        f.render_widget(empty, area);
        return;
    };

    let lines = match *row {
        RowRef::Category(ci) => render_category_detail(&app.tree, ci),
        RowRef::Group(ci, gi) => render_group_detail(&app.tree, ci, gi),
        RowRef::Entry(ci, gi, ei) => render_entry_detail(&app.tree, ci, gi, ei),
    };

    let paragraph = Paragraph::new(lines)
        .block(block)
        .wrap(Wrap { trim: false });
    f.render_widget(paragraph, area);
}

fn render_category_detail(tree: &crate::tui::tree::Tree, ci: usize) -> Vec<Line<'static>> {
    let cat = &tree.categories[ci];
    let safety = cat.category.default_safety();
    let safety_color = safety_to_color(safety);

    vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("{}", cat.category),
            Style::default().fg(theme::ACCENT).bold(),
        )),
        Line::from(""),
        detail_line("Size", &util::human_size(cat.total_size)),
        detail_line("Items", &cat.entry_count.to_string()),
        detail_line("Groups", &cat.groups.len().to_string()),
        Line::from(vec![
            Span::styled("  Safety:  ", Style::default().fg(theme::DIM)),
            Span::styled(format!("{safety}"), Style::default().fg(safety_color)),
        ]),
    ]
}

fn render_group_detail(tree: &crate::tui::tree::Tree, ci: usize, gi: usize) -> Vec<Line<'static>> {
    let group = &tree.categories[ci].groups[gi];
    let safety_color = safety_to_color(group.safety);

    vec![
        Line::from(""),
        Line::from(Span::styled(
            group.name.clone(),
            Style::default().fg(theme::ACCENT).bold(),
        )),
        Line::from(""),
        detail_line("Size", &util::human_size(group.total_size)),
        detail_line("Entries", &group.entries.len().to_string()),
        Line::from(vec![
            Span::styled("  Safety:  ", Style::default().fg(theme::DIM)),
            Span::styled(
                format!("{}", group.safety),
                Style::default().fg(safety_color),
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            format!("  Category: {}", tree.categories[ci].category),
            Style::default().fg(theme::DIM),
        )),
    ]
}

fn render_entry_detail(tree: &crate::tui::tree::Tree, ci: usize, gi: usize, ei: usize) -> Vec<Line<'static>> {
    let entry = &tree.categories[ci].groups[gi].entries[ei];
    let safety_color = safety_to_color(entry.safety);

    let mut lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            util::tilde_path(&entry.path),
            Style::default().fg(theme::ACCENT).bold(),
        )),
        Line::from(""),
        detail_line("Size", &util::human_size(entry.size)),
        Line::from(vec![
            Span::styled("  Safety:  ", Style::default().fg(theme::DIM)),
            Span::styled(
                format!("{}", entry.safety),
                Style::default().fg(safety_color),
            ),
        ]),
    ];

    if let Some(count) = entry.item_count {
        lines.push(detail_line("Items", &format!("{count:}")));
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        format!("  {}", entry.description),
        Style::default().fg(theme::FG),
    )));

    lines
}

fn detail_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("  {label}:  "),
            Style::default().fg(theme::DIM),
        ),
        Span::styled(
            value.to_string(),
            Style::default().fg(theme::FG).bold(),
        ),
    ])
}

fn safety_to_color(safety: SafetyLevel) -> ratatui::style::Color {
    match safety {
        SafetyLevel::Safe => theme::SAFE_COLOR,
        SafetyLevel::Caution => theme::CAUTION_COLOR,
        SafetyLevel::Danger => theme::DANGER_COLOR,
    }
}
