// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Closed-loop auto-analysis engine.
//!
//! Scans a directory (or single file), runs SSR + MSN + trial compression
//! on each file, and produces per-extension recommendations plus a
//! `.cpac-config.yml` describing the optimal settings for the directory.

use crate::collector::{collect_files, CollectOptions, CorpusFile};
use cpac_types::{Backend, CompressConfig, CompressionLevel};
use std::collections::HashMap;
use std::path::Path;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Per-file analysis result.
#[derive(Clone, Debug, serde::Serialize)]
pub struct FileAnalysis {
    pub name: String,
    pub extension: String,
    pub size: u64,
    pub entropy: f64,
    pub ascii_ratio: f64,
    pub domain: Option<String>,
    pub best_backend: String,
    pub best_ratio: f64,
    pub msn_helpful: bool,
}

/// Per-extension recommendation.
#[derive(Clone, Debug, serde::Serialize)]
pub struct ExtensionRec {
    pub extension: String,
    pub file_count: usize,
    pub total_bytes: u64,
    pub best_backend: String,
    pub best_level: String,
    pub enable_msn: bool,
    pub avg_ratio: f64,
}

/// Full auto-analyze report.
#[derive(Clone, Debug, serde::Serialize)]
pub struct AutoAnalyzeReport {
    pub directory: String,
    pub total_files: usize,
    pub total_bytes: u64,
    pub files: Vec<FileAnalysis>,
    pub extensions: Vec<ExtensionRec>,
    pub recommended_config: String,
    pub elapsed_secs: f64,
}

// ---------------------------------------------------------------------------
// Trial runner (lightweight per-file)
// ---------------------------------------------------------------------------

struct TrialOutcome {
    label: String,
    #[allow(dead_code)]
    compressed_size: usize,
    ratio: f64,
}

fn quick_trials(data: &[u8]) -> Vec<TrialOutcome> {
    let configs: Vec<(&str, CompressConfig)> = vec![
        (
            "zstd-default",
            CompressConfig {
                backend: Some(Backend::Zstd),
                ..Default::default()
            },
        ),
        (
            "brotli-default",
            CompressConfig {
                backend: Some(Backend::Brotli),
                ..Default::default()
            },
        ),
        (
            "zstd-msn",
            CompressConfig {
                backend: Some(Backend::Zstd),
                enable_msn: true,
                ..Default::default()
            },
        ),
        (
            "brotli-msn",
            CompressConfig {
                backend: Some(Backend::Brotli),
                enable_msn: true,
                ..Default::default()
            },
        ),
        (
            "zstd-high",
            CompressConfig {
                backend: Some(Backend::Zstd),
                level: CompressionLevel::High,
                ..Default::default()
            },
        ),
        (
            "brotli-best",
            CompressConfig {
                backend: Some(Backend::Brotli),
                level: CompressionLevel::Best,
                ..Default::default()
            },
        ),
    ];

    let mut results = Vec::new();
    let orig = data.len();
    for (label, cfg) in configs {
        let mut trial_cfg = cfg;
        trial_cfg.disable_parallel = true;
        if let Ok(r) = cpac_engine::compress(data, &trial_cfg) {
            let ratio = if !r.data.is_empty() {
                orig as f64 / r.data.len() as f64
            } else {
                0.0
            };
            results.push(TrialOutcome {
                label: label.to_string(),
                compressed_size: r.data.len(),
                ratio,
            });
        }
    }
    results
}

// ---------------------------------------------------------------------------
// Main entry point
// ---------------------------------------------------------------------------

/// Run closed-loop auto-analysis on a directory.
///
/// Scans for files, runs lightweight trial compression, and produces
/// per-extension recommendations plus a YAML config.
pub fn auto_analyze(dir: &Path, quick: bool) -> AutoAnalyzeReport {
    let start = Instant::now();

    let opts = CollectOptions {
        all_types: true,
        max_file_size: if quick {
            10 * 1024 * 1024
        } else {
            100 * 1024 * 1024
        },
        ..Default::default()
    };
    let corpus_files = collect_files(dir, &opts);

    let mut file_results: Vec<FileAnalysis> = Vec::new();
    // ext -> Vec<(best_backend, best_ratio, msn_helpful)>
    let mut ext_stats: HashMap<String, Vec<(String, f64, bool, u64)>> = HashMap::new();

    let max_files = if quick { 50 } else { 200 };
    let files_to_analyze: Vec<&CorpusFile> = corpus_files.iter().take(max_files).collect();

    for cf in &files_to_analyze {
        let data = match std::fs::read(&cf.path) {
            Ok(d) => d,
            Err(_) => continue,
        };
        if data.is_empty() {
            continue;
        }

        let ssr = cpac_ssr::analyze(&data);
        let domain = ssr.domain_hint.as_ref().map(|d| format!("{d:?}"));

        let trials = quick_trials(&data);
        if trials.is_empty() {
            continue;
        }

        let best = trials
            .iter()
            .max_by(|a, b| a.ratio.partial_cmp(&b.ratio).unwrap())
            .unwrap();
        let msn_helpful = trials.iter().filter(|t| t.label.contains("msn")).any(|t| {
            let non_msn = trials
                .iter()
                .find(|n| {
                    !n.label.contains("msn")
                        && n.label.starts_with(t.label.split('-').next().unwrap_or(""))
                })
                .map(|n| n.ratio)
                .unwrap_or(0.0);
            t.ratio > non_msn * 1.01
        });

        let best_backend = best.label.split('-').next().unwrap_or("zstd").to_string();

        let fa = FileAnalysis {
            name: cf.name.clone(),
            extension: cf.extension.clone(),
            size: cf.size,
            entropy: ssr.entropy_estimate,
            ascii_ratio: ssr.ascii_ratio,
            domain,
            best_backend: best_backend.clone(),
            best_ratio: best.ratio,
            msn_helpful,
        };

        ext_stats.entry(cf.extension.clone()).or_default().push((
            best_backend,
            best.ratio,
            msn_helpful,
            cf.size,
        ));

        file_results.push(fa);
    }

    // Build per-extension recommendations
    let mut extensions: Vec<ExtensionRec> = ext_stats
        .iter()
        .map(|(ext, entries)| {
            let file_count = entries.len();
            let total_bytes: u64 = entries.iter().map(|(_, _, _, sz)| *sz).sum();
            let avg_ratio = entries.iter().map(|(_, r, _, _)| *r).sum::<f64>() / file_count as f64;
            let msn_count = entries.iter().filter(|(_, _, m, _)| *m).count();
            let enable_msn = msn_count as f64 / file_count as f64 > 0.5;

            // Most common backend
            let mut backend_counts: HashMap<&str, usize> = HashMap::new();
            for (b, _, _, _) in entries {
                *backend_counts.entry(b.as_str()).or_default() += 1;
            }
            let best_backend = backend_counts
                .into_iter()
                .max_by_key(|(_, c)| *c)
                .map(|(b, _)| b.to_string())
                .unwrap_or_else(|| "zstd".into());

            let best_level = if avg_ratio > 3.0 { "high" } else { "default" }.to_string();

            ExtensionRec {
                extension: ext.clone(),
                file_count,
                total_bytes,
                best_backend,
                best_level,
                enable_msn,
                avg_ratio,
            }
        })
        .collect();
    extensions.sort_by(|a, b| b.total_bytes.cmp(&a.total_bytes));

    // Generate YAML config
    let recommended_config = generate_yaml_config(&extensions);
    let total_bytes = corpus_files.iter().map(|f| f.size).sum();

    AutoAnalyzeReport {
        directory: dir.display().to_string(),
        total_files: corpus_files.len(),
        total_bytes,
        files: file_results,
        extensions,
        recommended_config,
        elapsed_secs: start.elapsed().as_secs_f64(),
    }
}

/// Generate a `.cpac-config.yml` from the analysis.
fn generate_yaml_config(extensions: &[ExtensionRec]) -> String {
    let mut lines = Vec::new();
    lines.push("# Auto-generated CPAC configuration".to_string());
    lines.push("# Re-run `cpac auto-analyze` to refresh".to_string());

    // Find global defaults (most common backend/level across all files)
    let total_files: usize = extensions.iter().map(|e| e.file_count).sum();
    let global_msn = extensions
        .iter()
        .filter(|e| e.enable_msn)
        .map(|e| e.file_count)
        .sum::<usize>() as f64
        / total_files.max(1) as f64
        > 0.5;
    let global_backend = extensions
        .iter()
        .max_by_key(|e| e.file_count)
        .map(|e| e.best_backend.clone())
        .unwrap_or_else(|| "zstd".into());

    lines.push(format!("default_backend: {global_backend}"));
    lines.push("default_level: default".to_string());
    lines.push(format!("enable_msn: {global_msn}"));
    if global_msn {
        lines.push("msn_confidence: 0.5".to_string());
    }

    // Per-extension overrides (only where they differ from global)
    let overrides: Vec<&ExtensionRec> = extensions
        .iter()
        .filter(|e| {
            !e.extension.is_empty()
                && (e.best_backend != global_backend
                    || e.enable_msn != global_msn
                    || e.best_level != "default")
        })
        .collect();

    if !overrides.is_empty() {
        lines.push("files:".to_string());
        for ext in overrides {
            let mut parts = Vec::new();
            parts.push(format!("backend: {}", ext.best_backend));
            parts.push(format!("level: {}", ext.best_level));
            if ext.enable_msn != global_msn {
                parts.push(format!("enable_msn: {}", ext.enable_msn));
            }
            lines.push(format!(
                "  \"*.{}\": {{ {} }}",
                ext.extension,
                parts.join(", ")
            ));
        }
    }

    lines.join("\n")
}

/// Format the report as Markdown.
#[must_use]
pub fn format_report(report: &AutoAnalyzeReport) -> String {
    let mut md = String::new();
    md.push_str("# CPAC Auto-Analysis Report\n\n");
    md.push_str(&format!("**Directory**: {}\n", report.directory));
    md.push_str(&format!(
        "**Files**: {} ({:.1} MB)\n",
        report.total_files,
        report.total_bytes as f64 / 1_048_576.0
    ));
    md.push_str(&format!("**Elapsed**: {:.1}s\n\n", report.elapsed_secs));

    if !report.extensions.is_empty() {
        md.push_str("## Per-Extension Recommendations\n\n");
        md.push_str("| Extension | Files | Size | Backend | Level | MSN | Avg Ratio |\n");
        md.push_str("|-----------|------:|-----:|---------|-------|-----|---------:|\n");
        for ext in &report.extensions {
            let ext_name = if ext.extension.is_empty() {
                "(none)"
            } else {
                &ext.extension
            };
            md.push_str(&format!(
                "| .{} | {} | {:.1} KB | {} | {} | {} | {:.2}x |\n",
                ext_name,
                ext.file_count,
                ext.total_bytes as f64 / 1024.0,
                ext.best_backend,
                ext.best_level,
                if ext.enable_msn { "yes" } else { "no" },
                ext.avg_ratio,
            ));
        }
    }

    md.push_str("\n## Recommended Configuration\n\n");
    md.push_str("```yaml\n");
    md.push_str(&report.recommended_config);
    md.push_str("\n```\n");

    md
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn create_test_dir() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        // JSON file
        let json = dir.path().join("data.json");
        std::fs::write(&json, r#"{"key":"value","num":42}"#.repeat(500)).unwrap();
        // Log file
        let log = dir.path().join("app.log");
        let mut f = std::fs::File::create(&log).unwrap();
        for i in 0..500 {
            writeln!(
                f,
                "2026-01-01T00:00:{i:02}Z INFO request completed in {i}ms"
            )
            .unwrap();
        }
        // Text file
        std::fs::write(dir.path().join("readme.txt"), "Hello World! ".repeat(1000)).unwrap();
        dir
    }

    #[test]
    fn auto_analyze_runs() {
        let dir = create_test_dir();
        let report = auto_analyze(dir.path(), true);
        assert!(report.total_files >= 3);
        assert!(!report.extensions.is_empty());
        assert!(!report.recommended_config.is_empty());
    }

    #[test]
    fn yaml_config_generated() {
        let dir = create_test_dir();
        let report = auto_analyze(dir.path(), true);
        assert!(report.recommended_config.contains("default_backend"));
    }

    #[test]
    fn markdown_report() {
        let dir = create_test_dir();
        let report = auto_analyze(dir.path(), true);
        let md = format_report(&report);
        assert!(md.contains("Auto-Analysis Report"));
        assert!(md.contains("Per-Extension"));
    }
}
