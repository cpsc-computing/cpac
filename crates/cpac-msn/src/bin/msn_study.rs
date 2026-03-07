// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! MSN Exploit Study — measures the true compressed-space cost of each
//! extraction technique on the benchmark corpus.
//!
//! Key metrics per file:
//!   z_orig        — baseline: zstd(original)
//!   z_combined    — MSN inline: zstd(metadata || residual)  [the real pipeline]
//!   z_residual    — hypothetical: zstd(residual) with 0 metadata cost
//!   net_gain      — z_orig - z_combined  (positive = MSN helps)
//!   meta_z_cost   — z_combined - z_residual  (what metadata actually costs)
//!   resid_z_sav   — z_orig - z_residual   (max achievable savings)
//!
//! Per-field marginal cost:
//!   For each field F, remove it from the metadata (keep same residual) and
//!   re-compress.  marginal_cost = z_combined_full - z_combined_without_F.
//!   Positive = keeping F saves that many bytes.  Negative = F is hurting.

use clap::Parser;
use cpac_msn::{encode_metadata_compact, extract, MsnMetadata};
use std::collections::HashMap;
use std::io::Write;
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// Extension whitelists
// ---------------------------------------------------------------------------

/// Extensions we treat as text-like and will pass through MSN analysis.
const TEXT_EXTENSIONS: &[&str] = &[
    "log", "txt", "text", "json", "jsonl", "yaml", "yml", "csv", "xml",
    "html", "htm", "sql", "rtf", "conf", "cnf", "tf", "java", "c", "h",
    "sh", "bash", "py", "rs", "go", "tex", "sgml", "ps", "eps", "kml",
    "pl", "f", "unk", "dump", "script", "lsp", "dist", "disabled", "3",
    "1", "0",
];

/// Extensions that are definitively binary — skip entirely.
const BINARY_EXTENSIONS: &[&str] = &[
    "flac", "wav", "mp3", "ogg", "aac", "mp4", "avi", "mkv", "mov",
    "jpg", "jpeg", "png", "gif", "bmp", "tiff", "webp",
    "pdf", "doc", "docx", "ppt", "pptx", "xls", "xlsx", "pub", "pps",
    "gz", "zip", "tar", "bz2", "xz", "7z", "rar", "cpac",
    "bin", "so", "dll", "exe", "obj", "o",
    "fits", "swf", "kmz", "wp", "hlp", "dbase3",
];

fn is_text_extension(ext: &str) -> bool {
    TEXT_EXTENSIONS.contains(&ext)
}

fn is_binary_extension(ext: &str) -> bool {
    BINARY_EXTENSIONS.contains(&ext)
}

// ---------------------------------------------------------------------------
// CLI args
// ---------------------------------------------------------------------------

#[derive(Parser, Debug)]
#[command(name = "msn_study", about = "MSN exploit study: measure field-level compression ROI")]
struct Args {
    /// Corpus directory (defaults to the loghub-2.0 2k benchmark set)
    #[arg(
        default_value = r"C:\Users\trist\Development\BitConcepts\cpac\.work\benchdata\logs\loghub-2.0\2k"
    )]
    corpus: PathBuf,

    /// zstd compression level to use for measurements (default 3)
    #[arg(short = 'l', long, default_value_t = 3)]
    level: i32,

    /// Minimum MSN confidence threshold (default 0.3)
    #[arg(short = 'c', long, default_value_t = 0.30)]
    confidence: f64,

    /// Maximum file size in MB to process (0 = no limit, default 100)
    #[arg(long, default_value_t = 100)]
    max_size_mb: u64,

    /// Process all file types, not just the text-extension whitelist
    #[arg(long)]
    all_types: bool,

    /// Do not recurse into subdirectories
    #[arg(long)]
    no_recursive: bool,

    /// Show only per-file reports for files where MSN was applied
    #[arg(long)]
    applied_only: bool,

    /// Suppress per-file detail reports; show only summary
    #[arg(long)]
    quiet: bool,

    /// Output CSV for each file to stdout (for scripted analysis)
    #[arg(long)]
    csv: bool,
}

// ---------------------------------------------------------------------------
// zstd helper
// ---------------------------------------------------------------------------

fn zstd_compress(data: &[u8], level: i32) -> Vec<u8> {
    zstd::encode_all(std::io::Cursor::new(data), level)
        .expect("zstd encode failed")
}

// ---------------------------------------------------------------------------
// Field analysis helpers
// ---------------------------------------------------------------------------

/// Build a metadata struct with all fields except `exclude_key`.
fn meta_without_field(meta: &MsnMetadata, exclude_key: &str) -> Vec<u8> {
    let mut m = meta.clone();
    m.fields.remove(exclude_key);
    encode_metadata_compact(&m).expect("encode failed")
}

/// Human-readable description of a field value for the report.
fn describe_field(val: &serde_json::Value) -> String {
    match val {
        serde_json::Value::Array(a) => {
            let typ = if a.first().is_some_and(|v| v.is_string()) {
                "str"
            } else {
                "num"
            };
            let sample = if typ == "str" {
                a.first()
                    .and_then(|v| v.as_str())
                    .map(|s| {
                        let end = s.char_indices().nth(16).map(|(i, _)| i).unwrap_or(s.len());
                        format!(" \"{}\"…", &s[..end])
                    })
                    .unwrap_or_default()
            } else {
                let nums: Vec<i64> = a.iter().filter_map(|v| v.as_i64()).collect();
                if nums.is_empty() {
                    String::new()
                } else {
                    let mn = nums.iter().copied().min().unwrap_or(0);
                    let mx = nums.iter().copied().max().unwrap_or(0);
                    format!(" [{mn}…{mx}]")
                }
            };
            format!("[{}×{}]{}", a.len(), typ, sample)
        }
        serde_json::Value::String(s) => {
            let end = s.char_indices().nth(24).map(|(i, _)| i).unwrap_or(s.len());
            format!("\"{}\"", &s[..end])
        }
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        _ => "null".to_string(),
    }
}

/// Approximate raw bytes removed from residual by this field's extraction.
fn estimate_residual_removal(key: &str, val: &serde_json::Value) -> Option<usize> {
    match key {
        "epoch_deltas" => {
            let n = val.as_array().map(|a| a.len()).unwrap_or(0);
            Some(n * 8) // 10-char epoch minus 2-char @E = 8
        }
        "datetime_micros" => {
            let n = val.as_array().map(|a| a.len()).unwrap_or(0);
            Some(n * 24) // 26-char BGL datetime minus 2-char @D = 24
        }
        "ts_prefix" => {
            let pfx_len = val.as_str().map(|s| s.len()).unwrap_or(0);
            Some(pfx_len) // per-line savings (multiply by line count manually)
        }
        _ => {
            if let serde_json::Value::Array(arr) = val {
                if arr.first().is_some_and(|v| v.is_string()) {
                    let saving: usize = arr
                        .iter()
                        .filter_map(|v| v.as_str())
                        .map(|s| s.len().saturating_sub(3))
                        .sum();
                    return Some(saving);
                }
            }
            None
        }
    }
}

// ---------------------------------------------------------------------------
// Core study logic
// ---------------------------------------------------------------------------

#[derive(Debug)]
struct FileStudy {
    name: String,
    rel_dir: String,    // relative directory from corpus root (for grouping)
    extension: String,
    orig_raw: usize,
    z_orig: usize,
    domain: Option<String>,
    confidence: f64,
    applied: bool,
    // Set only when applied:
    residual_raw: usize,
    meta_raw: usize,
    z_residual: usize,
    z_combined: usize,
    fields: HashMap<String, FieldStudy>,
    bypass_raw_ok: bool,
}

#[derive(Debug)]
struct FieldStudy {
    #[allow(dead_code)]
    key: String,
    raw_meta_contribution: usize,
    marginal_z_cost: i64,
    estimated_residual_removal: Option<usize>,
    description: String,
}

fn study_file(path: &Path, corpus_root: &Path, level: i32, min_conf: f64) -> FileStudy {
    let data = std::fs::read(path).unwrap_or_default();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("?")
        .to_string();
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
        .unwrap_or_default();
    let rel_dir = path
        .strip_prefix(corpus_root)
        .ok()
        .and_then(|p| p.parent())
        .and_then(|p| p.to_str())
        .unwrap_or("")
        .to_string();

    let z_orig = zstd_compress(&data, level);

    let msn = match extract(&data, path.to_str(), min_conf) {
        Ok(r) => r,
        Err(_) => {
            return FileStudy {
                name,
                rel_dir,
                extension,
                orig_raw: data.len(),
                z_orig: z_orig.len(),
                domain: None,
                confidence: 0.0,
                applied: false,
                residual_raw: 0,
                meta_raw: 0,
                z_residual: 0,
                z_combined: 0,
                fields: HashMap::new(),
                bypass_raw_ok: false,
            };
        }
    };

    if !msn.applied {
        return FileStudy {
            name,
            rel_dir,
            extension,
            orig_raw: data.len(),
            z_orig: z_orig.len(),
            domain: msn.domain_id,
            confidence: msn.confidence,
            applied: false,
            residual_raw: 0,
            meta_raw: 0,
            z_residual: 0,
            z_combined: 0,
            fields: HashMap::new(),
            bypass_raw_ok: false,
        };
    }

    let meta = msn.metadata();
    let meta_bytes = encode_metadata_compact(&meta).expect("encode");
    let residual = &msn.residual;

    let z_residual = zstd_compress(residual, level);

    let mut combined = meta_bytes.clone();
    combined.extend_from_slice(residual);
    let z_combined = zstd_compress(&combined, level);

    let bypass_raw_ok = residual.len() + meta_bytes.len() < data.len();

    // Per-field marginal cost analysis
    let mut field_studies: HashMap<String, FieldStudy> = HashMap::new();
    let mut field_keys: Vec<String> = msn.fields.keys().cloned().collect();
    field_keys.sort();

    for key in &field_keys {
        let val = &msn.fields[key];

        let meta_without = meta_without_field(&meta, key);
        let raw_meta_contribution = meta_bytes.len().saturating_sub(meta_without.len());

        let mut combined_without = meta_without.clone();
        combined_without.extend_from_slice(residual);
        let z_combined_without = zstd_compress(&combined_without, level);

        let marginal_z_cost = z_combined_without.len() as i64 - z_combined.len() as i64;

        let est_removal = estimate_residual_removal(key, val);
        let description = describe_field(val);

        field_studies.insert(
            key.clone(),
            FieldStudy {
                key: key.clone(),
                raw_meta_contribution,
                marginal_z_cost,
                estimated_residual_removal: est_removal,
                description,
            },
        );
    }

    FileStudy {
        name,
        rel_dir,
        extension,
        orig_raw: data.len(),
        z_orig: z_orig.len(),
        domain: msn.domain_id,
        confidence: msn.confidence,
        applied: true,
        residual_raw: residual.len(),
        meta_raw: meta_bytes.len(),
        z_residual: z_residual.len(),
        z_combined: z_combined.len(),
        fields: field_studies,
        bypass_raw_ok,
    }
}

// ---------------------------------------------------------------------------
// Report formatting
// ---------------------------------------------------------------------------

fn print_report(study: &FileStudy) {
    let sep = "─".repeat(76);
    let orig_ratio = study.orig_raw as f64 / study.z_orig as f64;

    if !study.applied {
        println!("{sep}");
        println!("FILE: {}  ({} B raw)", study.name, study.orig_raw);
        let domain_str = study.domain.as_deref().unwrap_or("none");
        println!(
            "  NO DOMAIN APPLIED  detected={}  conf={:.2}",
            domain_str, study.confidence
        );
        println!("  z_orig: {} B  ({:.2}x)", study.z_orig, orig_ratio);
        return;
    }

    let comb_ratio = study.orig_raw as f64 / study.z_combined as f64;
    let net_gain = study.z_orig as i64 - study.z_combined as i64;
    let meta_z_cost = study.z_combined as i64 - study.z_residual as i64;
    let resid_z_sav = study.z_orig as i64 - study.z_residual as i64;
    let efficiency_pct = if resid_z_sav > 0 {
        net_gain as f64 / resid_z_sav as f64 * 100.0
    } else {
        0.0
    };
    let result_str = if net_gain > 0 { "✓ WINS" } else { "✗ HURTS" };

    println!("{sep}");
    println!("FILE: {}  ({} B raw)", study.name, study.orig_raw);
    println!(
        "  domain: {}  conf={:.2}",
        study.domain.as_deref().unwrap_or("?"),
        study.confidence
    );
    println!(
        "  z_orig:          {:>9} B  ({:.2}x)  baseline",
        study.z_orig, orig_ratio
    );
    println!(
        "  z_combined(MSN): {:>9} B  ({:.2}x)  net={:+} B  {}",
        study.z_combined, comb_ratio, net_gain, result_str
    );
    println!(
        "  z_residual_only: {:>9} B  ({:.2}x)  [ideal: 0 meta overhead]",
        study.z_residual,
        study.orig_raw as f64 / study.z_residual as f64
    );
    println!(
        "  residual_raw:    {:>9} B  ({:.1}% of orig)    meta_raw: {} B",
        study.residual_raw,
        study.residual_raw as f64 / study.orig_raw as f64 * 100.0,
        study.meta_raw
    );
    println!(
        "  resid_z_savings: {:>+9} B  (max possible gain from extraction)",
        resid_z_sav
    );
    println!(
        "  meta_z_cost:     {:>+9} B  (actual cost of metadata in stream)",
        meta_z_cost
    );
    println!(
        "  NET:             {:>+9} B  (efficiency: {:.1}% of max)  {}",
        net_gain, efficiency_pct, result_str
    );
    println!(
        "  bypass_check(raw): {}  (residual+meta={} vs orig={})",
        if study.bypass_raw_ok { "PASS" } else { "FAIL" },
        study.residual_raw + study.meta_raw,
        study.orig_raw
    );

    if study.fields.is_empty() {
        println!("  (no fields extracted)");
        return;
    }

    println!("  ┌─ Field breakdown (marginal z-cost in combined stream) ─────────────────┐");
    let mut fkeys: Vec<&str> = study.fields.keys().map(|s| s.as_str()).collect();
    fkeys.sort();

    for key in fkeys {
        let f = &study.fields[key];
        let roi_str = if f.marginal_z_cost > 0 {
            format!("SAVES {:+}B", f.marginal_z_cost)
        } else if f.marginal_z_cost < 0 {
            format!("COSTS {:+}B", f.marginal_z_cost)
        } else {
            "NEUTRAL".to_string()
        };
        let removal_str = f
            .estimated_residual_removal
            .map(|r| format!("~{}B residual removed", r))
            .unwrap_or_default();
        println!(
            "  │  {:22}  raw={:>6}B  {}  {}",
            key, f.raw_meta_contribution, roi_str, removal_str
        );
        println!("  │    {}", f.description);
    }
    println!("  └──────────────────────────────────────────────────────────────────────────┘");
}

fn print_csv_header() {
    println!("file,ext,rel_dir,orig_raw,z_orig,orig_ratio,applied,domain,conf,residual_raw,meta_raw,z_residual,z_combined,comb_ratio,net_gain,meta_z_cost,resid_z_sav,bypass_raw_ok");
}

fn print_csv_row(s: &FileStudy) {
    println!(
        "{},{},{},{},{},{:.3},{},{},{:.2},{},{},{},{},{:.3},{},{},{},{}",
        s.name,
        s.extension,
        s.rel_dir,
        s.orig_raw,
        s.z_orig,
        s.orig_raw as f64 / s.z_orig as f64,
        s.applied,
        s.domain.as_deref().unwrap_or(""),
        s.confidence,
        s.residual_raw,
        s.meta_raw,
        s.z_residual,
        s.z_combined,
        if s.z_combined > 0 {
            s.orig_raw as f64 / s.z_combined as f64
        } else {
            0.0
        },
        s.z_orig as i64 - s.z_combined as i64,
        s.z_combined as i64 - s.z_residual as i64,
        s.z_orig as i64 - s.z_residual as i64,
        s.bypass_raw_ok,
    );
}

// ---------------------------------------------------------------------------
// Summary: per-file table + by-extension + by-domain + by-directory
// ---------------------------------------------------------------------------

fn print_summary(studies: &[FileStudy]) {
    // --- overall file-by-file table ---
    println!("\n{}", "═".repeat(80));
    println!("SUMMARY — ALL FILES");
    println!("{}", "═".repeat(80));
    println!(
        "{:<34}  {:>8}  {:>8}  {:>7}  {:>9}  Domain",
        "File", "z_orig", "z_msn", "ratio", "net_gain"
    );
    println!("{}", "─".repeat(80));

    let mut total_orig_z: i64 = 0;
    let mut total_msn_z: i64 = 0;

    for s in studies {
        let (msn_z_str, net_str, domain_str) = if s.applied {
            let net = s.z_orig as i64 - s.z_combined as i64;
            total_orig_z += s.z_orig as i64;
            total_msn_z += s.z_combined as i64;
            (
                format!("{:>8}", s.z_combined),
                format!("{:>+9}", net),
                s.domain.as_deref().unwrap_or("?").to_string(),
            )
        } else {
            total_orig_z += s.z_orig as i64;
            total_msn_z += s.z_orig as i64;
            (
                format!("{:>8}", s.z_orig),
                "      n/a".to_string(),
                format!("({}) NO_DOMAIN", s.domain.as_deref().unwrap_or("none")),
            )
        };

        let comb_ratio = if s.applied {
            s.orig_raw as f64 / s.z_combined as f64
        } else {
            s.orig_raw as f64 / s.z_orig as f64
        };

        let display_name = if s.name.chars().count() > 33 {
            let start = s.name.char_indices()
                .rev()
                .nth(31)
                .map(|(i, _)| i)
                .unwrap_or(0);
            format!("\u{2026}{}", &s.name[start..])
        } else {
            s.name.clone()
        };

        println!(
            "{:<34}  {:>8}  {}  {:>7.2}x  {}  {}",
            display_name,
            s.z_orig,
            msn_z_str,
            comb_ratio,
            net_str,
            domain_str
        );
    }

    println!("{}", "─".repeat(80));
    if total_msn_z > 0 {
        let total_gain = total_orig_z - total_msn_z;
        println!(
            "{:<34}  {:>8}  {:>8}  {:>7.3}x  {:>+9}  (total)",
            "TOTAL",
            total_orig_z,
            total_msn_z,
            total_orig_z as f64 / total_msn_z as f64,
            total_gain
        );
    }

    // --- per-extension breakdown ---
    println!("\n{}", "═".repeat(80));
    println!("BY EXTENSION");
    println!("{}", "═".repeat(80));
    println!(
        "{:<10}  {:>6}  {:>6}  {:>12}  {:>12}  {:>8}  {:>10}",
        "Ext", "Files", "Appld", "z_orig_sum", "z_msn_sum", "ratio", "net_gain"
    );
    println!("{}", "─".repeat(80));

    // (count, applied_count, z_orig_sum, z_msn_sum)
    let mut by_ext: HashMap<String, (usize, usize, i64, i64)> = HashMap::new();
    for s in studies {
        let e = by_ext.entry(s.extension.clone()).or_insert((0, 0, 0, 0));
        e.0 += 1;
        if s.applied {
            e.1 += 1;
            e.2 += s.z_orig as i64;
            e.3 += s.z_combined as i64;
        } else {
            e.2 += s.z_orig as i64;
            e.3 += s.z_orig as i64;
        }
    }

    let mut ext_keys: Vec<String> = by_ext.keys().cloned().collect();
    // Sort by net gain descending
    ext_keys.sort_by_key(|k| -(by_ext[k].2 - by_ext[k].3));

    for ext in &ext_keys {
        let (cnt, applied, z_orig, z_msn) = by_ext[ext];
        let net = z_orig - z_msn;
        let ratio = if z_msn > 0 { z_orig as f64 / z_msn as f64 } else { 0.0 };
        println!(
            "{:<10}  {:>6}  {:>6}  {:>12}  {:>12}  {:>8.3}x  {:>+10}",
            if ext.is_empty() { "(none)" } else { ext },
            cnt, applied, z_orig, z_msn, ratio, net
        );
    }

    // --- by-domain breakdown ---
    println!("\n{}", "═".repeat(80));
    println!("BY DOMAIN");
    println!("{}", "═".repeat(80));
    println!(
        "{:<22}  {:>6}  {:>12}  {:>12}  {:>8}  {:>10}",
        "Domain", "Files", "z_orig_sum", "z_msn_sum", "ratio", "net_gain"
    );
    println!("{}", "─".repeat(80));

    let mut by_domain: HashMap<String, (usize, i64, i64)> = HashMap::new();
    for s in studies {
        let key = s.domain.clone().unwrap_or_else(|| "NO_DOMAIN".to_string());
        let e = by_domain.entry(key).or_insert((0, 0, 0));
        e.0 += 1;
        e.1 += s.z_orig as i64;
        e.2 += if s.applied { s.z_combined as i64 } else { s.z_orig as i64 };
    }

    let mut domain_keys: Vec<String> = by_domain.keys().cloned().collect();
    domain_keys.sort_by_key(|k| -(by_domain[k].1 - by_domain[k].2));

    for dom in &domain_keys {
        let (cnt, z_orig, z_msn) = by_domain[dom];
        let net = z_orig - z_msn;
        let ratio = if z_msn > 0 { z_orig as f64 / z_msn as f64 } else { 0.0 };
        println!(
            "{:<22}  {:>6}  {:>12}  {:>12}  {:>8.3}x  {:>+10}",
            dom, cnt, z_orig, z_msn, ratio, net
        );
    }

    // --- by-directory breakdown (top-level component) ---
    println!("\n{}", "═".repeat(80));
    println!("BY TOP-LEVEL DIRECTORY");
    println!("{}", "═".repeat(80));
    println!(
        "{:<30}  {:>6}  {:>12}  {:>12}  {:>8}  {:>10}",
        "Dir", "Files", "z_orig_sum", "z_msn_sum", "ratio", "net_gain"
    );
    println!("{}", "─".repeat(80));

    let mut by_dir: HashMap<String, (usize, i64, i64)> = HashMap::new();
    for s in studies {
        let dir = if s.rel_dir.is_empty() {
            "(root)".to_string()
        } else {
            s.rel_dir
                .split(['/', '\\'])
                .next()
                .unwrap_or(&s.rel_dir)
                .to_string()
        };
        let e = by_dir.entry(dir).or_insert((0, 0, 0));
        e.0 += 1;
        e.1 += s.z_orig as i64;
        e.2 += if s.applied { s.z_combined as i64 } else { s.z_orig as i64 };
    }

    let mut dir_keys: Vec<String> = by_dir.keys().cloned().collect();
    dir_keys.sort_by_key(|k| -(by_dir[k].1 - by_dir[k].2));

    for dir in &dir_keys {
        let (cnt, z_orig, z_msn) = by_dir[dir];
        let net = z_orig - z_msn;
        let ratio = if z_msn > 0 { z_orig as f64 / z_msn as f64 } else { 0.0 };
        println!(
            "{:<30}  {:>6}  {:>12}  {:>12}  {:>8.3}x  {:>+10}",
            dir, cnt, z_orig, z_msn, ratio, net
        );
    }
}

// ---------------------------------------------------------------------------
// File collection (recursive)
// ---------------------------------------------------------------------------

struct Skipped {
    path: PathBuf,
    size_mb: f64,
}

fn collect_files(
    dir: &Path,
    recursive: bool,
    max_size_bytes: u64,
    all_types: bool,
    result: &mut Vec<PathBuf>,
    skipped: &mut Vec<Skipped>,
) {
    let Ok(entries) = std::fs::read_dir(dir) else { return };

    for entry in entries.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.is_dir() {
            if recursive {
                collect_files(&path, recursive, max_size_bytes, all_types, result, skipped);
            }
            continue;
        }
        if !path.is_file() {
            continue;
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        if !all_types {
            // Always skip known-binary — no point studying them
            if is_binary_extension(&ext) {
                continue;
            }
            // Unknown extension: skip quietly
            if !ext.is_empty() && !is_text_extension(&ext) {
                continue;
            }
        }

        let size = entry.metadata().map(|m| m.len()).unwrap_or(0);
        if max_size_bytes > 0 && size > max_size_bytes {
            skipped.push(Skipped {
                path,
                size_mb: size as f64 / (1024.0 * 1024.0),
            });
            continue;
        }

        result.push(path);
    }
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

fn main() {
    let args = Args::parse();
    let _ = std::io::stdout().flush();

    let max_size_bytes = if args.max_size_mb == 0 {
        0u64 // 0 means no limit inside collect_files
    } else {
        args.max_size_mb * 1024 * 1024
    };

    let mut paths: Vec<PathBuf> = Vec::new();
    let mut skipped: Vec<Skipped> = Vec::new();

    collect_files(
        &args.corpus,
        !args.no_recursive,
        max_size_bytes,
        args.all_types,
        &mut paths,
        &mut skipped,
    );

    paths.sort();

    if paths.is_empty() {
        eprintln!("No eligible files found in {}", args.corpus.display());
        std::process::exit(1);
    }

    if !args.csv {
        println!("MSN EXPLOIT STUDY");
        println!("Corpus:        {}", args.corpus.display());
        println!("zstd level:    {}  min_confidence: {:.2}", args.level, args.confidence);
        println!("Max file size: {} MB (0=unlimited)", args.max_size_mb);
        println!("Recursive:     {}", !args.no_recursive);
        println!("All types:     {}", args.all_types);
        println!("{} files to process", paths.len());
        if !skipped.is_empty() {
            println!(
                "{} files skipped (> {} MB):",
                skipped.len(), args.max_size_mb
            );
            for s in &skipped {
                println!("  SKIP [{:7.1} MB]  {}", s.size_mb, s.path.display());
            }
        }
        println!();
    } else {
        print_csv_header();
    }

    let total = paths.len();
    let mut studies: Vec<FileStudy> = Vec::new();

    for (i, path) in paths.iter().enumerate() {
        // Progress to stderr (doesn't pollute stdout/CSV output)
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("?");
        let disp_end = name.char_indices().nth(55).map(|(i, _)| i).unwrap_or(name.len());
        let display = &name[..disp_end];
        eprint!("\r[{:>5}/{:<5}] {:<55}", i + 1, total, display);
        let _ = std::io::stderr().flush();

        let s = study_file(path, &args.corpus, args.level, args.confidence);

        let show_report = !args.quiet && !args.csv && (!args.applied_only || s.applied);
        if show_report {
            print_report(&s);
        }

        if args.csv {
            print_csv_row(&s);
        }

        studies.push(s);
    }

    // Clear progress line
    eprint!("\r{:<80}\r", "");
    let _ = std::io::stderr().flush();

    if !args.csv {
        print_summary(&studies);
    }
}
