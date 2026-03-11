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
    let ext = filename.and_then(|f| f.rsplit_once('.')).map(|(_, e)| e);
    let recommended_chain = if overall_constraints.is_empty() {
        // No columnar data — recommend based on SSR characteristics
        recommend_from_ssr(&ssr, ext)
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

/// Lightweight variant of [`analyze_structure`] for the `smart_preprocess` path.
///
/// Accepts a pre-computed SSR result and skips MSN domain detection/extraction
/// entirely — the caller (`compress()`) already ran MSN and passed the residual.
/// Also accepts `skip_expensive` flag to suppress BWT on parallel sub-blocks.
/// This eliminates ~33% of per-block overhead for structured text data where
/// MSN extraction was the dominant cost inside the original `analyze_structure`.
#[must_use]
pub fn analyze_structure_fast(
    ssr: &SSRResult,
    filename: Option<&str>,
    skip_expensive: bool,
) -> StructureProfile {
    let ext = filename.and_then(|f| f.rsplit_once('.')).map(|(_, e)| e);
    let mut recommended_chain = recommend_from_ssr(ssr, ext);

    // P1: drop bwt_chain when called from parallel sub-blocks — BWT on
    // multi-MB blocks is expensive and block-parallel framing already
    // destroys the cross-block context BWT relies on.
    if skip_expensive {
        recommended_chain.retain(|r| r.name != "bwt_chain");
    }

    let estimated_gain = estimate_overall_gain(ssr, &[], &recommended_chain);

    StructureProfile {
        ssr: ssr.clone(),
        domain: None,
        columns: Vec::new(),
        recommended_chain,
        estimated_gain,
    }
}

/// Extract typed columns from MSN extraction result.
fn extract_typed_columns(extraction: &cpac_msn::MsnResult) -> Vec<(String, CpacType)> {
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
    /// Per-extension win rates (ext → (win_rate, files)).
    by_extension: HashMap<String, (f64, usize)>,
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
                let avg_gain = overall
                    .get("avg_gain_bytes")
                    .and_then(|v| v.as_f64())
                    .unwrap_or(0.0);
                let files = overall.get("files").and_then(|v| v.as_u64()).unwrap_or(0) as usize;

                // Load per-extension data
                let mut by_extension = HashMap::new();
                if let Some(ext_obj) = val.get("by_extension").and_then(|v| v.as_object()) {
                    for (ext, ext_val) in ext_obj {
                        if let (Some(wr), Some(fc)) = (
                            ext_val.get("win_rate").and_then(|v| v.as_f64()),
                            ext_val.get("files").and_then(|v| v.as_u64()),
                        ) {
                            by_extension.insert(ext.clone(), (wr, fc as usize));
                        }
                    }
                }

                map.insert(
                    name.clone(),
                    CalibrationEntry {
                        win_rate,
                        avg_gain,
                        files,
                        by_extension,
                    },
                );
            }
        }
        Some(map)
    })
}

/// Look up calibrated win-rate for a transform, optionally scoped to a file
/// extension.
///
/// **Design principle**: extension is a *hint* that tells us which calibration
/// bucket to check first.  The primary decision is always structural (SSR
/// entropy, ASCII ratio, size).  When no extension-specific calibration data
/// exists, we return `None` so the SSR-based heuristic defaults in
/// [`recommend_from_ssr`] drive the decision.  The overall (cross-corpus)
/// rate is only used as a fallback when the extension IS known but has too
/// few per-extension samples — because at least we know the file's domain.
///
/// Returning `None` for unknown-extension files avoids corpus dilution: e.g.
/// the bwt_chain overall rate (1.7%) is meaningless for extensionless Silesia
/// files that structurally benefit from BWT at 58% win rate on their domain.
fn calibrated_confidence(name: &str, ext: Option<&str>) -> Option<f64> {
    let cal = load_calibration().as_ref()?;
    let entry = cal.get(name)?;

    if let Some(extension) = ext {
        // Extension known — use per-extension rate when we have enough data
        let key = if extension.is_empty() {
            "(none)"
        } else {
            extension
        };
        if let Some(&(wr, fc)) = entry.by_extension.get(key) {
            if fc >= 5 {
                return Some(wr);
            }
        }
        // Known extension but sparse data — fall back to overall
        if entry.files >= 10 {
            return Some(entry.win_rate);
        }
    }

    // No extension (or no calibration data at all): return None so the
    // caller uses the SSR-structural default.  The overall rate is
    // unreliable here — it's dominated by whichever corpus has the most
    // files, not by structural similarity to this file.
    None
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
fn recommend_from_ssr(ssr: &SSRResult, ext: Option<&str>) -> Vec<TransformRecommendation> {
    let mut recs = Vec::new();

    let is_text = ssr.ascii_ratio > 0.80;
    let is_binary = ssr.ascii_ratio < 0.50;
    let size = ssr.data_size;

    // --- Tier 1: normalize (94.7% overall win-rate on structured text) ---
    if is_text {
        let confidence = calibrated_confidence("normalize", ext).unwrap_or(0.95);
        recs.push(TransformRecommendation {
            name: "normalize".to_string(),
            priority: 5,
            confidence,
        });
    }

    // --- Tier 1: bwt_chain (dominant on large text) ---
    // SA-IS BWT is O(n) — cap raised to 64 MiB (BWT_MAX_SIZE).
    // Per-extension calibration avoids dilution from small config files.
    // P7: Raised from 32 KB to 16 MB.  Benchmarks (dickens, nci, xml, mozilla)
    // show that BWT as a pre-transform before zstd-6 never improves ratio over
    // standalone zstd-6 on files under 16 MB — zstd's LZ77 already captures
    // the text redundancy that BWT would expose.  The BWT trial is expensive
    // (~80 ms on 10 MB) and produces no benefit, halving throughput.  Files
    // >= 16 MB go through the parallel path where P1 strips BWT anyway.
    if is_text && ssr.entropy_estimate < 5.5 && size > 16_000_000 && size <= 64_000_000 {
        let confidence = calibrated_confidence("bwt_chain", ext).unwrap_or(0.60);
        recs.push(TransformRecommendation {
            name: "bwt_chain".to_string(),
            priority: 10,
            confidence,
        });
    }

    // --- Tier 2: byte_plane (situational on binary) ---
    // Empirical: 22.5% gain on x-ray, strong on medical/scientific binary.
    // Confidence raised to 0.55 to pass SMART_MIN_CONFIDENCE (0.50).
    if is_binary && ssr.entropy_estimate < 6.0 {
        recs.push(TransformRecommendation {
            name: "byte_plane".to_string(),
            priority: 10,
            confidence: 0.55,
        });
    }

    // --- Tier 0: DoF elimination (const_elim, stride_elim) ---
    // const_elim: effective when >90% of bytes are the same value.
    // Calibration: 0.07% overall win rate — only fire on very low entropy.
    if ssr.entropy_estimate < 1.0 && size >= 64 {
        recs.push(TransformRecommendation {
            name: "const_elim".to_string(),
            priority: 1,
            confidence: 0.95,
        });
    }
    // stride_elim: effective on structured binary with fixed-width integer sequences.
    // SSR-gated; calibration dilutes — keep hardcoded.
    // Lowered from 0.70 to 0.30: calibration shows no standalone wins across
    // all corpora.  At 0.30 it participates in adaptive trials but won't be
    // a primary recommendation (SMART_MIN_CONFIDENCE = 0.50).
    if is_binary && ssr.entropy_estimate < 5.0 && size >= 64 {
        recs.push(TransformRecommendation {
            name: "stride_elim".to_string(),
            priority: 2,
            confidence: 0.30,
        });
    }

    // --- Tier 1.5: prediction (sequential correlation) ---
    // Effective on binary data with temporal/spatial correlation (medical
    // images, star catalogs, telemetry).  Transform study evidence:
    //   x-ray: -19.7%, mr: -8.7%, sao: -2.2% vs zstd-3 baseline.
    // Calibration overall win_rate (0.5%) is misleading because it includes
    // text/structured files where predict always loses.  SSR gates restrict
    // this to binary-only, so we keep the hardcoded confidence.
    if is_binary && ssr.entropy_estimate > 1.0 && ssr.entropy_estimate < 6.0 && size >= 64 {
        recs.push(TransformRecommendation {
            name: "predict".to_string(),
            priority: 4,
            confidence: 0.55,
        });
    }

    // --- condition: DISABLED ---
    // Calibration: 0% win rate across 1,368 files, zero gain on every file
    // type tested.  Removed to avoid evaluation overhead.

    // --- Tier 2: transpose (binary with fixed-width records) ---
    // Empirical: columnar layout dramatically improves entropy coding for
    // structured binary (database pages, sensor arrays, protocol headers).
    if is_binary && ssr.entropy_estimate < 7.0 && size >= 256 {
        recs.push(TransformRecommendation {
            name: "transpose".to_string(),
            priority: 8,
            confidence: 0.45, // Below SMART_MIN but participates in adaptive trials
        });
    }

    // --- Tier 2: float_split (binary with IEEE 754 patterns) ---
    // Separates exponent/mantissa streams for columnar compression.
    if is_binary && ssr.entropy_estimate < 6.5 && size >= 128 {
        recs.push(TransformRecommendation {
            name: "float_split".to_string(),
            priority: 7,
            confidence: 0.45,
        });
    }

    // --- Tier 3: rolz (medium-entropy text/binary with local patterns) ---
    // Legacy TP fallback handled ROLZ separately; now integrated into smart
    // path so it participates in adaptive trials alongside DAG transforms.
    if ssr.entropy_estimate > 3.5 && ssr.entropy_estimate < 6.5 && size >= 512 {
        recs.push(TransformRecommendation {
            name: "rolz".to_string(),
            priority: 6,
            confidence: 0.40,
        });
    }

    // NOTE: The following are deliberately NEVER recommended on raw Serial
    // data based on benchmark evidence:
    // - condition:     0% win rate on 1,368 files (zero effect, pure overhead)
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
        let avg_confidence = chain.iter().map(|r| r.confidence).sum::<f64>() / chain.len() as f64;
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
                r.name,
                r.priority,
                r.confidence * 100.0,
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
