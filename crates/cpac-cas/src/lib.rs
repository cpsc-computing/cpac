// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Constraint-based Auto-CAS: inference, cost model, DoF extraction.
//!
//! Analyzes structured data to discover constraints (ranges, enumerations,
//! functional dependencies) that reduce the Degrees of Freedom (DoF)
//! and improve compression.

use std::collections::HashSet;

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
}

/// Direction of monotonicity.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MonotonicDir {
    Increasing,
    Decreasing,
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
        let mut vals: Vec<String> = unique.iter().map(|v| v.to_string()).collect();
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
    let unique: HashSet<&str> = values.iter().map(|s| s.as_str()).collect();
    if unique.len() <= 64 && unique.len() < values.len() / 2 {
        let mut vals: Vec<String> = unique.iter().map(|s| s.to_string()).collect();
        vals.sort();
        constraints.push(Constraint::Enumeration { values: vals });
    }

    constraints
}

/// Infer additional structural constraints on integer data.
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

/// Full column analysis returning a `CasAnalysis`.
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
pub fn cas_compress(data: &[u8]) -> Vec<u8> {
    // For raw bytes, we treat each byte as an i64 value in a single column
    let values: Vec<i64> = data.iter().map(|&b| b as i64).collect();
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
pub fn estimate_dof(
    values_count: usize,
    unique_count: usize,
    value_range: Option<(i64, i64)>,
) -> f64 {
    if values_count == 0 {
        return 0.0;
    }
    match value_range {
        Some((min, max)) => {
            let range_bits = ((max - min + 1) as f64).log2().max(0.0);
            range_bits * values_count as f64
        }
        None => {
            // String: entropy-based estimate
            let diversity = unique_count as f64 / values_count as f64;
            (diversity * 8.0) * values_count as f64
        }
    }
}

/// Compute constrained DoF after applying constraints.
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
        }
    }
    (total_dof - reduction).max(0.0)
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
}
