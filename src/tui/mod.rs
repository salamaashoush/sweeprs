pub mod app;
pub mod deletion;
pub mod event;
pub mod explore;
pub mod theme;
pub mod tree;
pub mod views;
pub mod widgets;

use std::io;

use anyhow::Result;
use crossterm::execute;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Layout};
use ratatui::prelude::Widget;
use ratatui::style::Style;

use crate::config::Config;

use app::App;
use event::{Event, EventHandler};
use views::View;
use widgets::disk_bar::DiskBar;

pub fn run() -> Result<()> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let config = Config::load()?;
    let mut app = App::new(config);
    let events = EventHandler::new(200);

    app.start_scan();

    while app.running {
        app.check_scan();
        app.check_deletion();
        app.check_explorer();

        // Only redraw when state has changed (input, scan updates, etc.)
        // Always redraw while scanning since the progress text changes.
        if app.needs_redraw
            || app.scanning
            || app.deleting()
            || app
                .explorer
                .as_ref()
                .is_some_and(explore::Explorer::measuring)
        {
            app.needs_redraw = false;

            // Pre-compute values that need &mut self before entering the draw closure.
            let total_reclaimable = app.tree.total_reclaimable();

            terminal.draw(|f| draw(f, &mut app, total_reclaimable))?;
        }

        match events.next()? {
            Event::Key(key) => app.handle_key(key),
            Event::Resize => {
                app.needs_redraw = true;
            }
            Event::Tick => {}
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    Ok(())
}

/// Lay out and render one frame.
pub fn draw(f: &mut ratatui::Frame, app: &mut App, total_reclaimable: u64) {
    let chunks = Layout::vertical([
        Constraint::Length(3), // disk bar
        Constraint::Min(5),    // main content: tree + detail
        Constraint::Length(1), // status bar
        Constraint::Length(1), // help bar
    ])
    .split(f.area());

    // Disk bar / scanning progress
    if let Some(ref disk_info) = app.result.disk_info {
        DiskBar::new(disk_info, total_reclaimable).render(chunks[0], f.buffer_mut());
    } else if app.scanning {
        let scanning_text = format!(
            " Scanning... {}/{} rules | {}",
            app.scan_rules_done, app.scan_rules_total, app.last_rule_name,
        );
        let scanning = ratatui::widgets::Paragraph::new(scanning_text)
            .style(Style::default().fg(theme::ACCENT))
            .block(
                ratatui::widgets::Block::bordered()
                    .title(" sweeprs ")
                    .border_style(Style::default().fg(theme::BORDER)),
            );
        f.render_widget(scanning, chunks[0]);
    } else {
        let empty = ratatui::widgets::Block::bordered()
            .title(" sweeprs ")
            .border_style(Style::default().fg(theme::BORDER));
        f.render_widget(empty, chunks[0]);
    }

    // Main content: tree panel (62%) | detail panel (38%)
    let content = Layout::horizontal([Constraint::Percentage(62), Constraint::Percentage(38)])
        .split(chunks[1]);

    if app.view == View::Explore
        || (app.explorer.is_some() && matches!(app.view, View::Confirm | View::Deleting))
    {
        views::explore_panel::render(f, content[0], app);
        views::explore_panel::render_detail(f, content[1], app);
    } else {
        views::tree_panel::render(f, content[0], app);
        views::detail_panel::render(f, content[1], app);
    }

    // Status bar
    views::status_bar::render(f, chunks[2], app);

    // Help bar / search bar
    if app.view == View::Search {
        let search_text = format!(" /{}", app.search_query);
        let search_bar =
            ratatui::widgets::Paragraph::new(search_text).style(Style::default().fg(theme::ACCENT));
        f.render_widget(search_bar, chunks[3]);
    } else if app.explorer.is_some() {
        views::help_bar::render_explore(f, chunks[3]);
    } else if app.search_active && !app.search_query.is_empty() {
        views::help_bar::render_with_filter(f, chunks[3], &app.search_query);
    } else {
        views::help_bar::render(f, chunks[3]);
    }

    // Confirm overlay
    if app.view == View::Confirm {
        views::confirm::render(f, f.area(), &app.selected_for_deletion);
    }
    if app.view == View::Deleting
        && let Some(job) = &app.deletion
    {
        views::deleting::render(f, f.area(), job);
    }
}
