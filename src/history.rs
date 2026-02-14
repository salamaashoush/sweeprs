use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};

use crate::scanner::entry::{Category, ScanResult};
use crate::util;

const MAX_RECORDS: usize = 100;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanRecord {
    pub timestamp: String,
    pub total_reclaimable: u64,
    pub item_count: usize,
    pub disk_used: u64,
    pub disk_total: u64,
    pub disk_available: u64,
    pub scan_duration_secs: Option<f64>,
    pub category_breakdown: Vec<CategorySummary>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategorySummary {
    pub category: Category,
    pub size: u64,
    pub count: usize,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ScanHistory {
    pub records: Vec<ScanRecord>,
}

fn history_path() -> PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| PathBuf::from("~/.config"))
        .join("sweeprs")
        .join("history.json")
}

impl ScanHistory {
    pub fn load() -> Self {
        let path = history_path();
        if !path.exists() {
            return Self::default();
        }
        std::fs::read_to_string(&path)
            .ok()
            .and_then(|data| serde_json::from_str(&data).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<()> {
        let path = history_path();
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, json)?;
        Ok(())
    }

    pub fn add_record(&mut self, record: ScanRecord) {
        self.records.push(record);
        // Keep only the last MAX_RECORDS entries
        if self.records.len() > MAX_RECORDS {
            let drain_count = self.records.len() - MAX_RECORDS;
            self.records.drain(..drain_count);
        }
    }
}

/// Create a `ScanRecord` from a completed scan result.
pub fn record_from_result(result: &ScanResult) -> ScanRecord {
    let now = chrono::Local::now();

    // Build category breakdown
    let mut cat_map: rustc_hash::FxHashMap<Category, (u64, usize)> =
        rustc_hash::FxHashMap::default();
    for entry in &result.entries {
        let (size, count) = cat_map.entry(entry.category).or_default();
        *size += entry.size;
        *count += 1;
    }
    let mut category_breakdown: Vec<CategorySummary> = cat_map
        .into_iter()
        .map(|(category, (size, count))| CategorySummary {
            category,
            size,
            count,
        })
        .collect();
    category_breakdown.sort_by(|a, b| b.size.cmp(&a.size));

    let (disk_used, disk_total, disk_available) = result
        .disk_info
        .as_ref()
        .map(|d| (d.used_bytes, d.total_bytes, d.available_bytes))
        .unwrap_or((0, 0, 0));

    ScanRecord {
        timestamp: now.format("%Y-%m-%d %H:%M:%S").to_string(),
        total_reclaimable: result.total_size,
        item_count: result.entries.len(),
        disk_used,
        disk_total,
        disk_available,
        scan_duration_secs: result.scan_duration_secs,
        category_breakdown,
    }
}

/// Save a scan result to history.
pub fn save_scan(result: &ScanResult) {
    let record = record_from_result(result);
    let mut history = ScanHistory::load();
    history.add_record(record);
    let _ = history.save();
}

/// Print scan history summary to the terminal.
pub fn print_history() {
    let history = ScanHistory::load();
    if history.records.is_empty() {
        println!("No scan history yet. Run `sweeprs scan` to start tracking.");
        return;
    }

    println!("{:<20} {:>12} {:>8} {:>12} {:>12}",
        "TIMESTAMP", "RECLAIMABLE", "ITEMS", "DISK USED", "AVAILABLE"
    );
    println!("{}", "-".repeat(68));

    for record in &history.records {
        println!(
            "{:<20} {:>12} {:>8} {:>12} {:>12}",
            record.timestamp,
            util::human_size(record.total_reclaimable),
            record.item_count,
            util::human_size(record.disk_used),
            util::human_size(record.disk_available),
        );
    }

    // Show trend if we have at least 2 records
    if history.records.len() >= 2 {
        let latest = &history.records[history.records.len() - 1];
        let previous = &history.records[history.records.len() - 2];

        println!();
        println!("Trend (vs previous scan):");

        let reclaim_diff = latest.total_reclaimable as i64 - previous.total_reclaimable as i64;
        let disk_diff = latest.disk_used as i64 - previous.disk_used as i64;

        let sign = |v: i64| if v >= 0 { "+" } else { "" };

        println!(
            "  Reclaimable: {}{} ({} -> {})",
            sign(reclaim_diff),
            util::human_size(reclaim_diff.unsigned_abs()),
            util::human_size(previous.total_reclaimable),
            util::human_size(latest.total_reclaimable),
        );
        println!(
            "  Disk used:   {}{} ({} -> {})",
            sign(disk_diff),
            util::human_size(disk_diff.unsigned_abs()),
            util::human_size(previous.disk_used),
            util::human_size(latest.disk_used),
        );
    }
}
