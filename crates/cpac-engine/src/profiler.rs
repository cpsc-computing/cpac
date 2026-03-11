// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! File profiling engine — trial compression, gap analysis, recommendations.
//!
//! Runs multiple compression configurations against a file sample and reports
//! the best pipeline, current-vs-best gap, and actionable recommendations.

use cpac_types::{Backend, CompressConfig, CompressionLevel, CpacResult, Track};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------------------
// Profile result types
// ---------------------------------------------------------------------------

/// Result of a single trial compression run.
#[derive(Clone, Debug)]
pub struct TrialResult {
    /// Human-readable label for this trial.
    pub label: String,
    /// Compressed size in bytes.
    pub compressed_size: usize,
    /// Compression ratio (original / compressed).
    pub ratio: f64,
    /// Compression time.
    pub compress_time: Duration,
    /// Decompression time.
    pub decompress_time: Duration,
    /// Whether the roundtrip was lossless.
    pub verified: bool,
    /// The config used for this trial.
    pub config: CompressConfig,
}

/// Gap analysis entry — how much better could we do?
#[derive(Clone, Debug)]
pub struct GapEntry {
    /// What the current CPAC default achieves.
    pub current_ratio: f64,
    pub current_size: usize,
    /// Best ratio found in trials.
    pub best_ratio: f64,
    pub best_size: usize,
    pub best_label: String,
    /// Gap: percentage improvement available.
    pub gap_pct: f64,
}

/// Configuration recommendation from profiling.
#[derive(Clone, Debug)]
pub struct Recommendation {
    /// Short action description.
    pub action: String,
    /// Expected improvement.
    pub expected_gain: String,
    /// Confidence (0.0–1.0).
    pub confidence: f64,
}

/// Complete profiling result for a file.
#[derive(Clone, Debug)]
pub struct ProfileResult {
    /// File size.
    pub original_size: usize,
    /// SSR analysis results.
    pub entropy: f64,
    pub ascii_ratio: f64,
    pub track: Track,
    /// Domain detected (if any).
    pub domain: Option<String>,
    /// Trial compression results (sorted by ratio, best first).
    pub trials: Vec<TrialResult>,
    /// Gap analysis vs CPAC default.
    pub gap: Option<GapEntry>,
    /// Recommendations.
    pub recommendations: Vec<Recommendation>,
}

// ---------------------------------------------------------------------------
// Trial configurations
// ---------------------------------------------------------------------------

/// Generate trial configurations for comprehensive profiling.
fn build_trial_configs(quick: bool) -> Vec<(String, CompressConfig)> {
    let mut trials = vec![
        // Default CPAC pipeline
        ("cpac-default".to_string(), CompressConfig::default()),
        // Default with MSN enabled
        (
            "cpac-msn".to_string(),
            CompressConfig {
                enable_msn: true,
                ..Default::default()
            },
        ),
        // Default with smart transforms
        (
            "cpac-smart".to_string(),
            CompressConfig {
                enable_smart_transforms: true,
                ..Default::default()
            },
        ),
        // Smart + MSN combined
        (
            "cpac-smart+msn".to_string(),
            CompressConfig {
                enable_smart_transforms: true,
                enable_msn: true,
                ..Default::default()
            },
        ),
    ];

    // Backend-specific trials
    for (label, backend) in [
        ("zstd-3", Backend::Zstd),
        ("brotli-6", Backend::Brotli),
        ("gzip-6", Backend::Gzip),
    ] {
        trials.push((
            label.to_string(),
            CompressConfig {
                backend: Some(backend),
                ..Default::default()
            },
        ));
    }

    if !quick {
        // Higher compression level trials
        trials.push((
            "zstd-high".to_string(),
            CompressConfig {
                backend: Some(Backend::Zstd),
                level: CompressionLevel::High,
                ..Default::default()
            },
        ));

        trials.push((
            "zstd-best".to_string(),
            CompressConfig {
                backend: Some(Backend::Zstd),
                level: CompressionLevel::Best,
                ..Default::default()
            },
        ));

        trials.push((
            "brotli-best".to_string(),
            CompressConfig {
                backend: Some(Backend::Brotli),
                level: CompressionLevel::Best,
                ..Default::default()
            },
        ));

        // Force Track 1 (MSN on every block)
        trials.push((
            "force-track1".to_string(),
            CompressConfig {
                enable_msn: true,
                force_track: Some(Track::Track1),
                ..Default::default()
            },
        ));

        // Force Track 2 (bypass MSN)
        trials.push((
            "force-track2".to_string(),
            CompressConfig {
                force_track: Some(Track::Track2),
                ..Default::default()
            },
        ));

        // Preset-like: Maximum (all features on)
        trials.push((
            "preset-maximum".to_string(),
            CompressConfig {
                enable_smart_transforms: true,
                enable_msn: true,
                level: CompressionLevel::High,
                ..Default::default()
            },
        ));

        // Preset-like: Archive (max ratio)
        trials.push((
            "preset-archive".to_string(),
            CompressConfig {
                enable_smart_transforms: true,
                enable_msn: true,
                level: CompressionLevel::Best,
                ..Default::default()
            },
        ));
    }

    trials
}

// ---------------------------------------------------------------------------
// Core profiling engine
// ---------------------------------------------------------------------------

/// Profile a file by running trial compressions and analyzing results.
///
/// # Arguments
/// * `data` — file contents
/// * `filename` — optional filename for extension-based detection
/// * `quick` — if true, run fewer trials (faster but less thorough)
pub fn profile_file(data: &[u8], filename: Option<&str>, quick: bool) -> CpacResult<ProfileResult> {
    let original_size = data.len();

    // SSR analysis
    let ssr = cpac_ssr::analyze(data);

    // Domain detection
    let registry = cpac_msn::global_registry();
    let domain: Option<String> = registry
        .auto_detect(data, filename, 0.3)
        .map(|(d, _conf)| d.info().name.to_string());

    // Build and run trials
    let trial_configs = build_trial_configs(quick);
    let mut trials: Vec<TrialResult> = Vec::with_capacity(trial_configs.len());

    for (label, mut config) in trial_configs {
        // Set filename for domain detection
        config.filename = filename.map(String::from);

        // Single iteration for profiling (speed over accuracy)
        let start = Instant::now();
        let comp_result = match crate::compress(data, &config) {
            Ok(r) => r,
            Err(_) => continue, // skip failed trials
        };
        let compress_time = start.elapsed();

        // Verify roundtrip
        let decomp_start = Instant::now();
        let verified = match crate::decompress(&comp_result.data) {
            Ok(dec) => dec.data == data,
            Err(_) => false,
        };
        let decompress_time = decomp_start.elapsed();

        let ratio = if comp_result.compressed_size > 0 {
            original_size as f64 / comp_result.compressed_size as f64
        } else {
            0.0
        };

        trials.push(TrialResult {
            label,
            compressed_size: comp_result.compressed_size,
            ratio,
            compress_time,
            decompress_time,
            verified,
            config,
        });
    }

    // Sort trials by ratio (best first)
    trials.sort_by(|a, b| {
        b.ratio
            .partial_cmp(&a.ratio)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Gap analysis: compare default vs best
    let default_trial = trials.iter().find(|t| t.label == "cpac-default");
    let best_trial = trials.first();
    let gap = match (default_trial, best_trial) {
        (Some(def), Some(best)) if best.compressed_size < def.compressed_size => {
            let gap_pct = (1.0 - best.compressed_size as f64 / def.compressed_size as f64) * 100.0;
            Some(GapEntry {
                current_ratio: def.ratio,
                current_size: def.compressed_size,
                best_ratio: best.ratio,
                best_size: best.compressed_size,
                best_label: best.label.clone(),
                gap_pct,
            })
        }
        _ => None,
    };

    // Generate recommendations
    let recommendations = generate_recommendations(&ssr, &trials, &gap, &domain);

    Ok(ProfileResult {
        original_size,
        entropy: ssr.entropy_estimate,
        ascii_ratio: ssr.ascii_ratio,
        track: ssr.track,
        domain,
        trials,
        gap,
        recommendations,
    })
}

/// Generate recommendations based on profiling results.
fn generate_recommendations(
    ssr: &cpac_ssr::SSRResult,
    trials: &[TrialResult],
    gap: &Option<GapEntry>,
    domain: &Option<String>,
) -> Vec<Recommendation> {
    let mut recs = Vec::new();

    // Check if MSN provides benefit
    let default_size = trials
        .iter()
        .find(|t| t.label == "cpac-default")
        .map(|t| t.compressed_size);
    let msn_size = trials
        .iter()
        .find(|t| t.label == "cpac-msn")
        .map(|t| t.compressed_size);
    if let (Some(def), Some(msn)) = (default_size, msn_size) {
        if msn < def {
            let gain = (1.0 - msn as f64 / def as f64) * 100.0;
            recs.push(Recommendation {
                action: "Enable MSN (--smart or enable_msn config)".to_string(),
                expected_gain: format!("{gain:+.1}% size reduction"),
                confidence: 0.9,
            });
        }
    }

    // Check if smart transforms help
    let smart_size = trials
        .iter()
        .find(|t| t.label == "cpac-smart")
        .map(|t| t.compressed_size);
    if let (Some(def), Some(smart)) = (default_size, smart_size) {
        if smart < def {
            let gain = (1.0 - smart as f64 / def as f64) * 100.0;
            recs.push(Recommendation {
                action: "Enable smart transforms (--smart)".to_string(),
                expected_gain: format!("{gain:+.1}% size reduction"),
                confidence: 0.85,
            });
        }
    }

    // Check if higher compression levels help significantly
    let zstd9_size = trials
        .iter()
        .find(|t| t.label == "zstd-9")
        .map(|t| t.compressed_size);
    let zstd3_size = trials
        .iter()
        .find(|t| t.label == "zstd-3")
        .map(|t| t.compressed_size);
    if let (Some(z3), Some(z9)) = (zstd3_size, zstd9_size) {
        if z9 < z3 {
            let gain = (1.0 - z9 as f64 / z3 as f64) * 100.0;
            if gain > 5.0 {
                recs.push(Recommendation {
                    action: "Use higher zstd level (--level 9) for ratio-critical workloads"
                        .to_string(),
                    expected_gain: format!("{gain:+.1}% vs zstd-3"),
                    confidence: 0.7,
                });
            }
        }
    }

    // Report gap if significant
    if let Some(ref g) = gap {
        if g.gap_pct > 5.0 {
            recs.push(Recommendation {
                action: format!("Switch to '{}' pipeline for this file type", g.best_label),
                expected_gain: format!(
                    "{:+.1}% ({:.2}x → {:.2}x)",
                    g.gap_pct, g.current_ratio, g.best_ratio
                ),
                confidence: 0.95,
            });
        }
    }

    // Domain-specific advice
    if domain.is_some() && ssr.track == Track::Track1 {
        recs.push(Recommendation {
            action: "Domain detected — MSN extraction should be active for best results"
                .to_string(),
            expected_gain: "varies by domain".to_string(),
            confidence: 0.8,
        });
    }

    recs
}

/// Format a profile result as a human-readable string.
#[must_use]
pub fn format_profile_result(result: &ProfileResult) -> String {
    let mut out = String::new();

    out.push_str("=== CPAC File Profile ===\n");
    out.push_str(&format!(
        "Size:        {} bytes ({:.1} KB)\n",
        result.original_size,
        result.original_size as f64 / 1024.0
    ));
    out.push_str(&format!("Entropy:     {:.2} bits/byte\n", result.entropy));
    out.push_str(&format!(
        "ASCII ratio: {:.1}%\n",
        result.ascii_ratio * 100.0
    ));
    out.push_str(&format!("Track:       {:?}\n", result.track));
    if let Some(ref d) = result.domain {
        out.push_str(&format!("Domain:      {d}\n"));
    }

    out.push_str(&format!(
        "\n--- Trial Matrix ({} configs) ---\n",
        result.trials.len()
    ));
    out.push_str(&format!(
        "{:<20} {:>10} {:>8} {:>10} {:>10} {:>5}\n",
        "Config", "Compressed", "Ratio", "Comp (ms)", "Dec (ms)", "OK?"
    ));
    out.push_str(&"-".repeat(65));
    out.push('\n');
    for trial in &result.trials {
        let ok_str = if trial.verified { "YES" } else { "NO" };
        out.push_str(&format!(
            "{:<20} {:>10} {:>7.2}x {:>9.1} {:>9.1} {:>5}\n",
            trial.label,
            trial.compressed_size,
            trial.ratio,
            trial.compress_time.as_secs_f64() * 1000.0,
            trial.decompress_time.as_secs_f64() * 1000.0,
            ok_str,
        ));
    }

    if let Some(ref gap) = result.gap {
        out.push_str(&format!(
            "\n--- Gap Analysis ---\n\
             Current (default): {:.2}x ({} bytes)\n\
             Best ({}):   {:.2}x ({} bytes)\n\
             Gap:               {:+.1}%\n",
            gap.current_ratio,
            gap.current_size,
            gap.best_label,
            gap.best_ratio,
            gap.best_size,
            gap.gap_pct,
        ));
    }

    if !result.recommendations.is_empty() {
        out.push_str("\n--- Recommendations ---\n");
        for (i, rec) in result.recommendations.iter().enumerate() {
            out.push_str(&format!(
                "{}. {} ({})\n",
                i + 1,
                rec.action,
                rec.expected_gain,
            ));
        }
    }

    out
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn profile_text_file() {
        let data = b"The quick brown fox jumps over the lazy dog. ".repeat(100);
        let result = profile_file(&data, Some("test.txt"), true).unwrap();
        assert!(result.entropy > 0.0);
        assert!(result.ascii_ratio > 0.9);
        assert!(!result.trials.is_empty());
        // All trials should verify lossless
        for trial in &result.trials {
            assert!(trial.verified, "trial '{}' failed roundtrip", trial.label);
        }
    }

    #[test]
    fn profile_binary_data() {
        let data: Vec<u8> = (0..8192).map(|i| (i * 37 % 256) as u8).collect();
        let result = profile_file(&data, None, true).unwrap();
        assert!(result.ascii_ratio < 0.5);
        assert!(!result.trials.is_empty());
    }

    #[test]
    fn format_output_has_trial_matrix() {
        let data = b"test data for profiling output formatting ".repeat(50);
        let result = profile_file(&data, None, true).unwrap();
        let output = format_profile_result(&result);
        assert!(output.contains("Trial Matrix"));
        assert!(output.contains("cpac-default"));
    }

    #[test]
    fn full_profile_has_more_trials() {
        let data = b"Profiling test with extended trials needed ".repeat(200);
        let quick = profile_file(&data, Some("test.txt"), true).unwrap();
        let full = profile_file(&data, Some("test.txt"), false).unwrap();
        assert!(
            full.trials.len() >= quick.trials.len(),
            "full ({}) should have >= quick ({}) trials",
            full.trials.len(),
            quick.trials.len()
        );
    }
}
