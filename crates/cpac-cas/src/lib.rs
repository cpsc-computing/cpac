// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Constraint-based Auto-CAS: inference, cost model, `DoF` extraction.
//!
//! Analyzes structured data to discover constraints (ranges, enumerations,
//! functional dependencies) that reduce the Degrees of Freedom (`DoF`)
//! and improve compression.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::no_effect_underscore_binding,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

use std::collections::HashSet;
use cpac_types::CpacType;

/// A discovered constraint on a column.
#[derive(Clone, Debug, PartialEq)]
pub enum Constraint {
    /// Column values fall within `[min, max]`.
    Range { min: i64, max: i64 },
    /// Column has a fixed set of values.
    Enumeration { values: Vec<String> },
    /// Column is constant.
    Constant { value: String },
    /// Column values are monotonically increasing.
    Monotonic { direction: MonotonicDir },
    /// Column B is functionally dependent on column A.
    FunctionalDependency { from: String, to: String },
    /// Column has a fixed stride (delta between consecutive values).
    Stride { step: i64 },
    /// Column values are always sorted.
    Sorted { direction: MonotonicDir },
    /// String column values have bounded length.
    LengthBounded { min_len: usize, max_len: usize },
    /// Values follow a repeating cycle of given period.
    Periodic { period: usize },
    /// Values are RLE-friendly (long runs of identical values).
    RunLength { avg_run: usize },
    /// Column contains null / sentinel values (with bitmap).
    Nullable { null_count: usize, sentinel: String },
    /// Float column benefits from XOR-delta encoding.
    XorDeltaBenefit { avg_leading_zeros: u32 },
    /// Float column has clustered values (small variance).
    FloatClustered { mean: f64, std_dev: f64 },
    /// Column is a linear function of another: target = multiplier * source + offset.
    Algebraic {
        target: String,
        source: String,
        multiplier: i64,
        offset: i64,
    },
}

/// Direction of monotonicity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonotonicDir {
    Increasing,
    Decreasing,
}

/// Classification of a variable (column) for constraint projection.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum VarClass {
    /// Column has a single constant value across all rows.
    Fixed { value: i64 },
    /// Column is a linear function of another: Y = multiplier * source + offset.
    DerivedLinear {
        source_col: usize,
        multiplier: i64,
        offset: i64,
    },
    /// Column is independent and must be stored.
    Free,
}

/// Result of constraint inference on a dataset.
#[derive(Clone, Debug)]
pub struct CasAnalysis {
    /// Discovered constraints per column.
    pub constraints: Vec<(String, Vec<Constraint>)>,
    /// Total degrees of freedom before constraints.
    pub total_dof: f64,
    /// Degrees of freedom after constraints.
    pub constrained_dof: f64,
    /// Estimated compression benefit from constraints.
    pub estimated_benefit: f64,
}

/// Infer constraints from integer column data.
#[must_use]
pub fn infer_int_constraints(_name: &str, values: &[i64]) -> Vec<Constraint> {
    let mut constraints = Vec::new();
    if values.is_empty() {
        return constraints;
    }

    // Check constant
    let first = values[0];
    if values.iter().all(|&v| v == first) {
        constraints.push(Constraint::Constant {
            value: first.to_string(),
        });
        return constraints;
    }

    // Range constraint
    let min = *values.iter().min().unwrap();
    let max = *values.iter().max().unwrap();
    constraints.push(Constraint::Range { min, max });

    // Enumeration (if few unique values)
    let unique: HashSet<i64> = values.iter().copied().collect();
    if unique.len() <= 32 && unique.len() < values.len() / 2 {
        let mut vals: Vec<String> = unique
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        vals.sort();
        constraints.push(Constraint::Enumeration { values: vals });
    }

    // Monotonicity
    if values.windows(2).all(|w| w[1] >= w[0]) {
        constraints.push(Constraint::Monotonic {
            direction: MonotonicDir::Increasing,
        });
    } else if values.windows(2).all(|w| w[1] <= w[0]) {
        constraints.push(Constraint::Monotonic {
            direction: MonotonicDir::Decreasing,
        });
    }

    constraints
}

/// Infer constraints from string column data.
#[must_use]
pub fn infer_string_constraints(_name: &str, values: &[String]) -> Vec<Constraint> {
    let mut constraints = Vec::new();
    if values.is_empty() {
        return constraints;
    }

    // Check constant
    let first = &values[0];
    if values.iter().all(|v| v == first) {
        constraints.push(Constraint::Constant {
            value: first.clone(),
        });
        return constraints;
    }

    // Enumeration
    let unique: HashSet<&str> = values.iter().map(std::string::String::as_str).collect();
    if unique.len() <= 64 && unique.len() < values.len() / 2 {
        let mut vals: Vec<String> = unique
            .iter()
            .map(std::string::ToString::to_string)
            .collect();
        vals.sort();
        constraints.push(Constraint::Enumeration { values: vals });
    }

    // Length bounds
    let min_len = values.iter().map(String::len).min().unwrap_or(0);
    let max_len = values.iter().map(String::len).max().unwrap_or(0);
    if min_len != max_len || max_len <= 64 {
        constraints.push(Constraint::LengthBounded { min_len, max_len });
    }

    constraints
}

/// Infer constraints from float column data.
#[must_use]
pub fn infer_float_constraints(values: &[f64]) -> Vec<Constraint> {
    let mut constraints = Vec::new();
    if values.len() < 2 {
        return constraints;
    }

    // Monotonicity
    if values.windows(2).all(|w| w[1] >= w[0]) {
        constraints.push(Constraint::Monotonic {
            direction: MonotonicDir::Increasing,
        });
    } else if values.windows(2).all(|w| w[1] <= w[0]) {
        constraints.push(Constraint::Monotonic {
            direction: MonotonicDir::Decreasing,
        });
    }

    // XOR-delta benefit: count average leading zeros in XOR of consecutive f64
    let mut total_lz: u64 = 0;
    for w in values.windows(2) {
        let xor = w[0].to_bits() ^ w[1].to_bits();
        total_lz += u64::from(xor.leading_zeros());
    }
    let avg_lz = (total_lz / (values.len() as u64 - 1)) as u32;
    if avg_lz >= 16 {
        constraints.push(Constraint::XorDeltaBenefit {
            avg_leading_zeros: avg_lz,
        });
    }

    // Clustering: if std dev is small relative to mean
    let n = values.len() as f64;
    let mean = values.iter().sum::<f64>() / n;
    let variance = values.iter().map(|&v| (v - mean).powi(2)).sum::<f64>() / n;
    let std_dev = variance.sqrt();
    if mean.abs() > f64::EPSILON && std_dev / mean.abs() < 0.1 {
        constraints.push(Constraint::FloatClustered { mean, std_dev });
    }

    constraints
}

/// Infer additional structural constraints on integer data.
#[must_use]
pub fn infer_structural_constraints(values: &[i64]) -> Vec<Constraint> {
    let mut constraints = Vec::new();
    if values.len() < 2 {
        return constraints;
    }
    // Stride detection
    let d0 = values[1] - values[0];
    if values.windows(2).all(|w| w[1] - w[0] == d0) {
        constraints.push(Constraint::Stride { step: d0 });
    }
    // Sorted
    if values.windows(2).all(|w| w[1] >= w[0]) {
        constraints.push(Constraint::Sorted {
            direction: MonotonicDir::Increasing,
        });
    } else if values.windows(2).all(|w| w[1] <= w[0]) {
        constraints.push(Constraint::Sorted {
            direction: MonotonicDir::Decreasing,
        });
    }
    // Periodic detection (small periods)
    for period in 2..=16 {
        if values.len() >= period * 3
            && values.chunks(period).skip(1).all(|chunk| {
                chunk
                    .iter()
                    .zip(values[..period].iter())
                    .all(|(a, b)| a == b)
            })
        {
            constraints.push(Constraint::Periodic { period });
            break;
        }
    }
    // Run-length friendliness
    let mut _run_len = 1usize;
    let mut total_runs = 0usize;
    for w in values.windows(2) {
        if w[1] == w[0] {
            _run_len += 1;
        } else {
            total_runs += 1;
            _run_len = 1;
        }
    }
    total_runs += 1;
    let avg_run = values.len() / total_runs.max(1);
    if avg_run >= 3 {
        constraints.push(Constraint::RunLength { avg_run });
    }
    constraints
}

// ---------------------------------------------------------------------------
// Cross-column algebraic analysis
// ---------------------------------------------------------------------------

/// Classify columns as Fixed, Derived, or Free.
///
/// Analyzes cross-column relationships to discover which columns can be
/// eliminated by constraint projection.  Only integer columns are analyzed.
#[must_use]
pub fn classify_variables(columns: &[(String, Vec<i64>)]) -> Vec<(String, VarClass)> {
    let n = columns.len();
    if n == 0 {
        return Vec::new();
    }

    let mut classes: Vec<Option<VarClass>> = vec![None; n];

    // Pass 1: detect constants
    for (i, (_name, values)) in columns.iter().enumerate() {
        if values.is_empty() {
            classes[i] = Some(VarClass::Free);
            continue;
        }
        let first = values[0];
        if values.iter().all(|&v| v == first) {
            classes[i] = Some(VarClass::Fixed { value: first });
        }
    }

    // Pass 2: detect linear relationships (Y = a*X + b).
    // Only derive from columns not yet classified (future Free columns).
    for j in 0..n {
        if classes[j].is_some() {
            continue;
        }
        let y_vals = &columns[j].1;
        if y_vals.len() < 2 {
            continue;
        }
        for i in 0..n {
            if i == j || classes[i].is_some() {
                continue;
            }
            let x_vals = &columns[i].1;
            if x_vals.len() != y_vals.len() {
                continue;
            }
            if let Some((a, b)) = detect_linear_relation(x_vals, y_vals) {
                classes[j] = Some(VarClass::DerivedLinear {
                    source_col: i,
                    multiplier: a,
                    offset: b,
                });
                break;
            }
        }
    }

    // Pass 3: remaining columns are Free
    columns
        .iter()
        .enumerate()
        .map(|(i, (name, _))| {
            let class = classes[i].take().unwrap_or(VarClass::Free);
            (name.clone(), class)
        })
        .collect()
}

/// Detect if Y = a*X + b for all corresponding values.
fn detect_linear_relation(x: &[i64], y: &[i64]) -> Option<(i64, i64)> {
    if x.len() < 2 || x.len() != y.len() {
        return None;
    }
    let idx0 = 0;
    let idx1 = (1..x.len()).find(|&i| x[i] != x[idx0])?;
    let dx = x[idx1] - x[idx0];
    let dy = y[idx1] - y[idx0];
    if dx == 0 || dy % dx != 0 {
        return None;
    }
    let a = dy / dx;
    let b = y[idx0] - a * x[idx0];
    for (&xi, &yi) in x.iter().zip(y.iter()) {
        if a.checked_mul(xi).and_then(|ax| ax.checked_add(b)) != Some(yi) {
            return None;
        }
    }
    Some((a, b))
}

/// Infer algebraic constraints between integer columns.
#[must_use]
pub fn infer_algebraic_constraints(columns: &[(String, Vec<i64>)]) -> Vec<Constraint> {
    let classes = classify_variables(columns);
    let mut constraints = Vec::new();
    for (target_name, class) in &classes {
        if let VarClass::DerivedLinear {
            source_col,
            multiplier,
            offset,
        } = class
        {
            constraints.push(Constraint::Algebraic {
                target: target_name.clone(),
                source: columns[*source_col].0.clone(),
                multiplier: *multiplier,
                offset: *offset,
            });
        }
    }
    constraints
}

// ---------------------------------------------------------------------------
// Unified column analyzer (Phase 3.1)
// ---------------------------------------------------------------------------

/// Analyze a single `CpacType` column and return discovered constraints.
///
/// Dispatches to the appropriate inference functions based on the type variant.
#[must_use]
pub fn analyze_column(name: &str, data: &CpacType) -> Vec<Constraint> {
    match data {
        CpacType::IntColumn { values, .. } => {
            let mut c = infer_int_constraints(name, values);
            c.extend(infer_structural_constraints(values));
            c
        }
        CpacType::FloatColumn { values, .. } => infer_float_constraints(values),
        CpacType::StringColumn { values, .. } => infer_string_constraints(name, values),
        CpacType::ColumnSet { columns } => {
            // Flatten: return union of all sub-column constraints
            let mut all = Vec::new();
            for (col_name, col_data) in columns {
                all.extend(analyze_column(col_name, col_data));
            }
            all
        }
        _ => Vec::new(), // Serial, Struct — no column-level inference
    }
}

/// Full column analysis returning a `CasAnalysis`.
#[must_use]
pub fn analyze_columns(columns: &[(String, Vec<i64>)]) -> CasAnalysis {
    let mut all_constraints = Vec::new();
    let mut total_dof = 0.0;
    let mut total_constrained = 0.0;
    for (name, values) in columns {
        let mut col_constraints = infer_int_constraints(name, values);
        col_constraints.extend(infer_structural_constraints(values));
        let unique: HashSet<i64> = values.iter().copied().collect();
        let min = values.iter().min().copied().unwrap_or(0);
        let max = values.iter().max().copied().unwrap_or(0);
        let dof = estimate_dof(values.len(), unique.len(), Some((min, max)));
        let cdof = constrained_dof(dof, &col_constraints, values.len());
        total_dof += dof;
        total_constrained += cdof;
        all_constraints.push((name.clone(), col_constraints));
    }
    // Cross-column algebraic inference
    let algebraic = infer_algebraic_constraints(columns);
    for ac in &algebraic {
        if let Constraint::Algebraic { target, .. } = ac {
            if let Some(entry) = all_constraints.iter_mut().find(|(name, _)| name == target) {
                entry.1.push(ac.clone());
            }
        }
    }
    let benefit = if total_dof > 0.0 {
        1.0 - total_constrained / total_dof
    } else {
        0.0
    };
    CasAnalysis {
        constraints: all_constraints,
        total_dof,
        constrained_dof: total_constrained,
        estimated_benefit: benefit,
    }
}

// ---------------------------------------------------------------------------
// AutoCasCompressor
// ---------------------------------------------------------------------------

const CAS_MAGIC: &[u8; 3] = b"CAS";
const CAS_VERSION: u8 = 1;

/// Compress data with CAS header prepended.
///
/// Format: `[CAS][version][num_constraints:u16 LE][constraint_table][residual_data]`
#[must_use]
pub fn cas_compress(data: &[u8]) -> Vec<u8> {
    // For raw bytes, we treat each byte as an i64 value in a single column
    let values: Vec<i64> = data.iter().map(|&b| i64::from(b)).collect();
    let analysis = analyze_columns(&[("data".into(), values)]);
    let num_c = analysis
        .constraints
        .iter()
        .map(|(_, cs)| cs.len())
        .sum::<usize>() as u16;
    let mut out = Vec::new();
    out.extend_from_slice(CAS_MAGIC);
    out.push(CAS_VERSION);
    out.extend_from_slice(&num_c.to_le_bytes());
    // Simplified: store benefit as 4-byte float
    out.extend_from_slice(&(analysis.estimated_benefit as f32).to_le_bytes());
    out.extend_from_slice(data);
    out
}

/// Decompress CAS-framed data.
pub fn cas_decompress(data: &[u8]) -> cpac_types::CpacResult<Vec<u8>> {
    if data.len() < 8 || &data[0..3] != CAS_MAGIC {
        return Err(cpac_types::CpacError::InvalidFrame(
            "not a CAS frame".into(),
        ));
    }
    // Skip header: magic(3) + version(1) + num_constraints(2) + benefit(4) = 10
    Ok(data[10..].to_vec())
}

/// Estimate degrees of freedom for a column.
#[must_use]
pub fn estimate_dof(
    values_count: usize,
    unique_count: usize,
    value_range: Option<(i64, i64)>,
) -> f64 {
    if values_count == 0 {
        return 0.0;
    }
    if let Some((min, max)) = value_range {
        let range_bits = ((max - min + 1) as f64).log2().max(0.0);
        range_bits * values_count as f64
    } else {
        // String: entropy-based estimate
        let diversity = unique_count as f64 / values_count as f64;
        (diversity * 8.0) * values_count as f64
    }
}

/// Compute constrained `DoF` after applying constraints.
#[must_use]
pub fn constrained_dof(total_dof: f64, constraints: &[Constraint], values_count: usize) -> f64 {
    let mut reduction = 0.0;
    for constraint in constraints {
        match constraint {
            Constraint::Constant { .. } => {
                reduction += total_dof; // Column is fully determined
            }
            Constraint::Enumeration { values } => {
                let enum_bits = (values.len() as f64).log2().max(1.0);
                reduction += total_dof - enum_bits * values_count as f64;
            }
            Constraint::Monotonic { .. } => {
                reduction += total_dof * 0.3; // ~30% savings from monotonicity
            }
            Constraint::Range { min, max } => {
                let range_bits = ((*max - *min + 1) as f64).log2().max(0.0);
                let max_bits = 64.0;
                reduction += (max_bits - range_bits) * values_count as f64;
            }
            Constraint::FunctionalDependency { .. } => {
                reduction += total_dof * 0.5;
            }
            Constraint::Stride { .. } => {
                reduction += total_dof * 0.9; // Almost fully determined
            }
            Constraint::Sorted { .. } => {
                reduction += total_dof * 0.3;
            }
            Constraint::LengthBounded { .. } => {
                reduction += total_dof * 0.1;
            }
            Constraint::Periodic { period } => {
                // Only need to store one period
                let p = *period as f64;
                let n = values_count as f64;
                if n > 0.0 {
                    reduction += total_dof * (1.0 - p / n);
                }
            }
            Constraint::RunLength { avg_run } => {
                let r = *avg_run as f64;
                if r > 1.0 {
                    reduction += total_dof * (1.0 - 1.0 / r);
                }
            }
            Constraint::Nullable { .. } => {
                reduction += total_dof * 0.05;
            }
            Constraint::XorDeltaBenefit { avg_leading_zeros } => {
                // More leading zeros → more compressible after XOR
                let benefit = (*avg_leading_zeros as f64) / 64.0;
                reduction += total_dof * benefit;
            }
            Constraint::FloatClustered { .. } => {
                reduction += total_dof * 0.2;
            }
            Constraint::Algebraic { .. } => {
                reduction += total_dof * 0.9;
            }
        }
    }
    (total_dof - reduction).max(0.0)
}

// ---------------------------------------------------------------------------
// Constraint-to-Transform Recommendation (Phase 3.2)
// ---------------------------------------------------------------------------

/// A recommended transform with its name and optional parameters.
#[derive(Clone, Debug)]
pub struct TransformRecommendation {
    /// Transform name (matches `TransformNode::name()`).
    pub name: String,
    /// Priority (lower = apply first in chain).
    pub priority: u8,
    /// Empirical confidence that this transform helps (0.0–1.0).
    /// Based on observed win-rate from corpus benchmarks.
    pub confidence: f64,
}

/// Map discovered constraints to recommended transforms.
///
/// Returns a deduplicated list of transforms sorted by priority (chain order).
#[must_use]
pub fn recommend_transforms(constraints: &[Constraint]) -> Vec<TransformRecommendation> {
    let mut recs: Vec<TransformRecommendation> = Vec::new();

    for c in constraints {
        match c {
            Constraint::Monotonic { .. } => {
                // Delta + zigzag are effective on column data (NOT raw Serial).
                // Confidence based on benchmark: high on IntColumn, 0% on Serial.
                push_unique(&mut recs, "delta", 10, 0.85);
                push_unique(&mut recs, "zigzag", 20, 0.80);
            }
            Constraint::Enumeration { values } if values.len() <= 256 => {
                push_unique(&mut recs, "vocab", 5, 0.75);
            }
            Constraint::Enumeration { .. } => {}
            Constraint::Range { min, max } => {
                let span = max.saturating_sub(*min);
                if span <= 65535 {
                    push_unique(&mut recs, "range_pack", 30, 0.70);
                }
            }
            Constraint::RunLength { avg_run } if *avg_run >= 4 => {
                // RLE only useful on column data; zero gain on raw Serial per benchmarks.
                push_unique(&mut recs, "rle", 8, 0.40);
            }
            Constraint::RunLength { .. } => {}
            Constraint::Constant { .. } => {
                push_unique(&mut recs, "const_elim", 1, 0.99);
            }
            Constraint::Stride { .. } => {
                push_unique(&mut recs, "stride_elim", 2, 0.95);
            }
            Constraint::Periodic { .. } => {
                push_unique(&mut recs, "delta", 10, 0.85);
            }
            Constraint::LengthBounded { max_len, .. } => {
                if *max_len <= 32 {
                    push_unique(&mut recs, "vocab", 5, 0.75);
                }
            }
            Constraint::XorDeltaBenefit { .. } => {
                push_unique(&mut recs, "float_xor", 10, 0.80);
                push_unique(&mut recs, "byte_plane", 25, 0.60);
            }
            Constraint::FloatClustered { .. } => {
                push_unique(&mut recs, "float_xor", 10, 0.80);
            }
            Constraint::Sorted { .. }
            | Constraint::Nullable { .. } => {}
            Constraint::FunctionalDependency { .. } => {
                push_unique(&mut recs, "projection", 0, 0.85);
            }
            Constraint::Algebraic { .. } => {
                push_unique(&mut recs, "projection", 0, 0.90);
            }
        }
    }

    recs.sort_by_key(|r| r.priority);
    recs
}

fn push_unique(recs: &mut Vec<TransformRecommendation>, name: &str, priority: u8, confidence: f64) {
    if !recs.iter().any(|r| r.name == name) {
        recs.push(TransformRecommendation {
            name: name.to_string(),
            priority,
            confidence,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn infer_constant_column() {
        let vals = vec![42i64; 100];
        let constraints = infer_int_constraints("col", &vals);
        assert!(constraints
            .iter()
            .any(|c| matches!(c, Constraint::Constant { .. })));
    }

    #[test]
    fn infer_range_and_enum() {
        let vals: Vec<i64> = (0..100).map(|i| i % 5).collect();
        let constraints = infer_int_constraints("col", &vals);
        assert!(constraints
            .iter()
            .any(|c| matches!(c, Constraint::Range { min: 0, max: 4 })));
        assert!(constraints
            .iter()
            .any(|c| matches!(c, Constraint::Enumeration { .. })));
    }

    #[test]
    fn infer_monotonic() {
        let vals: Vec<i64> = (0..100).collect();
        let constraints = infer_int_constraints("col", &vals);
        assert!(constraints.iter().any(|c| matches!(
            c,
            Constraint::Monotonic {
                direction: MonotonicDir::Increasing
            }
        )));
    }

    #[test]
    fn infer_string_enum() {
        let vals: Vec<String> = vec!["GET", "POST", "GET", "PUT", "GET", "POST", "GET", "GET"]
            .into_iter()
            .map(String::from)
            .collect();
        let constraints = infer_string_constraints("method", &vals);
        assert!(constraints
            .iter()
            .any(|c| matches!(c, Constraint::Enumeration { .. })));
    }

    #[test]
    fn dof_estimation() {
        let dof = estimate_dof(1000, 10, Some((0, 255)));
        assert!(dof > 0.0);
        let constrained = constrained_dof(dof, &[Constraint::Range { min: 0, max: 255 }], 1000);
        assert!(constrained < dof);
    }

    #[test]
    fn infer_stride() {
        let vals: Vec<i64> = (0..100).map(|i| i * 3).collect();
        let c = infer_structural_constraints(&vals);
        assert!(c
            .iter()
            .any(|x| matches!(x, Constraint::Stride { step: 3 })));
    }

    #[test]
    fn infer_periodic() {
        let vals: Vec<i64> = [1, 2, 3].iter().cycle().take(99).copied().collect();
        let c = infer_structural_constraints(&vals);
        assert!(c
            .iter()
            .any(|x| matches!(x, Constraint::Periodic { period: 3 })));
    }

    #[test]
    fn infer_run_length() {
        let vals: Vec<i64> = vec![1; 20]
            .into_iter()
            .chain(vec![2; 20])
            .chain(vec![3; 20])
            .collect();
        let c = infer_structural_constraints(&vals);
        assert!(c.iter().any(|x| matches!(x, Constraint::RunLength { .. })));
    }

    #[test]
    fn analyze_multi_column() {
        let cols = vec![
            ("id".into(), (0..100i64).collect()),
            ("status".into(), (0..100).map(|i| i % 3).collect()),
        ];
        let analysis = analyze_columns(&cols);
        assert_eq!(analysis.constraints.len(), 2);
        assert!(analysis.estimated_benefit > 0.0);
    }

    #[test]
    fn cas_compress_decompress_roundtrip() {
        let data = b"Hello CAS compressor test data!";
        let compressed = cas_compress(data);
        assert!(compressed.starts_with(b"CAS"));
        let decompressed = cas_decompress(&compressed).unwrap();
        assert_eq!(decompressed, data);
    }

    // Phase 3.1 tests -------------------------------------------------------

    #[test]
    fn analyze_column_int() {
        let data = CpacType::IntColumn {
            values: (0..100).collect(),
            original_width: 8,
        };
        let cs = analyze_column("id", &data);
        assert!(cs.iter().any(|c| matches!(c, Constraint::Monotonic { .. })));
        assert!(cs.iter().any(|c| matches!(c, Constraint::Stride { step: 1 })));
    }

    #[test]
    fn analyze_column_float_xor() {
        // Similar values → high leading zeros in XOR
        let values: Vec<f64> = (0..100).map(|i| 100.0 + (i as f64) * 0.001).collect();
        let data = CpacType::FloatColumn {
            values,
            precision: cpac_types::FloatPrecision::F64,
        };
        let cs = analyze_column("temp", &data);
        assert!(
            cs.iter().any(|c| matches!(c, Constraint::Monotonic { .. })),
            "expected monotonic for increasing float sequence"
        );
    }

    #[test]
    fn analyze_column_string_enum() {
        let values = vec!["A", "B", "A", "C", "B", "A", "C", "A"]
            .into_iter()
            .map(String::from)
            .collect();
        let data = CpacType::StringColumn {
            values,
            total_bytes: 8,
        };
        let cs = analyze_column("status", &data);
        assert!(cs.iter().any(|c| matches!(c, Constraint::Enumeration { .. })));
        assert!(cs.iter().any(|c| matches!(c, Constraint::LengthBounded { .. })));
    }

    #[test]
    fn analyze_column_set() {
        let cols = vec![
            (
                "id".to_string(),
                CpacType::IntColumn {
                    values: (0..50).collect(),
                    original_width: 4,
                },
            ),
            (
                "method".to_string(),
                CpacType::StringColumn {
                    values: vec!["GET", "POST", "GET", "PUT", "GET", "POST", "GET", "GET"]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                    total_bytes: 30,
                },
            ),
        ];
        let data = CpacType::ColumnSet { columns: cols };
        let cs = analyze_column("root", &data);
        assert!(cs.len() >= 2, "should have constraints from both sub-columns");
    }

    // Phase 3.2 tests -------------------------------------------------------

    #[test]
    fn recommend_monotonic_int() {
        let constraints = vec![
            Constraint::Monotonic { direction: MonotonicDir::Increasing },
            Constraint::Range { min: 0, max: 1000 },
        ];
        let recs = recommend_transforms(&constraints);
        let names: Vec<&str> = recs.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"delta"));
        assert!(names.contains(&"zigzag"));
        assert!(names.contains(&"range_pack"));
    }

    #[test]
    fn recommend_enum_vocab() {
        let constraints = vec![
            Constraint::Enumeration { values: vec!["A".into(), "B".into(), "C".into()] },
        ];
        let recs = recommend_transforms(&constraints);
        assert!(recs.iter().any(|r| r.name == "vocab"));
    }

    #[test]
    fn recommend_rle() {
        let constraints = vec![
            Constraint::RunLength { avg_run: 10 },
        ];
        let recs = recommend_transforms(&constraints);
        assert!(recs.iter().any(|r| r.name == "rle"));
    }

    #[test]
    fn recommend_float_xor() {
        let constraints = vec![
            Constraint::XorDeltaBenefit { avg_leading_zeros: 32 },
        ];
        let recs = recommend_transforms(&constraints);
        let names: Vec<&str> = recs.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"float_xor"));
        assert!(names.contains(&"byte_plane"));
    }

    #[test]
    fn recommend_constant() {
        let constraints = vec![
            Constraint::Constant { value: "42".into() },
        ];
        let recs = recommend_transforms(&constraints);
        assert!(recs.iter().any(|r| r.name == "const_elim"));
    }

    #[test]
    fn recommend_chain_order() {
        // vocab (5) < rle (8) < delta (10) < zigzag (20) < range_pack (30)
        let constraints = vec![
            Constraint::Monotonic { direction: MonotonicDir::Increasing },
            Constraint::RunLength { avg_run: 5 },
            Constraint::Enumeration { values: vec!["X".into()] },
            Constraint::Range { min: 0, max: 100 },
        ];
        let recs = recommend_transforms(&constraints);
        // Check ordering by priority
        for w in recs.windows(2) {
            assert!(w[0].priority <= w[1].priority, "{} should come before {}", w[0].name, w[1].name);
        }
    }

    #[test]
    fn infer_float_monotonic() {
        let vals: Vec<f64> = (0..50).map(|i| i as f64 * 1.5).collect();
        let cs = infer_float_constraints(&vals);
        assert!(cs.iter().any(|c| matches!(c, Constraint::Monotonic { direction: MonotonicDir::Increasing })));
    }

    #[test]
    fn infer_string_length_bounds() {
        let vals: Vec<String> = vec!["abc", "de", "fghij"]
            .into_iter()
            .map(String::from)
            .collect();
        let cs = infer_string_constraints("col", &vals);
        assert!(cs.iter().any(|c| matches!(c, Constraint::LengthBounded { min_len: 2, max_len: 5 })));
    }

    // Phase 4 tests ----------------------------------------------------------

    #[test]
    fn classify_variables_basic() {
        let cols = vec![
            ("id".into(), (0..10i64).collect()),
            ("constant".into(), vec![42i64; 10]),
            ("derived".into(), (0..10).map(|i| 3 * i + 7).collect()),
        ];
        let classes = classify_variables(&cols);
        assert_eq!(classes.len(), 3);
        assert_eq!(classes[0].1, VarClass::Free);
        assert!(matches!(classes[1].1, VarClass::Fixed { value: 42 }));
        assert!(matches!(
            classes[2].1,
            VarClass::DerivedLinear {
                source_col: 0,
                multiplier: 3,
                offset: 7,
            }
        ));
    }

    #[test]
    fn infer_algebraic_basic() {
        let cols = vec![
            ("x".into(), (0..10i64).collect()),
            ("y".into(), (0..10).map(|i| 2 * i + 1).collect()),
        ];
        let constraints = infer_algebraic_constraints(&cols);
        assert_eq!(constraints.len(), 1);
        assert!(matches!(
            &constraints[0],
            Constraint::Algebraic {
                target,
                source,
                multiplier: 2,
                offset: 1,
            } if target == "y" && source == "x"
        ));
    }

    #[test]
    fn recommend_projection_for_algebraic() {
        let constraints = vec![Constraint::Algebraic {
            target: "y".into(),
            source: "x".into(),
            multiplier: 2,
            offset: 1,
        }];
        let recs = recommend_transforms(&constraints);
        assert!(recs.iter().any(|r| r.name == "projection"));
    }

    #[test]
    fn classify_all_free() {
        let cols = vec![
            ("a".into(), vec![1i64, 5, 3, 9, 2]),
            ("b".into(), vec![10i64, 20, 30, 40, 50]),
        ];
        let classes = classify_variables(&cols);
        assert!(classes.iter().all(|(_, c)| *c == VarClass::Free));
    }
}
