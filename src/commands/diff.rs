use std::path::Path;

use anyhow::Result;
use indexmap::IndexMap;

use crate::scanner::entry::{Category, ScanResult};
use crate::util;

pub fn diff_scans(before_path: &Path, after_path: &Path) -> Result<()> {
    let before_data = std::fs::read_to_string(before_path)?;
    let after_data = std::fs::read_to_string(after_path)?;

    let before: ScanResult = serde_json::from_str(&before_data)?;
    let after: ScanResult = serde_json::from_str(&after_data)?;

    println!("Scan Comparison");
    println!("{}", "=".repeat(60));

    // Overall
    let size_diff = after.total_size as i64 - before.total_size as i64;
    let sign = if size_diff >= 0 { "+" } else { "" };
    println!(
        "Total reclaimable: {} -> {} ({}{})",
        util::human_size(before.total_size),
        util::human_size(after.total_size),
        sign,
        util::human_size(size_diff.unsigned_abs()),
    );
    println!(
        "Items: {} -> {}",
        before.entries.len(),
        after.entries.len(),
    );
    println!();

    // Per-category breakdown
    let before_cats = group_by_category(&before);
    let after_cats = group_by_category(&after);

    let mut all_cats: Vec<Category> = before_cats
        .keys()
        .chain(after_cats.keys())
        .copied()
        .collect();
    all_cats.sort_by_key(|c| {
        Category::ALL
            .iter()
            .position(|a| *a == *c)
            .unwrap_or(usize::MAX)
    });
    all_cats.dedup();

    println!(
        "{:<24} {:>12} {:>12} {:>12}",
        "CATEGORY", "BEFORE", "AFTER", "DIFF"
    );
    println!("{}", "-".repeat(64));

    for cat in &all_cats {
        let b = before_cats.get(cat).copied().unwrap_or(0);
        let a = after_cats.get(cat).copied().unwrap_or(0);
        let d = a as i64 - b as i64;
        let s = if d >= 0 { "+" } else { "" };
        println!(
            "{:<24} {:>12} {:>12} {:>12}",
            cat.to_string(),
            util::human_size(b),
            util::human_size(a),
            format!("{s}{}", util::human_size(d.unsigned_abs())),
        );
    }

    Ok(())
}

fn group_by_category(result: &ScanResult) -> IndexMap<Category, u64> {
    let mut map = IndexMap::new();
    for entry in &result.entries {
        *map.entry(entry.category).or_insert(0u64) += entry.size;
    }
    map
}
