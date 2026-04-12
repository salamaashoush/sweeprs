use std::sync::mpsc;
use std::thread;
use std::time::SystemTime;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::Config;
use crate::rules::{brew, docker, git_data};
use crate::scanner;
use crate::scanner::ScanUpdate;
use crate::scanner::entry::{ScanResult, ScannedEntry};
use crate::tui::tree::{RowRef, Tree};
use crate::tui::views::View;

/// Maximum age of a cached scan result before it's ignored (2 hours).
const SCAN_CACHE_TTL_SECS: u64 = 7200;

#[allow(clippy::struct_excessive_bools)]
pub struct App {
    pub running: bool,
    pub view: View,
    pub result: ScanResult,
    pub scanning: bool,
    pub tree: Tree,
    pub cursor: usize,
    pub scroll_offset: usize,
    pub selected_for_deletion: Vec<ScannedEntry>,
    pub config: Config,
    pub scan_receiver: Option<mpsc::Receiver<ScanUpdate>>,
    pub scan_rules_done: usize,
    pub scan_rules_total: usize,
    pub last_rule_name: String,
    pub search_query: String,
    pub search_active: bool,
    /// Set to true when anything changes that requires a redraw.
    pub needs_redraw: bool,
}

impl App {
    pub fn new(config: Config) -> Self {
        // Try to load a cached scan result for instant startup
        let cached = Self::load_scan_cache();
        let has_cache = cached.is_some();
        let result = cached.unwrap_or_default();
        let tree = Tree::from_scan_result(&result);

        Self {
            running: true,
            view: View::Main,
            result,
            scanning: false,
            tree,
            cursor: 0,
            scroll_offset: 0,
            selected_for_deletion: Vec::new(),
            config,
            scan_receiver: None,
            scan_rules_done: 0,
            scan_rules_total: 0,
            last_rule_name: if has_cache {
                "cached result, rescanning...".to_owned()
            } else {
                String::new()
            },
            search_query: String::new(),
            search_active: false,
            needs_redraw: true,
        }
    }

    pub fn start_scan(&mut self) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        self.result = ScanResult::default();
        self.scan_rules_done = 0;
        self.scan_rules_total = 0;
        self.last_rule_name.clear();
        self.needs_redraw = true;

        let (tx, rx) = mpsc::channel();
        self.scan_receiver = Some(rx);
        let config = self.config.clone();

        thread::spawn(move || {
            scanner::scan_all_streaming(&config, &tx);
        });
    }

    pub fn check_scan(&mut self) {
        let Some(ref rx) = self.scan_receiver else {
            return;
        };

        let mut finished = false;
        let mut got_updates = false;
        while let Ok(msg) = rx.try_recv() {
            match msg {
                ScanUpdate::Started { rules_total } => {
                    self.scan_rules_total = rules_total;
                }
                ScanUpdate::RuleComplete { rule_name, entries } => {
                    let rule_bytes: u64 = entries.iter().map(|e| e.size).sum();
                    self.result.total_size += rule_bytes;
                    self.result.entries.extend(entries);
                    self.scan_rules_done += 1;
                    self.last_rule_name = rule_name.to_string();
                    got_updates = true;
                }
                ScanUpdate::Finished {
                    duration_secs,
                    disk_info,
                } => {
                    self.result.scan_duration_secs = Some(duration_secs);
                    self.result.disk_info = disk_info;
                    finished = true;
                }
            }
        }

        if finished {
            self.scanning = false;
            self.scan_receiver = None;
            // Save scan result for instant startup next time
            Self::save_scan_cache(&self.result);
        }

        if got_updates || finished {
            self.rebuild_tree();
            self.needs_redraw = true;
        }
    }

    fn rebuild_tree(&mut self) {
        self.tree = Tree::from_scan_result(&self.result);
        if self.search_active {
            self.tree.apply_search_filter(&self.search_query);
        }
        let visible = self.tree.visible_rows();
        let visible_count = visible.len();
        if visible_count == 0 {
            self.cursor = 0;
        } else if self.cursor >= visible_count {
            self.cursor = visible_count - 1;
        }
        self.clamp_scroll();
    }

    fn clamp_scroll(&mut self) {
        if self.cursor < self.scroll_offset {
            self.scroll_offset = self.cursor;
        }
        // scroll_offset upper bound is handled during rendering when we know viewport height
    }

    pub fn clamp_scroll_to_viewport(&mut self, viewport_height: usize) {
        if viewport_height == 0 {
            return;
        }
        if self.cursor >= self.scroll_offset + viewport_height {
            self.scroll_offset = self.cursor - viewport_height + 1;
        }
        if self.cursor < self.scroll_offset {
            self.scroll_offset = self.cursor;
        }
    }

    pub fn handle_key(&mut self, key: KeyEvent) {
        self.needs_redraw = true;

        if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
            self.running = false;
            return;
        }

        match self.view {
            View::Main => self.handle_main_key(key),
            View::Confirm => self.handle_confirm_key(key),
            View::Search => self.handle_search_key(key),
        }
    }

    #[allow(clippy::too_many_lines)]
    fn handle_main_key(&mut self, key: KeyEvent) {
        let visible = self.tree.visible_rows();
        let max = visible.len();

        match key.code {
            KeyCode::Char('q') => self.running = false,
            KeyCode::Esc => {
                if self.search_active {
                    // Clear search filter instead of quitting
                    self.search_query.clear();
                    self.search_active = false;
                    self.rebuild_tree();
                } else {
                    self.running = false;
                }
            }
            KeyCode::Char('j') | KeyCode::Down if max > 0 && self.cursor < max - 1 => {
                self.cursor += 1;
                self.clamp_scroll();
            }
            KeyCode::Char('k') | KeyCode::Up if self.cursor > 0 => {
                self.cursor -= 1;
                self.clamp_scroll();
            }
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => {
                if let Some(&row) = visible.get(self.cursor) {
                    if self.tree.is_expanded(row) {
                        // Already expanded: move to first child
                        let new_max = self.tree.visible_rows().len();
                        if self.cursor + 1 < new_max {
                            self.cursor += 1;
                            self.clamp_scroll();
                        }
                    } else {
                        self.tree.expand(row);
                        // After expanding, move to first child
                        let new_max = self.tree.visible_rows().len();
                        if self.cursor + 1 < new_max {
                            self.cursor += 1;
                            self.clamp_scroll();
                        }
                    }
                }
            }
            KeyCode::Char('h') | KeyCode::Left => {
                if let Some(&row) = visible.get(self.cursor) {
                    match row {
                        RowRef::Entry(..) | RowRef::Group(..) => {
                            if matches!(row, RowRef::Group(..)) && self.tree.is_expanded(row) {
                                self.tree.collapse(row);
                            } else if let Some(parent) = Tree::parent(row) {
                                // Jump to parent
                                let new_visible = self.tree.visible_rows();
                                if let Some(pos) = new_visible.iter().position(|r| *r == parent) {
                                    self.cursor = pos;
                                    self.clamp_scroll();
                                }
                            }
                        }
                        RowRef::Category(_) => {
                            if self.tree.is_expanded(row) {
                                self.tree.collapse(row);
                            }
                        }
                    }
                }
            }
            KeyCode::Char(' ') => {
                if let Some(&row) = visible.get(self.cursor) {
                    self.tree.toggle(row);
                }
            }
            KeyCode::Char('d') => {
                let selected = self.tree.selected_entries();
                if !selected.is_empty() {
                    self.selected_for_deletion = selected;
                    self.view = View::Confirm;
                }
            }
            KeyCode::Char('r') => {
                self.start_scan();
            }
            KeyCode::Char('g') => {
                self.cursor = 0;
                self.scroll_offset = 0;
            }
            KeyCode::Char('G') if max > 0 => {
                self.cursor = max - 1;
                self.clamp_scroll();
            }
            KeyCode::Char('o') => {
                if let Some(&RowRef::Entry(ci, gi, ei)) = visible.get(self.cursor) {
                    let path_str = self.tree.categories[ci].groups[gi].entries[ei]
                        .path
                        .display()
                        .to_string();
                    if !path_str.starts_with("docker:") && !path_str.starts_with("brew:") {
                        let _ = std::process::Command::new("open")
                            .args(["-R", &path_str])
                            .spawn();
                    }
                }
            }
            KeyCode::Char('y') => {
                // Copy selected entry path to clipboard
                if let Some(&RowRef::Entry(ci, gi, ei)) = visible.get(self.cursor) {
                    let path_str = self.tree.categories[ci].groups[gi].entries[ei]
                        .path
                        .display()
                        .to_string();
                    let _ = std::process::Command::new("pbcopy")
                        .stdin(std::process::Stdio::piped())
                        .spawn()
                        .and_then(|mut child| {
                            if let Some(ref mut stdin) = child.stdin {
                                use std::io::Write;
                                stdin.write_all(path_str.as_bytes())?;
                            }
                            child.wait()
                        });
                }
            }
            KeyCode::Char('/') => {
                self.view = View::Search;
                self.search_query.clear();
                self.search_active = true;
            }
            _ => {}
        }
    }

    fn handle_confirm_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Char('y') => {
                self.execute_deletion();
                self.view = View::Main;
                self.start_scan();
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.view = View::Main;
            }
            _ => {}
        }
    }

    fn handle_search_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                // Cancel search and clear filter
                self.view = View::Main;
                self.search_query.clear();
                self.search_active = false;
                self.rebuild_tree();
            }
            KeyCode::Enter => {
                // Confirm search and return to main view (filter stays active)
                self.view = View::Main;
                if self.search_query.is_empty() {
                    self.search_active = false;
                }
            }
            KeyCode::Backspace => {
                self.search_query.pop();
                self.rebuild_tree();
                self.cursor = 0;
                self.scroll_offset = 0;
            }
            KeyCode::Char(c) => {
                self.search_query.push(c);
                self.rebuild_tree();
                self.cursor = 0;
                self.scroll_offset = 0;
            }
            _ => {}
        }
    }

    fn execute_deletion(&mut self) {
        for entry in &self.selected_for_deletion {
            let path_str = entry.path.display().to_string();

            if path_str.starts_with("docker:") {
                let _ = docker::clean_docker_entry(&path_str);
                continue;
            }
            if path_str.starts_with("brew:") {
                let _ = brew::clean_brew_entry(&path_str);
                continue;
            }
            if path_str.starts_with("git-gc:") {
                let _ = git_data::clean_git_gc(&path_str);
                continue;
            }

            let path = &entry.path;
            if path.is_dir() {
                let _ = std::fs::remove_dir_all(path);
            } else if path.is_file() {
                let _ = std::fs::remove_file(path);
            }
        }
        self.selected_for_deletion.clear();
    }

    fn scan_cache_path() -> std::path::PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join("sweeprs")
            .join("last_scan.json")
    }

    fn load_scan_cache() -> Option<ScanResult> {
        let path = Self::scan_cache_path();
        let data = std::fs::read_to_string(&path).ok()?;

        // Check file modification time for TTL
        let metadata = std::fs::metadata(&path).ok()?;
        let modified = metadata.modified().ok()?;
        let age = SystemTime::now().duration_since(modified).ok()?;
        if age.as_secs() > SCAN_CACHE_TTL_SECS {
            return None;
        }

        serde_json::from_str(&data).ok()
    }

    fn save_scan_cache(result: &ScanResult) {
        let path = Self::scan_cache_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(result) {
            let _ = std::fs::write(&path, json);
        }
    }
}
