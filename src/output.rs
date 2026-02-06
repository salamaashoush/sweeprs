use indexmap::IndexMap;
use yansi::Paint;

use crate::scanner::entry::{Category, DiskInfo, SafetyLevel, ScanResult, ScannedEntry};
use crate::util;



pub fn print_table(result: &ScanResult) {
    print_header(result);

    if let Some(ref disk) = result.disk_info {
        print_disk_info(disk);
        println!();
    }

    let grouped = group_by_category(&result.entries);

    if grouped.is_empty() {
        println!("{}", "No items found.".dim());
        return;
    }

    println!("{}", "─".repeat(78).dim());

    for (category, entries) in &grouped {
        let cat_total: u64 = entries.iter().map(|e| e.size).sum();
        let item_count: usize = entries.len();
        let pct = if result.total_size > 0 {
            (cat_total as f64 / result.total_size as f64) * 100.0
        } else {
            0.0
        };

        println!(
            "{:<24} {:>10}  ({:>4.1}%)  {} items",
            category.to_string().bold(),
            util::human_size(cat_total).bold(),
            pct,
            item_count,
        );

        // Group entries by description within this category
        let mut desc_groups: IndexMap<&str, Vec<&ScannedEntry>> = IndexMap::new();
        for entry in entries {
            desc_groups
                .entry(&entry.description)
                .or_default()
                .push(entry);
        }
        // Sort each group internally by size desc
        for group in desc_groups.values_mut() {
            group.sort_by(|a, b| b.size.cmp(&a.size));
        }
        // Sort groups by total size descending
        desc_groups.sort_by(|_, a, _, b| {
            let a_total: u64 = a.iter().map(|e| e.size).sum();
            let b_total: u64 = b.iter().map(|e| e.size).sum();
            b_total.cmp(&a_total)
        });

        for (_desc, group) in &desc_groups {
            for entry in group {
                print_entry(entry);
            }
        }
        println!();
    }

    print_footer(result, &grouped);
}

pub fn print_json(result: &ScanResult) -> anyhow::Result<()> {
    let json = serde_json::to_string_pretty(result)?;
    println!("{json}");
    Ok(())
}

fn print_entry(entry: &ScannedEntry) {
    let safety_indicator = match entry.safety {
        SafetyLevel::Safe => "[Safe]".green(),
        SafetyLevel::Caution => "[Caution]".yellow(),
        SafetyLevel::Danger => "[Danger]".red(),
    };
    println!(
        "  {} {:>10}  {}  {}",
        safety_indicator,
        util::human_size(entry.size),
        entry.description,
        util::tilde_path(&entry.path).dim()
    );
}

fn print_header(result: &ScanResult) {
    let now = chrono::Local::now();
    let timestamp = now.format("%Y-%m-%d %H:%M:%S");
    let duration_str = result
        .scan_duration_secs
        .map(|d| format!(" in {d:.1}s"))
        .unwrap_or_default();

    println!(
        "{}  sweeprs scan  {}{}",
        ">>>".cyan().bold(),
        timestamp.to_string().dim(),
        duration_str.dim(),
    );
}

fn print_disk_info(disk: &DiskInfo) {
    let bar_width = 40;
    let filled = (disk.usage_percent / 100.0 * bar_width as f64) as usize;
    let empty = bar_width - filled;

    let bar_color = if disk.usage_percent >= 95.0 {
        yansi::Color::Red
    } else if disk.usage_percent >= 85.0 {
        yansi::Color::Yellow
    } else {
        yansi::Color::Green
    };

    print!("Disk: {} [", disk.name.bold());
    print!("{}", "#".repeat(filled).fg(bar_color));
    print!("{}", ".".repeat(empty).dim());
    println!(
        "] {:.1}% ({} / {})",
        disk.usage_percent,
        util::human_size(disk.used_bytes),
        util::human_size(disk.total_bytes),
    );
    println!(
        "Available: {}",
        util::human_size(disk.available_bytes).green()
    );
    if let Some(purgeable) = disk.purgeable_bytes {
        println!(
            "Purgeable: {}",
            util::human_size(purgeable).green()
        );
    }
    if disk.snapshot_bytes > 0 {
        println!(
            "Snapshots: {}",
            util::human_size(disk.snapshot_bytes).yellow()
        );
    }
}

fn print_footer(result: &ScanResult, grouped: &IndexMap<Category, Vec<ScannedEntry>>) {
    let total_items: usize = result.entries.len();
    let cat_count = grouped.len();
    let duration_str = result
        .scan_duration_secs
        .map(|d| format!("  scanned in {d:.1}s"))
        .unwrap_or_default();

    println!("{}", "─".repeat(78).dim());
    println!(
        "{} categories, {} items, {} reclaimable{}",
        cat_count,
        total_items,
        util::human_size(result.total_size).bold().green(),
        duration_str.dim(),
    );
}

fn group_by_category(entries: &[ScannedEntry]) -> IndexMap<Category, Vec<ScannedEntry>> {
    let mut map: IndexMap<Category, Vec<ScannedEntry>> = IndexMap::new();
    for entry in entries {
        map.entry(entry.category).or_default().push(entry.clone());
    }
    // Sort categories by total size descending
    map.sort_by(|_, a, _, b| {
        let a_total: u64 = a.iter().map(|e| e.size).sum();
        let b_total: u64 = b.iter().map(|e| e.size).sum();
        b_total.cmp(&a_total)
    });
    map
}
