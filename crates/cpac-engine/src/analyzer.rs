// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Smart Structure Analyzer — capstone module.
//!
//! Runs SSR → MSN detection → CAS constraint inference → transform
//! recommendation in a single `analyze_structure()` call and returns a
//! [`StructureProfile`] describing the file's characteristics and the
//! recommended compression strategy.

use cpac_cas::{analyze_column, recommend_transforms, Constraint, TransformRecommendation};
use cpac_ssr::SSRResult;
use cpac_types::{CpacType, Track};
use std::collections::HashMap;
use std::sync::OnceLock;

/// Per-column profile including type, constraints, and recommended transforms.
#[derive(Clone, Debug)]
pub struct ColumnProfile {
    /// Column name.
    pub name: String,
    /// Detected constraints.
    pub constraints: Vec<Constraint>,
    /// Recommended transforms for this column.
    pub recommended: Vec<TransformRecommendation>,
}

/// Full structure profile for a file.
#[derive(Clone, Debug)]
pub struct StructureProfile {
    /// SSR analysis results (entropy, ASCII ratio, track).
    pub ssr: SSRResult,
    /// Detected MSN domain (if any).
    pub domain: Option<cpac_msn::DomainInfo>,
    /// Per-column profiles (empty for non-structured data).
    pub columns: Vec<ColumnProfile>,
    /// Overall recommended transform chain.
    pub recommended_chain: Vec<TransformRecommendation>,
    /// Estimated compression gain (0.0–1.0, higher = more compressible).
    pub estimated_gain: f64,
}

/// Analyze a file's structure and recommend the optimal compression strategy.
///
/// Runs the full pipeline: SSR → MSN domain detection → column extraction →
/// CAS constraint inference → transform recommendation.
#[must_use]
pub fn analyze_structure(data: &[u8], filename: Option<&str>) -> StructureProfile {
    // Step 1: SSR analysis
    let ssr = cpac_ssr::analyze(data);

    // Step 2: MSN domain detection via registry
    let registry = cpac_msn::global_registry();
    let best_domain = registry
        .auto_detect(data, filename, 0.3)
        .map(|(d, _conf)| d.info());

    // Step 3: Try MSN extraction for column analysis
    let mut columns = Vec::new();
    let mut overall_constraints = Vec::new();

    if ssr.track == Track::Track1 {
        if let Ok(extraction) = cpac_msn::extract(data, filename, 0.3) {
            if extraction.applied {
                // Try to build typed columns from extraction fields
                let typed_columns = extract_typed_columns(&extraction);
                for (name, col_data) in &typed_columns {
                    let constraints = analyze_column(name, col_data);
                    let recommended = recommend_transforms(&constraints);
                    overall_constraints.extend(constraints.clone());
                    columns.push(ColumnProfile {
                        name: name.clone(),
                        constraints,
                        recommended,
                    });
                }
            }
        }
    }

    // Step 4: Build overall recommendation
    let recommended_chain = if overall_constraints.is_empty() {
        // No columnar data — recommend based on SSR characteristics
        recommend_from_ssr(&ssr)
    } else {
        recommend_transforms(&overall_constraints)
    };

    // Step 5: Estimate gain
    let estimated_gain = estimate_overall_gain(&ssr, &columns, &recommended_chain);

    StructureProfile {
        ssr,
        domain: best_domain,
        columns,
        recommended_chain,
        estimated_gain,
    }
}

/// Extract typed columns from MSN extraction result.
fn extract_typed_columns(
    extraction: &cpac_msn::MsnResult,
) -> Vec<(String, CpacType)> {
    let mut columns = Vec::new();

    // Check for int_columns from CSV columnar extraction
    if let Some(serde_json::Value::Object(int_cols)) = extraction.fields.get("int_columns") {
        for (col_idx, arr_val) in int_cols {
            if let Some(arr) = arr_val.as_array() {
                let values: Vec<i64> = arr.iter().filter_map(|v| v.as_i64()).collect();
                if !values.is_empty() {
                    columns.push((
                        format!("col_{col_idx}"),
                        CpacType::IntColumn {
                            values,
                            original_width: 8,
                        },
                    ));
                }
            }
        }
    }

    // Check for headers (string column names)
    if let Some(serde_json::Value::Array(headers)) = extraction.fields.get("headers") {
        let header_strs: Vec<String> = headers
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect();
        if !header_strs.is_empty() {
            columns.push((
                "headers".to_string(),
                CpacType::StringColumn {
                    total_bytes: header_strs.iter().map(String::len).sum(),
                    values: header_strs,
                },
            ));
        }
    }

    columns
}

// ---------------------------------------------------------------------------
// Runtime calibration
// ---------------------------------------------------------------------------

/// Per-transform calibration entry loaded from calibration.json.
#[derive(Clone, Debug)]
struct CalibrationEntry {
    win_rate: f64,
    #[allow(dead_code)]
    avg_gain: f64,
    files: usize,
}

/// Loaded calibration data: transform name → entry.
type CalibrationMap = HashMap<String, CalibrationEntry>;

/// Global calibration cache (loaded once on first access).
static CALIBRATION: OnceLock<Option<CalibrationMap>> = OnceLock::new();

/// Load calibration data from the default path or `CPAC_CALIBRATION` env var.
fn load_calibration() -> &'static Option<CalibrationMap> {
    CALIBRATION.get_or_init(|| {
        let path = std::env::var("CPAC_CALIBRATION")
            .unwrap_or_else(|_| ".work/benchmarks/calibration.json".to_string());
        let content = std::fs::read_to_string(&path).ok()?;
        let json: serde_json::Value = serde_json::from_str(&content).ok()?;
        let transforms = json.get("transforms")?.as_object()?;
        let mut map = HashMap::new();
        for (name, val) in transforms {
            if let Some(overall) = val.get("overall") {
                let win_rate = overall.get("win_rate")?.as_f64()?;
                let avg_gain = overall.get("avg_gain_bytes").and_then(|v| v.as_f64()).unwrap_or(0.0);
                let files = overall.get("files").and_then(|v| v.as_u64()).unwrap_or(0) as usize;
                map.insert(name.clone(), CalibrationEntry { win_rate, avg_gain, files });
            }
        }
        Some(map)
    })
}

/// Look up calibrated win-rate for a transform.  Returns `None` if no
/// calibration data is available or the transform isn't in the data.
fn calibrated_confidence(name: &str) -> Option<f64> {
    let cal = load_calibration().as_ref()?;
    let entry = cal.get(name)?;
    // Only trust entries with >= 10 data points.
    if entry.files >= 10 {
        Some(entry.win_rate)
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// Recommendation engine
// ---------------------------------------------------------------------------

/// Recommend transforms based on SSR characteristics alone.
///
/// Thresholds are calibrated from corpus benchmarks (silesia, enwik8, cloud,
/// logs, calgary, canterbury).  See `.work/benchmarks/transform-study/`.
///
/// When a `calibration.json` is available (via `CPAC_CALIBRATION` env var or
/// the default path), the hardcoded confidence values are overridden by the
/// empirically-measured win-rate from the calibration data.
///
/// # Empirical summary
///
/// | Transform   | Best domain        | Win-rate  | Gate                                |
/// |-------------|--------------------|-----------|-------------------------------------|
/// | normalize   | structured text    | 99.9%     | ascii_ratio > 0.80                  |
/// | bwt_chain   | large text         | ~60%      | ascii_ratio > 0.85, size > 32 KB    |
/// | byte_plane  | medical/sci binary | situational| ascii_ratio < 0.50, entropy < 6.0  |
/// | delta/rle/context_split on Serial: **never** (always hurts or zero gain)           |
fn recommend_from_ssr(ssr: &SSRResult) -> Vec<TransformRecommendation> {
    let mut recs = Vec::new();

    let is_text = ssr.ascii_ratio > 0.80;
    let is_binary = ssr.ascii_ratio < 0.50;
    let size = ssr.data_size;

    // --- Tier 1: normalize (99.9% win-rate on structured text) ---
    if is_text {
        let confidence = calibrated_confidence("normalize").unwrap_or(0.95);
        recs.push(TransformRecommendation {
            name: "normalize".to_string(),
            priority: 5,
            confidence,
        });
    }

    // --- Tier 1: bwt_chain (dominant on large text) ---
    // Cap at 1 MB to match bwt_chain transform limits. Our current BWT encode
    // uses a naive suffix-sort and becomes prohibitively expensive on larger
    // blocks.
    if is_text && ssr.entropy_estimate < 5.5 && size > 32_768 && size <= 1_000_000 {
        let confidence = calibrated_confidence("bwt_chain").unwrap_or(0.60);
        recs.push(TransformRecommendation {
            name: "bwt_chain".to_string(),
            priority: 10,
            confidence,
        });
    }

    // --- Tier 2: byte_plane (situational on binary) ---
    if is_binary && ssr.entropy_estimate < 6.0 {
        let confidence = calibrated_confidence("byte_plane").unwrap_or(0.30);
        recs.push(TransformRecommendation {
            name: "byte_plane".to_string(),
            priority: 10,
            confidence,
        });
    }

    // --- Tier 0: DoF elimination (const_elim, stride_elim) ---
    // These operate on raw Serial and can eliminate entire data regions.
    // const_elim: effective when >90% of bytes are the same value.
    if ssr.entropy_estimate < 1.0 && size >= 64 {
        let confidence = calibrated_confidence("const_elim").unwrap_or(0.95);
        recs.push(TransformRecommendation {
            name: "const_elim".to_string(),
            priority: 1,
            confidence,
        });
    }
    // stride_elim: effective on structured binary with fixed-width integer sequences.
    if is_binary && ssr.entropy_estimate < 5.0 && size >= 64 {
        let confidence = calibrated_confidence("stride_elim").unwrap_or(0.70);
        recs.push(TransformRecommendation {
            name: "stride_elim".to_string(),
            priority: 2,
            confidence,
        });
    }

    // --- Tier 1.5: prediction (sequential correlation) ---
    // Effective on binary data with temporal patterns (telemetry, counters).
    if is_binary && ssr.entropy_estimate > 1.0 && ssr.entropy_estimate < 6.0 && size >= 64 {
        let confidence = calibrated_confidence("predict").unwrap_or(0.55);
        recs.push(TransformRecommendation {
            name: "predict".to_string(),
            priority: 4,
            confidence,
        });
    }

    // --- Tier 1.5: entropy conditioning (mixed-content data) ---
    if is_text && ssr.entropy_estimate > 2.0 && ssr.entropy_estimate < 7.0 && size >= 256 {
        let confidence = calibrated_confidence("condition").unwrap_or(0.55);
        recs.push(TransformRecommendation {
            name: "condition".to_string(),
            priority: 3,
            confidence,
        });
    }

    // NOTE: The following are deliberately NEVER recommended on raw Serial
    // data based on benchmark evidence:
    // - delta:         always negative on raw bytes (-12.1M silesia, -6.3M enwik8)
    // - rle:           zero gain on all corpora (zstd already handles runs)
    // - context_split: zero gain everywhere (overhead always exceeds benefit)
    // - arith_decomp:  IntColumn-only (errors on Serial input)
    // These transforms remain available for column-level use via CAS/DAG.

    recs
}

/// Estimate overall compression gain from the profile.
///
/// Uses empirically-calibrated weights derived from corpus benchmarks.
/// The returned value (0.0–1.0) is a relative estimate; higher = more
/// compressible after applying the recommended transforms.
fn estimate_overall_gain(
    ssr: &SSRResult,
    columns: &[ColumnProfile],
    chain: &[TransformRecommendation],
) -> f64 {
    // Base gain from entropy (lower entropy = more gain potential)
    let entropy_gain = (1.0 - ssr.entropy_estimate / 8.0).max(0.0);

    // Column-based gain bonus
    let column_bonus = if columns.is_empty() {
        0.0
    } else {
        let avg_constraints = columns.iter().map(|c| c.constraints.len()).sum::<usize>() as f64
            / columns.len() as f64;
        (avg_constraints * 0.05).min(0.3)
    };

    // Transform chain bonus — weighted by empirical confidence
    let chain_bonus = if chain.is_empty() {
        0.0
    } else {
        let avg_confidence =
            chain.iter().map(|r| r.confidence).sum::<f64>() / chain.len() as f64;
        (avg_confidence * 0.15 + chain.len() as f64 * 0.01).min(0.25)
    };

    (entropy_gain * 0.5 + column_bonus + chain_bonus + ssr.viability_score * 0.3).min(1.0)
}

/// Format a structure profile as a human-readable string.
#[must_use]
pub fn format_profile(profile: &StructureProfile) -> String {
    let mut out = String::new();

    out.push_str(&format!(
        "=== Structure Profile ===\n\
         Entropy:     {:.2} bits/byte\n\
         ASCII ratio: {:.1}%\n\
         Track:       {:?}\n\
         Viability:   {:.2}\n",
        profile.ssr.entropy_estimate,
        profile.ssr.ascii_ratio * 100.0,
        profile.ssr.track,
        profile.ssr.viability_score,
    ));

    if let Some(ref domain) = profile.domain {
        out.push_str(&format!("Domain:      {} ({})\n", domain.name, domain.id));
    }

    if !profile.columns.is_empty() {
        out.push_str(&format!("\nColumns: {}\n", profile.columns.len()));
        for col in &profile.columns {
            out.push_str(&format!(
                "  {}: {} constraints, {} transforms\n",
                col.name,
                col.constraints.len(),
                col.recommended.len(),
            ));
        }
    }

    if !profile.recommended_chain.is_empty() {
        out.push_str("\nRecommended chain:\n");
        for r in &profile.recommended_chain {
            out.push_str(&format!(
                "  {} (priority={}, confidence={:.0}%)\n",
                r.name, r.priority, r.confidence * 100.0,
            ));
        }
    }

    out.push_str(&format!(
        "\nEstimated gain: {:.1}%\n",
        profile.estimated_gain * 100.0
    ));

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn analyze_text() {
        let data = b"The quick brown fox jumps over the lazy dog. ".repeat(100);
        let profile = analyze_structure(&data, Some("test.txt"));
        assert!(profile.ssr.ascii_ratio > 0.9);
        assert!(profile.estimated_gain > 0.0);
    }

    #[test]
    fn analyze_csv() {
        let mut data = b"id,name,value\n".to_vec();
        for i in 0..100 {
            data.extend_from_slice(format!("{i},item_{i},{}\n", i * 10).as_bytes());
        }
        let profile = analyze_structure(&data, Some("test.csv"));
        assert!(profile.domain.is_some(), "should detect CSV domain");
    }

    #[test]
    fn analyze_binary() {
        let data: Vec<u8> = (0..1024).map(|i| (i * 37 % 256) as u8).collect();
        let profile = analyze_structure(&data, None);
        assert!(profile.ssr.ascii_ratio < 0.5);
    }

    #[test]
    fn format_profile_output() {
        let data = b"test data for profile formatting ".repeat(20);
        let profile = analyze_structure(&data, None);
        let output = format_profile(&profile);
        assert!(output.contains("Structure Profile"));
        assert!(output.contains("Entropy"));
        assert!(output.contains("Estimated gain"));
    }
}
