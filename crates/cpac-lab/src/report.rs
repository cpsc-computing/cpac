// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Report formatting for experiment results.

use crate::experiment::GroupBy;
use crate::runner::{aggregate_results, GroupResult, TrialResult};

// ---------------------------------------------------------------------------
// CSV output
// ---------------------------------------------------------------------------

/// Print CSV header for per-file results.
pub fn print_csv_header() {
    println!(
        "file,ext,rel_dir,top_dir,trial,raw_size,z_baseline,z_after,\
         net_gain,transform_us,applied,msn_domain,error"
    );
}

/// Print a single CSV row.
pub fn print_csv_row(r: &TrialResult) {
    println!(
        "{},{},{},{},{},{},{},{},{},{},{},{},{}",
        r.file_name,
        r.extension,
        r.rel_dir,
        r.top_dir,
        r.trial_name,
        r.raw_size,
        r.z_baseline,
        r.z_after,
        r.net_gain,
        r.transform_us,
        r.applied,
        r.msn_domain.as_deref().unwrap_or(""),
        r.error.as_deref().unwrap_or(""),
    );
}

// ---------------------------------------------------------------------------
// Text summary
// ---------------------------------------------------------------------------

/// Print grouped summary tables.
pub fn print_summary(results: &[TrialResult], group_by: &[GroupBy]) {
    // Per-trial totals
    print_trial_totals(results);

    for grouping in group_by {
        match grouping {
            GroupBy::Extension => {
                let groups = aggregate_results(results, |r| {
                    if r.extension.is_empty() {
                        "(none)".to_string()
                    } else {
                        r.extension.clone()
                    }
                });
                print_group_table("BY EXTENSION", &groups);
            }
            GroupBy::Domain => {
                let groups = aggregate_results(results, |r| {
                    r.msn_domain
                        .clone()
                        .unwrap_or_else(|| "NO_DOMAIN".to_string())
                });
                print_group_table("BY DOMAIN", &groups);
            }
            GroupBy::Directory => {
                let groups = aggregate_results(results, |r| r.top_dir.clone());
                print_group_table("BY DIRECTORY", &groups);
            }
        }
    }
}

fn print_trial_totals(results: &[TrialResult]) {
    let groups = aggregate_results(results, |_| "(all)".to_string());

    println!("\n{}", "═".repeat(90));
    println!("TRIAL TOTALS");
    println!("{}", "═".repeat(90));
    println!(
        "{:<20}  {:>6}  {:>6}  {:>6}  {:>12}  {:>12}  {:>10}",
        "Trial", "Files", "Appld", "Helpd", "z_baseline", "z_after", "net_gain"
    );
    println!("{}", "─".repeat(90));

    for g in &groups {
        println!(
            "{:<20}  {:>6}  {:>6}  {:>6}  {:>12}  {:>12}  {:>+10}",
            g.trial_name,
            g.file_count,
            g.applied_count,
            g.helped_count,
            g.total_z_baseline,
            g.total_z_after,
            g.total_net_gain,
        );
    }
}

fn print_group_table(title: &str, groups: &[GroupResult]) {
    if groups.is_empty() {
        return;
    }

    println!("\n{}", "═".repeat(100));
    println!("{title}");
    println!("{}", "═".repeat(100));
    println!(
        "{:<20}  {:<16}  {:>6}  {:>6}  {:>6}  {:>6}  {:>10}  {:>10}  {:>10}",
        "Key", "Trial", "Files", "Appld", "Help", "Hurt", "net_gain", "best", "worst"
    );
    println!("{}", "─".repeat(100));

    for g in groups {
        println!(
            "{:<20}  {:<16}  {:>6}  {:>6}  {:>6}  {:>6}  {:>+10}  {:>+10}  {:>+10}",
            truncate_str(&g.key, 20),
            truncate_str(&g.trial_name, 16),
            g.file_count,
            g.applied_count,
            g.helped_count,
            g.hurt_count,
            g.total_net_gain,
            g.best_gain,
            g.worst_gain,
        );
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        let end = s
            .char_indices()
            .nth(max - 1)
            .map(|(i, _)| i)
            .unwrap_or(s.len());
        format!("{}…", &s[..end])
    }
}
