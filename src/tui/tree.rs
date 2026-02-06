use std::path::PathBuf;

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
    pub total_size: u64,
    pub entry_count: usize,
}

pub struct GroupNode {
    pub name: String,
    pub entries: Vec<EntryNode>,
    pub expanded: bool,
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
}

pub struct Tree {
    pub categories: Vec<CategoryNode>,
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
                            .map(|e| EntryNode {
                                path: e.path.clone(),
                                size: e.size,
                                safety: e.safety,
                                description: e.description.clone(),
                                item_count: e.item_count,
                                checked: false,
                            })
                            .collect();
                        entry_nodes.sort_by(|a, b| b.size.cmp(&a.size));

                        GroupNode {
                            name: name.to_string(),
                            entries: entry_nodes,
                            expanded: false,
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
                    total_size,
                    entry_count,
                }
            })
            .collect();

        categories.sort_by(|a, b| b.total_size.cmp(&a.total_size));

        Self { categories }
    }

    pub fn visible_rows(&self) -> Vec<RowRef> {
        let mut rows = Vec::new();
        for (ci, cat) in self.categories.iter().enumerate() {
            rows.push(RowRef::Category(ci));
            if cat.expanded {
                for (gi, group) in cat.groups.iter().enumerate() {
                    rows.push(RowRef::Group(ci, gi));
                    if group.expanded {
                        for ei in 0..group.entries.len() {
                            rows.push(RowRef::Entry(ci, gi, ei));
                        }
                    }
                }
            }
        }
        rows
    }

    pub fn category_check_state(&self, ci: usize) -> CheckState {
        let cat = &self.categories[ci];
        let mut any_checked = false;
        let mut any_unchecked = false;
        for group in &cat.groups {
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
        }
        if any_checked {
            CheckState::Checked
        } else {
            CheckState::Unchecked
        }
    }

    pub fn group_check_state(&self, ci: usize, gi: usize) -> CheckState {
        let group = &self.categories[ci].groups[gi];
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
    }

    pub fn expand(&mut self, row: RowRef) {
        match row {
            RowRef::Category(ci) => self.categories[ci].expanded = true,
            RowRef::Group(ci, gi) => self.categories[ci].groups[gi].expanded = true,
            RowRef::Entry(..) => {}
        }
    }

    pub fn collapse(&mut self, row: RowRef) {
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

    pub fn selection_summary(&self) -> (usize, u64) {
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
        (count, total)
    }

    pub fn total_reclaimable(&self) -> u64 {
        self.categories.iter().map(|c| c.total_size).sum()
    }
}
