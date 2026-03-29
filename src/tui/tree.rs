use std::path::PathBuf;
use std::rc::Rc;

use indexmap::IndexMap;

use crate::scanner::entry::{Category, SafetyLevel, ScanResult, ScannedEntry};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CheckState {
    Unchecked,
    Partial,
    Checked,
}

pub struct CategoryNode {
    pub category: Category,
    pub groups: Vec<GroupNode>,
    pub expanded: bool,
    pub hidden: bool,
    pub total_size: u64,
    pub entry_count: usize,
}

pub struct GroupNode {
    pub name: String,
    pub entries: Vec<EntryNode>,
    pub expanded: bool,
    pub hidden: bool,
    pub total_size: u64,
    pub safety: SafetyLevel,
}

pub struct EntryNode {
    pub path: PathBuf,
    pub size: u64,
    pub safety: SafetyLevel,
    pub description: String,
    pub item_count: Option<usize>,
    pub checked: bool,
    /// Pre-lowercased "path + description" for fast search filtering.
    search_text: String,
}

pub struct Tree {
    pub categories: Vec<CategoryNode>,
    /// Cached visible rows, shared via Rc to avoid cloning on every access.
    cached_rows: Option<Rc<Vec<RowRef>>>,
    /// Cached check states per category: `(category_check, group_checks)`.
    cached_check_states: Option<Vec<(CheckState, Vec<CheckState>)>>,
    /// Cached selection summary: `(count, total_bytes)`.
    cached_selection: Option<(usize, u64)>,
    /// Cached total reclaimable bytes.
    cached_total_reclaimable: Option<u64>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RowRef {
    Category(usize),
    Group(usize, usize),
    Entry(usize, usize, usize),
}

impl Tree {
    pub fn from_scan_result(result: &ScanResult) -> Self {
        let mut by_category: IndexMap<Category, Vec<&ScannedEntry>> = IndexMap::new();
        for entry in &result.entries {
            by_category.entry(entry.category).or_default().push(entry);
        }

        let mut categories: Vec<CategoryNode> = by_category
            .into_iter()
            .map(|(category, entries)| {
                let total_size: u64 = entries.iter().map(|e| e.size).sum();
                let entry_count = entries.len();

                // Group by description
                let mut by_desc: IndexMap<&str, Vec<&ScannedEntry>> = IndexMap::new();
                for entry in &entries {
                    by_desc.entry(&entry.description).or_default().push(entry);
                }

                let mut groups: Vec<GroupNode> = by_desc
                    .into_iter()
                    .map(|(name, group_entries)| {
                        let group_total: u64 = group_entries.iter().map(|e| e.size).sum();
                        let worst_safety = group_entries
                            .iter()
                            .map(|e| e.safety)
                            .max()
                            .unwrap_or(SafetyLevel::Safe);

                        let mut entry_nodes: Vec<EntryNode> = group_entries
                            .into_iter()
                            .map(|e| {
                                // Pre-compute lowercased search text once at build time
                                let path_str = e.path.display().to_string();
                                let mut search_text = String::with_capacity(
                                    path_str.len() + 1 + e.description.len(),
                                );
                                for c in path_str.chars() {
                                    for lc in c.to_lowercase() {
                                        search_text.push(lc);
                                    }
                                }
                                search_text.push('\0');
                                for c in e.description.chars() {
                                    for lc in c.to_lowercase() {
                                        search_text.push(lc);
                                    }
                                }
                                EntryNode {
                                    path: e.path.clone(),
                                    size: e.size,
                                    safety: e.safety,
                                    description: e.description.clone(),
                                    item_count: e.item_count,
                                    checked: false,
                                    search_text,
                                }
                            })
                            .collect();
                        entry_nodes.sort_by(|a, b| b.size.cmp(&a.size));

                        GroupNode {
                            name: name.to_string(),
                            entries: entry_nodes,
                            expanded: false,
                            hidden: false,
                            total_size: group_total,
                            safety: worst_safety,
                        }
                    })
                    .collect();

                groups.sort_by(|a, b| b.total_size.cmp(&a.total_size));

                CategoryNode {
                    category,
                    groups,
                    expanded: true,
                    hidden: false,
                    total_size,
                    entry_count,
                }
            })
            .collect();

        categories.sort_by(|a, b| b.total_size.cmp(&a.total_size));

        Self {
            categories,
            cached_rows: None,
            cached_check_states: None,
            cached_selection: None,
            cached_total_reclaimable: None,
        }
    }

    /// Invalidate check-state and selection caches (not layout).
    fn invalidate_checks(&mut self) {
        self.cached_check_states = None;
        self.cached_selection = None;
    }

    pub fn apply_search_filter(&mut self, query: &str) {
        self.cached_rows = None;
        if query.is_empty() {
            for cat in &mut self.categories {
                cat.hidden = false;
                for group in &mut cat.groups {
                    group.hidden = false;
                }
            }
            return;
        }
        let query = query.to_lowercase();
        for cat in &mut self.categories {
            let mut any_visible = false;
            for group in &mut cat.groups {
                let matches = group
                    .entries
                    .iter()
                    .any(|e| e.search_text.contains(&*query));
                group.hidden = !matches;
                if matches {
                    any_visible = true;
                }
            }
            cat.hidden = !any_visible;
        }
    }

    /// Returns a shared reference to visible rows. Callers share the same Rc
    /// instead of cloning the Vec on every access.
    pub fn visible_rows(&mut self) -> Rc<Vec<RowRef>> {
        if let Some(ref cached) = self.cached_rows {
            return Rc::clone(cached);
        }
        let mut rows = Vec::new();
        for (ci, cat) in self.categories.iter().enumerate() {
            if cat.hidden {
                continue;
            }
            rows.push(RowRef::Category(ci));
            if cat.expanded {
                for (gi, group) in cat.groups.iter().enumerate() {
                    if group.hidden {
                        continue;
                    }
                    rows.push(RowRef::Group(ci, gi));
                    if group.expanded {
                        for ei in 0..group.entries.len() {
                            rows.push(RowRef::Entry(ci, gi, ei));
                        }
                    }
                }
            }
        }
        let rc = Rc::new(rows);
        self.cached_rows = Some(Rc::clone(&rc));
        rc
    }

    /// Ensure check state cache is populated, then return a reference.
    fn ensure_check_states(&mut self) {
        if self.cached_check_states.is_some() {
            return;
        }
        let states: Vec<(CheckState, Vec<CheckState>)> = self
            .categories
            .iter()
            .map(|cat| {
                let group_states: Vec<CheckState> = cat
                    .groups
                    .iter()
                    .map(compute_group_check_state)
                    .collect();
                let cat_state = compute_category_check_state_from_groups(&group_states, cat);
                (cat_state, group_states)
            })
            .collect();
        self.cached_check_states = Some(states);
    }

    pub fn category_check_state(&mut self, ci: usize) -> CheckState {
        self.ensure_check_states();
        self.cached_check_states.as_ref().unwrap()[ci].0
    }

    pub fn group_check_state(&mut self, ci: usize, gi: usize) -> CheckState {
        self.ensure_check_states();
        self.cached_check_states.as_ref().unwrap()[ci].1[gi]
    }

    /// Populate check state cache if needed. Call before borrowing tree immutably
    /// for rendering (so that `cached_*_check_state` can use `&self`).
    pub fn ensure_check_cache(&mut self) {
        self.ensure_check_states();
    }

    /// Read cached category check state. Falls back to computing on the fly
    /// if the cache hasn't been primed via `ensure_check_cache`.
    pub fn cached_category_check_state(&self, ci: usize) -> CheckState {
        if let Some(ref states) = self.cached_check_states {
            return states[ci].0;
        }
        let cat = &self.categories[ci];
        let group_states: Vec<CheckState> = cat.groups.iter().map(compute_group_check_state).collect();
        compute_category_check_state_from_groups(&group_states, cat)
    }

    /// Read cached group check state. Falls back to computing on the fly
    /// if the cache hasn't been primed via `ensure_check_cache`.
    pub fn cached_group_check_state(&self, ci: usize, gi: usize) -> CheckState {
        if let Some(ref states) = self.cached_check_states {
            return states[ci].1[gi];
        }
        compute_group_check_state(&self.categories[ci].groups[gi])
    }

    pub fn toggle(&mut self, row: RowRef) {
        match row {
            RowRef::Category(ci) => {
                let new_state = self.category_check_state(ci) != CheckState::Checked;
                for group in &mut self.categories[ci].groups {
                    for entry in &mut group.entries {
                        entry.checked = new_state;
                    }
                }
            }
            RowRef::Group(ci, gi) => {
                let new_state = self.group_check_state(ci, gi) != CheckState::Checked;
                for entry in &mut self.categories[ci].groups[gi].entries {
                    entry.checked = new_state;
                }
            }
            RowRef::Entry(ci, gi, ei) => {
                let entry = &mut self.categories[ci].groups[gi].entries[ei];
                entry.checked = !entry.checked;
            }
        }
        self.invalidate_checks();
    }

    pub fn expand(&mut self, row: RowRef) {
        self.cached_rows = None;
        match row {
            RowRef::Category(ci) => self.categories[ci].expanded = true,
            RowRef::Group(ci, gi) => self.categories[ci].groups[gi].expanded = true,
            RowRef::Entry(..) => {}
        }
    }

    pub fn collapse(&mut self, row: RowRef) {
        self.cached_rows = None;
        match row {
            RowRef::Category(ci) => self.categories[ci].expanded = false,
            RowRef::Group(ci, gi) => self.categories[ci].groups[gi].expanded = false,
            RowRef::Entry(..) => {}
        }
    }

    pub fn is_expanded(&self, row: RowRef) -> bool {
        match row {
            RowRef::Category(ci) => self.categories[ci].expanded,
            RowRef::Group(ci, gi) => self.categories[ci].groups[gi].expanded,
            RowRef::Entry(..) => false,
        }
    }

    pub fn parent(row: RowRef) -> Option<RowRef> {
        match row {
            RowRef::Category(_) => None,
            RowRef::Group(ci, _) => Some(RowRef::Category(ci)),
            RowRef::Entry(ci, gi, _) => Some(RowRef::Group(ci, gi)),
        }
    }

    pub fn selected_entries(&self) -> Vec<ScannedEntry> {
        let mut result = Vec::new();
        for cat in &self.categories {
            for group in &cat.groups {
                for entry in &group.entries {
                    if entry.checked {
                        result.push(ScannedEntry {
                            path: entry.path.clone(),
                            size: entry.size,
                            category: cat.category,
                            safety: entry.safety,
                            description: entry.description.clone(),
                            item_count: entry.item_count,
                        });
                    }
                }
            }
        }
        result
    }

    pub fn selection_summary(&mut self) -> (usize, u64) {
        if let Some(cached) = self.cached_selection {
            return cached;
        }
        let mut count = 0;
        let mut total = 0;
        for cat in &self.categories {
            for group in &cat.groups {
                for entry in &group.entries {
                    if entry.checked {
                        count += 1;
                        total += entry.size;
                    }
                }
            }
        }
        let result = (count, total);
        self.cached_selection = Some(result);
        result
    }

    pub fn total_reclaimable(&mut self) -> u64 {
        if let Some(cached) = self.cached_total_reclaimable {
            return cached;
        }
        let total = self.categories.iter().map(|c| c.total_size).sum();
        self.cached_total_reclaimable = Some(total);
        total
    }
}

fn compute_group_check_state(group: &GroupNode) -> CheckState {
    let mut any_checked = false;
    let mut any_unchecked = false;
    for entry in &group.entries {
        if entry.checked {
            any_checked = true;
        } else {
            any_unchecked = true;
        }
        if any_checked && any_unchecked {
            return CheckState::Partial;
        }
    }
    if any_checked {
        CheckState::Checked
    } else {
        CheckState::Unchecked
    }
}

fn compute_category_check_state_from_groups(
    group_states: &[CheckState],
    cat: &CategoryNode,
) -> CheckState {
    if cat.groups.is_empty() {
        return CheckState::Unchecked;
    }
    let mut any_checked = false;
    let mut any_unchecked = false;
    for &state in group_states {
        match state {
            CheckState::Partial => return CheckState::Partial,
            CheckState::Checked => any_checked = true,
            CheckState::Unchecked => any_unchecked = true,
        }
        if any_checked && any_unchecked {
            return CheckState::Partial;
        }
    }
    if any_checked {
        CheckState::Checked
    } else {
        CheckState::Unchecked
    }
}
