// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Calibration: reads benchmark CSV results and computes per-transform
//! win-rates grouped by file extension.  Emits a `calibration.json` that
//! the analyzer can consume at compile-time or runtime.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Per-transform statistics for a given grouping (overall or per-extension).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformStats {
    pub files: usize,
    pub win_count: usize,
    pub loss_count: usize,
    pub neutral_count: usize,
    pub total_gain_bytes: i64,
    pub avg_gain_bytes: f64,
    pub win_rate: f64,
}

/// All stats for a single transform.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransformCalibration {
    pub overall: TransformStats,
    pub by_extension: BTreeMap<String, TransformStats>,
}

/// Top-level calibration output.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Calibration {
    pub version: u32,
    pub generated: String,
    pub csv_files: Vec<String>,
    pub total_rows: usize,
    pub transforms: BTreeMap<String, TransformCalibration>,
}

// ---------------------------------------------------------------------------
// CSV row (matching report::print_csv_header format)
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct CsvRow {
    ext: String,
    trial: String,
    net_gain: i64,
}

// ---------------------------------------------------------------------------
// Implementation
// ---------------------------------------------------------------------------

/// Discover all `.csv` files in `dir`.
pub fn discover_csvs(dir: &Path) -> Vec<PathBuf> {
    let mut csvs = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().and_then(|e| e.to_str()) == Some("csv") {
                csvs.push(p);
            }
        }
    }
    csvs.sort();
    csvs
}

/// Parse a single CSV file into rows, skipping baseline and error rows.
fn parse_csv(path: &Path) -> Vec<CsvRow> {
    let content = match std::fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Warning: cannot read {}: {e}", path.display());
            return Vec::new();
        }
    };

    let mut rows = Vec::new();
    let mut lines = content.lines();
    // Skip header
    let _header = lines.next();

    for line in lines {
        let fields: Vec<&str> = line.split(',').collect();
        if fields.len() < 13 {
            continue;
        }
        let trial = fields[4].to_string();
        // Skip baseline rows — they have net_gain == 0 by definition
        if trial == "baseline" {
            continue;
        }
        // Skip rows with errors
        if !fields[12].is_empty() {
            continue;
        }
        let net_gain: i64 = match fields[8].parse() {
            Ok(v) => v,
            Err(_) => continue,
        };
        let ext = fields[1].to_string();
        rows.push(CsvRow {
            ext,
            trial,
            net_gain,
        });
    }
    rows
}

/// Build a stats accumulator from a set of (net_gain) values.
fn compute_stats(gains: &[i64]) -> TransformStats {
    let files = gains.len();
    let win_count = gains.iter().filter(|g| **g > 0).count();
    let loss_count = gains.iter().filter(|g| **g < 0).count();
    let neutral_count = gains.iter().filter(|g| **g == 0).count();
    let total_gain_bytes: i64 = gains.iter().sum();
    let avg_gain_bytes = if files > 0 {
        total_gain_bytes as f64 / files as f64
    } else {
        0.0
    };
    let win_rate = if files > 0 {
        win_count as f64 / files as f64
    } else {
        0.0
    };
    TransformStats {
        files,
        win_count,
        loss_count,
        neutral_count,
        total_gain_bytes,
        avg_gain_bytes,
        win_rate,
    }
}

/// Run calibration on all CSVs in the given directory.
///
/// Returns a [`Calibration`] with per-transform stats.
pub fn calibrate(dir: &Path) -> Calibration {
    let csvs = discover_csvs(dir);
    let csv_names: Vec<String> = csvs
        .iter()
        .filter_map(|p| p.file_name())
        .map(|n| n.to_string_lossy().to_string())
        .collect();

    // Collect all rows
    let mut all_rows = Vec::new();
    for csv in &csvs {
        all_rows.extend(parse_csv(csv));
    }
    let total_rows = all_rows.len();

    // Group by trial name → Vec<(ext, net_gain)>
    let mut by_trial: BTreeMap<String, Vec<(String, i64)>> = BTreeMap::new();
    for row in &all_rows {
        by_trial
            .entry(row.trial.clone())
            .or_default()
            .push((row.ext.clone(), row.net_gain));
    }

    // Build per-transform calibration
    let mut transforms = BTreeMap::new();
    for (trial_name, entries) in &by_trial {
        // Overall stats
        let all_gains: Vec<i64> = entries.iter().map(|(_, g)| *g).collect();
        let overall = compute_stats(&all_gains);

        // By extension
        let mut ext_groups: BTreeMap<String, Vec<i64>> = BTreeMap::new();
        for (ext, gain) in entries {
            let key = if ext.is_empty() {
                "(none)".to_string()
            } else {
                ext.clone()
            };
            ext_groups.entry(key).or_default().push(*gain);
        }
        let by_extension: BTreeMap<String, TransformStats> = ext_groups
            .iter()
            .map(|(ext, gains)| (ext.clone(), compute_stats(gains)))
            .collect();

        transforms.insert(
            trial_name.clone(),
            TransformCalibration {
                overall,
                by_extension,
            },
        );
    }

    let generated = chrono_lite_now();

    Calibration {
        version: 1,
        generated,
        csv_files: csv_names,
        total_rows,
        transforms,
    }
}

/// Simple ISO-8601 timestamp without pulling in the chrono crate.
fn chrono_lite_now() -> String {
    use std::time::SystemTime;
    let dur = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = dur.as_secs();
    // Approximate: good enough for a generated-at timestamp
    let days = secs / 86400;
    let time_of_day = secs % 86400;
    let hours = time_of_day / 3600;
    let minutes = (time_of_day % 3600) / 60;
    let seconds = time_of_day % 60;

    // Approximate date from epoch days (good enough for 2020-2040)
    let (year, month, day) = epoch_days_to_ymd(days);
    format!("{year:04}-{month:02}-{day:02}T{hours:02}:{minutes:02}:{seconds:02}Z")
}

fn epoch_days_to_ymd(mut days: u64) -> (u64, u64, u64) {
    // Simple leap-year-aware conversion
    let mut year = 1970;
    loop {
        let days_in_year = if is_leap(year) { 366 } else { 365 };
        if days < days_in_year {
            break;
        }
        days -= days_in_year;
        year += 1;
    }
    let month_days: [u64; 12] = if is_leap(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };
    let mut month = 1;
    for &md in &month_days {
        if days < md {
            break;
        }
        days -= md;
        month += 1;
    }
    (year, month, days + 1)
}

fn is_leap(y: u64) -> bool {
    (y.is_multiple_of(4) && !y.is_multiple_of(100)) || y.is_multiple_of(400)
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_test_csv(dir: &Path) -> PathBuf {
        let path = dir.join("test.csv");
        let mut f = std::fs::File::create(&path).unwrap();
        writeln!(f, "file,ext,rel_dir,top_dir,trial,raw_size,z_baseline,z_after,net_gain,transform_us,applied,msn_domain,error").unwrap();
        writeln!(f, "a.txt,txt,,(root),baseline,1000,500,500,0,0,false,,").unwrap();
        writeln!(f, "a.txt,txt,,(root),normalize,1000,500,400,100,10,true,,").unwrap();
        writeln!(f, "a.txt,txt,,(root),delta,1000,500,600,-100,10,true,,").unwrap();
        writeln!(f, "b.json,json,,(root),baseline,2000,800,800,0,0,false,,").unwrap();
        writeln!(f, "b.json,json,,(root),normalize,2000,800,700,100,20,true,,").unwrap();
        writeln!(f, "b.json,json,,(root),delta,2000,800,900,-100,20,true,,").unwrap();
        writeln!(f, "c.txt,txt,,(root),baseline,500,300,300,0,0,false,,").unwrap();
        writeln!(f, "c.txt,txt,,(root),normalize,500,300,350,-50,5,true,,").unwrap();
        writeln!(f, "c.txt,txt,,(root),delta,500,300,250,50,5,true,,").unwrap();
        // Row with error — should be skipped
        writeln!(f, "d.bin,,,(root),normalize,100,90,90,0,1,false,,transform error: foo").unwrap();
        path
    }

    #[test]
    fn calibrate_basic() {
        let tmp = std::env::temp_dir().join("cpac_calibrate_test");
        let _ = std::fs::create_dir_all(&tmp);
        write_test_csv(&tmp);

        let cal = calibrate(&tmp);
        assert_eq!(cal.version, 1);
        assert!(!cal.csv_files.is_empty());

        // normalize: 2 wins (a.txt +100, b.json +100), 1 loss (c.txt -50)
        let norm = &cal.transforms["normalize"];
        assert_eq!(norm.overall.files, 3);
        assert_eq!(norm.overall.win_count, 2);
        assert_eq!(norm.overall.loss_count, 1);
        assert_eq!(norm.overall.total_gain_bytes, 150);
        assert!((norm.overall.win_rate - 2.0 / 3.0).abs() < 0.01);

        // delta: 1 win (c.txt +50), 2 losses
        let delta = &cal.transforms["delta"];
        assert_eq!(delta.overall.files, 3);
        assert_eq!(delta.overall.win_count, 1);
        assert_eq!(delta.overall.loss_count, 2);
        assert_eq!(delta.overall.total_gain_bytes, -150);

        // normalize by_extension
        let norm_txt = &norm.by_extension["txt"];
        assert_eq!(norm_txt.files, 2);
        assert_eq!(norm_txt.win_count, 1);

        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn discover_csvs_empty() {
        let tmp = std::env::temp_dir().join("cpac_cal_empty");
        let _ = std::fs::create_dir_all(&tmp);
        let csvs = discover_csvs(&tmp);
        // May or may not be empty depending on prior test runs
        assert!(csvs.is_empty() || csvs.iter().all(|p| p.extension().unwrap() == "csv"));
        let _ = std::fs::remove_dir_all(&tmp);
    }

    #[test]
    fn compute_stats_edge_cases() {
        let empty = compute_stats(&[]);
        assert_eq!(empty.files, 0);
        assert_eq!(empty.win_rate, 0.0);

        let all_wins = compute_stats(&[10, 20, 30]);
        assert_eq!(all_wins.win_count, 3);
        assert_eq!(all_wins.win_rate, 1.0);

        let all_losses = compute_stats(&[-10, -20]);
        assert_eq!(all_losses.loss_count, 2);
        assert_eq!(all_losses.win_rate, 0.0);
    }
}
