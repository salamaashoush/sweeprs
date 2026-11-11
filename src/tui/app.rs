use std::sync::mpsc;
use std::thread;
use std::time::SystemTime;

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::config::Config;
use crate::rules;
use crate::scanner;
use crate::scanner::ScanUpdate;
use crate::scanner::entry::{ScanResult, ScannedEntry};
use crate::tui::deletion::DeletionJob;
use crate::tui::explore::Explorer;
use crate::tui::tree::{RowRef, Tree};
use crate::tui::views::View;
use crate::virtual_entry;

/// A cached scan older than this is not shown at startup. Anything younger is
/// shown at once and replaced by the rescan that starts with the app, so it
/// only has to be better than an empty screen.
const SCAN_CACHE_TTL_SECS: u64 = 7 * 24 * 3600;

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
    pub deletion: Option<DeletionJob>,
    pub config: Config,
    pub scan_receiver: Option<mpsc::Receiver<ScanUpdate>>,
    /// Where a rescan collects its results while the previous ones stay on
    /// screen, so the tree does not reshuffle under the user mid-scan.
    scan_buffer: ScanResult,
    scan_cache: Option<std::path::PathBuf>,
    /// True when there was nothing to show at scan start, so results stream
    /// straight into the tree as rules report.
    pub streaming: bool,
    pub scan_rules_done: usize,
    pub scan_rules_total: usize,
    pub last_rule_name: String,
    pub search_query: String,
    pub search_active: bool,
    /// Set to true when anything changes that requires a redraw.
    pub needs_redraw: bool,
    /// The directory browser, while it is open.
    pub explorer: Option<Explorer>,
    /// When the results on screen were scanned, if they came from the cache.
    pub cached_at: Option<SystemTime>,
}

impl App {
    pub fn new(config: Config) -> Self {
        Self::with_scan_cache(config, Some(Self::scan_cache_path()))
    }

    /// `scan_cache` is where the last result is read at startup and written
    /// after each scan; `None` keeps the app off the disk entirely.
    fn with_scan_cache(config: Config, scan_cache: Option<std::path::PathBuf>) -> Self {
        // Try to load a cached scan result for instant startup
        let cached = scan_cache.as_deref().and_then(Self::load_scan_cache);
        let has_cache = cached.is_some();
        let cached_at = cached.as_ref().map(|(_, at)| *at);
        let result = cached.map(|(result, _)| result).unwrap_or_default();
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
            deletion: None,
            config,
            scan_receiver: None,
            scan_buffer: ScanResult::default(),
            scan_cache,
            streaming: false,
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
            explorer: None,
            cached_at,
        }
    }

    pub fn start_scan(&mut self) {
        if self.scanning {
            return;
        }
        self.scanning = true;
        self.scan_buffer = ScanResult::default();
        self.streaming = self.result.entries.is_empty();
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
                    let target = if self.streaming {
                        got_updates = true;
                        &mut self.result
                    } else {
                        &mut self.scan_buffer
                    };
                    target.total_size += entries.iter().map(|e| e.size).sum::<u64>();
                    target.entries.extend(entries);
                    self.scan_rules_done += 1;
                    self.last_rule_name = rule_name.to_string();
                }
                ScanUpdate::Finished {
                    duration_secs,
                    disk_info,
                } => {
                    // Overlaps only resolve once every rule has reported, so the
                    // running total shown while scanning is provisional.
                    let collected = if self.streaming {
                        std::mem::take(&mut self.result.entries)
                    } else {
                        std::mem::take(&mut self.scan_buffer.entries)
                    };
                    let mut deduped = rules::deduplicate_entries(collected);
                    // Anything deleted while the scan ran was measured before it
                    // went; it should not come back.
                    deduped.retain(|e| virtual_entry::is_virtual(&e.path) || e.path.exists());
                    self.result.total_size = deduped.iter().map(|e| e.size).sum();
                    self.result.entries = deduped;
                    self.result.scan_duration_secs = Some(duration_secs);
                    self.result.disk_info = disk_info;
                    finished = true;
                }
            }
        }

        if finished {
            self.cached_at = None;
            self.scanning = false;
            self.scan_receiver = None;
            // Save scan result for instant startup next time
            self.save_scan_cache();
        }

        if got_updates || finished {
            self.rebuild_tree();
            self.needs_redraw = true;
        }
    }

    /// Rebuild the tree from `self.result`, keeping what the user had
    /// selected, expanded and filtered, and the cursor on the same row.
    fn rebuild_tree(&mut self) {
        let state = self.tree.state();
        let anchor = self
            .tree
            .visible_rows()
            .get(self.cursor)
            .map(|row| self.tree.row_key(*row));
        let line_on_screen = self.cursor.saturating_sub(self.scroll_offset);

        self.tree = Tree::from_scan_result(&self.result);
        self.tree.restore(&state);
        if self.search_active {
            self.tree.apply_search_filter(&self.search_query);
        }

        let visible_count = self.tree.visible_rows().len();
        let anchored = anchor.and_then(|key| self.tree.find_row(&key));
        self.cursor = anchored
            .unwrap_or(self.cursor)
            .min(visible_count.saturating_sub(1));
        self.scroll_offset = self.cursor.saturating_sub(line_on_screen);
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
            // Quitting mid-deletion would kill the worker partway through an
            // `rm`; stop after the current item instead.
            match &self.deletion {
                Some(job) if !job.is_finished() => job.stop(),
                _ => self.running = false,
            }
            return;
        }

        match self.view {
            View::Main => self.handle_main_key(key),
            View::Confirm => self.handle_confirm_key(key),
            View::Deleting => self.handle_deleting_key(key),
            View::Search => self.handle_search_key(key),
            View::Explore => self.handle_explore_key(key),
        }
    }

    /// Open the explorer on what the cursor points at: the entry's directory,
    /// or home for a category, a group or a command entry.
    fn open_explorer(&mut self) {
        let visible = self.tree.visible_rows();
        let entry_path = match visible.get(self.cursor) {
            Some(&RowRef::Entry(ci, gi, ei)) => {
                Some(self.tree.categories[ci].groups[gi].entries[ei].path.clone())
            }
            _ => None,
        };
        let dir = entry_path
            .filter(|p| !virtual_entry::is_virtual(p))
            .and_then(|p| {
                if p.is_dir() {
                    Some(p)
                } else {
                    p.parent().map(std::path::Path::to_path_buf)
                }
            })
            .or_else(dirs::home_dir)
            .unwrap_or_else(|| std::path::PathBuf::from("/"));
        self.explorer = Some(Explorer::open(dir));
        self.view = View::Explore;
    }

    fn handle_explore_key(&mut self, key: KeyEvent) {
        let Some(explorer) = self.explorer.as_mut() else {
            self.view = View::Main;
            return;
        };
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => explorer.down(),
            KeyCode::Char('k') | KeyCode::Up => explorer.up(),
            KeyCode::Char('l') | KeyCode::Right | KeyCode::Enter => explorer.enter(),
            KeyCode::Char('h') | KeyCode::Left | KeyCode::Backspace => explorer.leave(),
            KeyCode::Char('r') => explorer.refresh(),
            KeyCode::Char('d') => {
                if let Some(entry) = explorer.deletion_entry() {
                    self.selected_for_deletion = vec![entry];
                    self.view = View::Confirm;
                }
            }
            KeyCode::Char('e' | 'q') | KeyCode::Esc => {
                self.explorer = None;
                self.view = View::Main;
            }
            _ => {}
        }
    }

    /// Pick up sizes the explorer measured since the last frame.
    pub fn check_explorer(&mut self) {
        if let Some(explorer) = self.explorer.as_mut()
            && explorer.poll()
        {
            self.needs_redraw = true;
        }
    }

    /// Where a confirm or a finished deletion returns to.
    fn home_view(&self) -> View {
        if self.explorer.is_some() {
            View::Explore
        } else {
            View::Main
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
                    let entry_path = &self.tree.categories[ci].groups[gi].entries[ei].path;
                    let path_str = entry_path.display().to_string();
                    if !virtual_entry::is_virtual(entry_path) {
                        if cfg!(target_os = "macos") {
                            let _ = std::process::Command::new("open")
                                .args(["-R", &path_str])
                                .spawn();
                        } else {
                            // xdg-open opens the parent directory for files,
                            // or the directory itself for dirs
                            let target = std::path::Path::new(&path_str);
                            let dir = if target.is_file() {
                                target.parent().map(|p| p.to_string_lossy().to_string())
                            } else {
                                Some(path_str.clone())
                            };
                            if let Some(dir) = dir {
                                let _ = std::process::Command::new("xdg-open").arg(&dir).spawn();
                            }
                        }
                    }
                }
            }
            KeyCode::Char('y') => {
                if let Some(&RowRef::Entry(ci, gi, ei)) = visible.get(self.cursor) {
                    let path_str = self.tree.categories[ci].groups[gi].entries[ei]
                        .path
                        .display()
                        .to_string();
                    if cfg!(target_os = "macos") {
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
                    } else {
                        // Try wl-copy (Wayland) first, fall back to xclip (X11)
                        let wl = std::process::Command::new("wl-copy").arg(&path_str).spawn();
                        if wl.is_err() {
                            let _ = std::process::Command::new("xclip")
                                .args(["-selection", "clipboard"])
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
                }
            }
            KeyCode::Char('e') => self.open_explorer(),
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
                let entries = std::mem::take(&mut self.selected_for_deletion);
                self.deletion = Some(DeletionJob::start(entries, self.config.clone()));
                self.view = View::Deleting;
            }
            KeyCode::Char('n') | KeyCode::Esc => {
                self.view = self.home_view();
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

    fn handle_deleting_key(&mut self, key: KeyEvent) {
        let Some(job) = &self.deletion else {
            self.view = View::Main;
            return;
        };
        if job.is_finished() {
            if matches!(
                key.code,
                KeyCode::Enter | KeyCode::Esc | KeyCode::Char('q' | ' ')
            ) {
                self.close_deletion();
            }
        } else if matches!(key.code, KeyCode::Esc | KeyCode::Char('s')) {
            job.stop();
        }
    }

    /// Pick up progress from a running deletion.
    pub fn check_deletion(&mut self) {
        if let Some(job) = &mut self.deletion
            && job.poll()
        {
            self.needs_redraw = true;
        }
    }

    pub fn deleting(&self) -> bool {
        self.deletion.as_ref().is_some_and(|job| !job.is_finished())
    }

    /// Drop what was deleted from the listing. Rescanning would take as long
    /// as the first scan, and everything else on disk is as it was.
    fn close_deletion(&mut self) {
        let Some(job) = self.deletion.take() else {
            return;
        };
        let removed: rustc_hash::FxHashSet<std::path::PathBuf> =
            job.removed_paths().into_iter().collect();
        self.result.entries.retain(|e| !removed.contains(&e.path));
        self.result.total_size = self.result.entries.iter().map(|e| e.size).sum();
        self.save_scan_cache();
        self.rebuild_tree();
        if let Some(explorer) = self.explorer.as_mut() {
            explorer.refresh();
        }
        self.view = self.home_view();
    }

    fn scan_cache_path() -> std::path::PathBuf {
        dirs::cache_dir()
            .unwrap_or_else(|| std::path::PathBuf::from("/tmp"))
            .join("sweeprs")
            .join("last_scan.json")
    }

    /// The last scan and when it was taken, without what has since gone.
    fn load_scan_cache(path: &std::path::Path) -> Option<(ScanResult, SystemTime)> {
        let data = std::fs::read_to_string(path).ok()?;
        let modified = std::fs::metadata(path).ok()?.modified().ok()?;
        let age = SystemTime::now().duration_since(modified).ok()?;
        if age.as_secs() > SCAN_CACHE_TTL_SECS {
            return None;
        }

        let mut result: ScanResult = serde_json::from_str(&data).ok()?;
        result
            .entries
            .retain(|e| virtual_entry::is_virtual(&e.path) || e.path.symlink_metadata().is_ok());
        result.total_size = result.entries.iter().map(|e| e.size).sum();
        Some((result, modified))
    }

    fn save_scan_cache(&self) {
        let Some(path) = &self.scan_cache else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string(&self.result) {
            let _ = std::fs::write(path, json);
        }
    }
}

#[cfg(test)]
mod tests {

    fn frame_text(app: &mut App) -> String {
        let mut terminal =
            ratatui::Terminal::new(ratatui::backend::TestBackend::new(110, 16)).expect("terminal");
        let total = app.tree.total_reclaimable();
        terminal
            .draw(|f| crate::tui::draw(f, app, total))
            .expect("draw");
        let buffer = terminal.backend().buffer().clone();
        (0..buffer.area.height)
            .map(|y| {
                (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol().to_owned())
                    .collect::<String>()
            })
            .collect::<Vec<_>>()
            .join("\n")
    }

    #[test]
    fn explore_lists_a_directory_by_size_and_says_what_no_rule_covers() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let root = tmp.path().canonicalize().expect("canonical");
        std::fs::create_dir(root.join("models")).expect("dir");
        std::fs::write(root.join("models/weights.bin"), vec![3u8; 512 * 1024]).expect("file");
        std::fs::write(root.join("notes.txt"), b"hello").expect("file");

        let mut app = App::with_scan_cache(Config::default(), None);
        app.explorer = Some(Explorer::open(root.clone()));
        app.view = View::Explore;
        while app.explorer.as_ref().is_some_and(Explorer::measuring) {
            app.check_explorer();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        app.check_explorer();

        let frame = frame_text(&mut app);
        println!("{frame}");
        let models = frame.find("models/").expect("models listed");
        let notes = frame.find("notes.txt").expect("notes listed");
        assert!(models < notes, "largest first");
        assert!(frame.contains("No rule offers this"));

        app.handle_key(KeyEvent::from(KeyCode::Char('d')));
        assert_eq!(app.view, View::Confirm);
        assert_eq!(app.selected_for_deletion[0].path, root.join("models"));
        app.handle_key(KeyEvent::from(KeyCode::Esc));
        assert_eq!(app.view, View::Explore);

        app.handle_key(KeyEvent::from(KeyCode::Char('d')));
        app.handle_key(KeyEvent::from(KeyCode::Char('y')));
        assert_eq!(app.view, View::Deleting);
        while app.deletion.as_ref().is_some_and(|job| !job.is_finished()) {
            app.check_deletion();
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        app.check_deletion();
        app.handle_key(KeyEvent::from(KeyCode::Enter));
        assert_eq!(app.view, View::Explore);
        assert!(!root.join("models").exists());
        let names: Vec<_> = app
            .explorer
            .as_ref()
            .map(|e| e.children.iter().map(|c| c.name.clone()).collect())
            .unwrap_or_default();
        assert_eq!(names, ["notes.txt"]);

        app.handle_key(KeyEvent::from(KeyCode::Char('e')));
        assert_eq!(app.view, View::Main);
        assert!(app.explorer.is_none());
    }

    use super::*;
    use crate::scanner::entry::Category;
    use crate::tui::tree::RowKey;
    use std::path::{Path, PathBuf};

    fn cache(path: &Path, description: &str, size: u64) -> ScannedEntry {
        ScannedEntry {
            path: path.to_path_buf(),
            size,
            category: Category::PackageCache,
            safety: crate::scanner::entry::SafetyLevel::Safe,
            description: description.to_owned(),
            item_count: None,
        }
    }

    fn group(name: &str) -> RowKey {
        RowKey::Group(Category::PackageCache, name.to_owned())
    }

    fn cursor_key(app: &mut App) -> RowKey {
        let row = app.tree.visible_rows()[app.cursor];
        app.tree.row_key(row)
    }

    /// A scan wired to a channel the test drives, so no rule actually runs.
    fn begin_scan(app: &mut App) -> mpsc::Sender<ScanUpdate> {
        let (tx, rx) = mpsc::channel();
        app.scanning = true;
        app.streaming = app.result.entries.is_empty();
        app.scan_buffer = ScanResult::default();
        app.scan_receiver = Some(rx);
        tx
    }

    fn finish(tx: &mpsc::Sender<ScanUpdate>) {
        tx.send(ScanUpdate::Finished {
            duration_secs: 1.0,
            disk_info: None,
        })
        .expect("send");
    }

    #[test]
    fn a_rescan_leaves_the_screen_selection_and_cursor_alone() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let [npm, pip, cargo] = ["npm", "pip", "cargo"].map(|d| {
            let path = tmp.path().join(d);
            std::fs::create_dir(&path).expect("dir");
            path
        });

        let mut app = App::with_scan_cache(Config::default(), None);
        app.result.entries = vec![cache(&npm, "npm cache", 30), cache(&pip, "pip cache", 20)];
        app.rebuild_tree();
        app.cursor = app.tree.position(&group("pip cache")).expect("pip");
        let row = app.tree.visible_rows()[app.cursor];
        app.tree.toggle(row);

        let tx = begin_scan(&mut app);
        tx.send(ScanUpdate::RuleComplete {
            rule_name: "caches",
            entries: vec![
                cache(&cargo, "Cargo cache", 90),
                cache(&npm, "npm cache", 30),
                cache(&pip, "pip cache", 20),
            ],
        })
        .expect("send");
        app.check_scan();

        // Mid-scan: nothing new on screen, nothing moved.
        assert!(app.tree.position(&group("Cargo cache")).is_none());
        assert_eq!(cursor_key(&mut app), group("pip cache"));

        finish(&tx);
        app.check_scan();

        // Finished: the new result is in, and the user's place is kept.
        assert!(app.tree.position(&group("Cargo cache")).is_some());
        assert_eq!(cursor_key(&mut app), group("pip cache"));
        let selected: Vec<PathBuf> = app
            .tree
            .selected_entries()
            .into_iter()
            .map(|e| e.path)
            .collect();
        assert_eq!(selected, [pip]);
    }

    #[test]
    fn a_first_scan_streams_in_without_dropping_selections() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let npm = tmp.path().join("npm");
        let cargo = tmp.path().join("cargo");
        std::fs::create_dir(&npm).expect("npm");
        std::fs::create_dir(&cargo).expect("cargo");

        let mut app = App::with_scan_cache(Config::default(), None);
        let tx = begin_scan(&mut app);
        assert!(app.streaming);

        tx.send(ScanUpdate::RuleComplete {
            rule_name: "npm",
            entries: vec![cache(&npm, "npm cache", 30)],
        })
        .expect("send");
        app.check_scan();
        app.cursor = app.tree.position(&group("npm cache")).expect("npm");
        let row = app.tree.visible_rows()[app.cursor];
        app.tree.toggle(row);

        tx.send(ScanUpdate::RuleComplete {
            rule_name: "cargo",
            entries: vec![cache(&cargo, "Cargo cache", 90)],
        })
        .expect("send");
        app.check_scan();

        assert_eq!(cursor_key(&mut app), group("npm cache"));
        assert_eq!(app.tree.selected_entries().len(), 1);
    }

    #[test]
    fn what_was_deleted_during_a_rescan_does_not_come_back() {
        let tmp = tempfile::tempdir().expect("tempdir");
        let kept = tmp.path().join("kept");
        std::fs::create_dir(&kept).expect("kept");
        let deleted = tmp.path().join("deleted");

        let mut app = App::with_scan_cache(Config::default(), None);
        app.result.entries = vec![cache(&kept, "kept cache", 10)];
        app.rebuild_tree();
        let tx = begin_scan(&mut app);
        tx.send(ScanUpdate::RuleComplete {
            rule_name: "caches",
            entries: vec![cache(&kept, "kept cache", 10), cache(&deleted, "gone", 5)],
        })
        .expect("send");
        finish(&tx);
        app.check_scan();

        assert!(app.result.entries.iter().all(|e| e.path != deleted));
    }
}
