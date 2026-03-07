// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Trial runner — applies transform chains and measures compression deltas.

use crate::collector::CorpusFile;
use crate::experiment::{Experiment, TransformSpec, TrialDef};
use cpac_dag::registry::TransformRegistry;
use cpac_transforms::traits::TransformContext;
use cpac_types::{CpacResult, CpacType};
use std::collections::HashMap;
use std::time::Instant;

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// Result of a single trial on a single file.
#[derive(Clone, Debug, serde::Serialize)]
pub struct TrialResult {
    /// File name.
    pub file_name: String,
    /// File extension.
    pub extension: String,
    /// Relative directory.
    pub rel_dir: String,
    /// Top-level directory (first path component of rel_dir).
    pub top_dir: String,
    /// Trial name.
    pub trial_name: String,
    /// Raw file size in bytes.
    pub raw_size: usize,
    /// Baseline compressed size: zstd(original).
    pub z_baseline: usize,
    /// Compressed size after transform chain: zstd(transform(data)).
    pub z_after: usize,
    /// Net gain in bytes (positive = transform helped).
    pub net_gain: i64,
    /// Transform execution time in microseconds.
    pub transform_us: u64,
    /// Whether a transform was actually applied (chain was non-empty and succeeded).
    pub applied: bool,
    /// Detected MSN domain (if msn_extract was used).
    pub msn_domain: Option<String>,
    /// Error message if transform failed.
    pub error: Option<String>,
}

/// Aggregated results for a group of files.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct GroupResult {
    /// Group key.
    pub key: String,
    /// Trial name.
    pub trial_name: String,
    /// Number of files.
    pub file_count: usize,
    /// Files where transform was applied.
    pub applied_count: usize,
    /// Files where net_gain > 0.
    pub helped_count: usize,
    /// Files where net_gain < 0.
    pub hurt_count: usize,
    /// Sum of z_baseline across files.
    pub total_z_baseline: i64,
    /// Sum of z_after across files.
    pub total_z_after: i64,
    /// Total net gain.
    pub total_net_gain: i64,
    /// Best net gain (single file).
    pub best_gain: i64,
    /// Worst net gain (single file).
    pub worst_gain: i64,
}

// ---------------------------------------------------------------------------
// zstd helper
// ---------------------------------------------------------------------------

fn zstd_compress(data: &[u8], level: i32) -> Vec<u8> {
    zstd::encode_all(std::io::Cursor::new(data), level).expect("zstd encode failed")
}

// ---------------------------------------------------------------------------
// Transform chain execution
// ---------------------------------------------------------------------------

/// Apply a chain of transforms to raw data.
///
/// Returns the transformed bytes (serialized back to `Serial`).
fn apply_transform_chain(
    data: &[u8],
    chain: &[TransformSpec],
    registry: &TransformRegistry,
) -> CpacResult<Vec<u8>> {
    if chain.is_empty() {
        return Ok(data.to_vec());
    }

    let ssr = cpac_ssr::analyze(data);
    let ctx = TransformContext {
        entropy_estimate: ssr.entropy_estimate,
        ascii_ratio: ssr.ascii_ratio,
        data_size: data.len(),
    };

    let mut current = CpacType::Serial(data.to_vec());

    for spec in chain {
        let transform = registry.get_by_name(&spec.name).ok_or_else(|| {
            cpac_types::CpacError::Transform(format!("unknown transform: {}", spec.name))
        })?;

        // Check if transform accepts the current type
        let tag = current.tag();
        if !transform.accepts().contains(&tag) {
            // Skip incompatible transforms silently
            continue;
        }

        let (output, _meta) = transform.encode(current, &ctx)?;
        current = output;
    }

    // Serialize back to bytes
    match current {
        CpacType::Serial(bytes) => Ok(bytes),
        CpacType::IntColumn { values, .. } => {
            // Pack i64 values as little-endian bytes
            let mut out = Vec::with_capacity(values.len() * 8);
            for v in &values {
                out.extend_from_slice(&v.to_le_bytes());
            }
            Ok(out)
        }
        CpacType::FloatColumn { values, .. } => {
            let mut out = Vec::with_capacity(values.len() * 8);
            for v in &values {
                out.extend_from_slice(&v.to_le_bytes());
            }
            Ok(out)
        }
        CpacType::StringColumn { values, .. } => {
            let joined = values.join("\n");
            Ok(joined.into_bytes())
        }
        _ => {
            // For Struct/ColumnSet, flatten to bytes
            Err(cpac_types::CpacError::Transform(
                "cannot serialize complex type to bytes".into(),
            ))
        }
    }
}

// ---------------------------------------------------------------------------
// Single trial execution
// ---------------------------------------------------------------------------

/// Run a single trial on a single file.
#[must_use]
pub fn run_trial(
    file: &CorpusFile,
    data: &[u8],
    trial: &TrialDef,
    zstd_level: i32,
    registry: &TransformRegistry,
) -> TrialResult {
    let level = trial.zstd_level.unwrap_or(zstd_level);
    let z_baseline = zstd_compress(data, level).len();

    let top_dir = if file.rel_dir.is_empty() {
        "(root)".to_string()
    } else {
        file.rel_dir
            .split(['/', '\\'])
            .next()
            .unwrap_or(&file.rel_dir)
            .to_string()
    };

    // If no transforms and no MSN, this is the baseline
    if trial.transforms.is_empty() && !trial.msn_extract {
        return TrialResult {
            file_name: file.name.clone(),
            extension: file.extension.clone(),
            rel_dir: file.rel_dir.clone(),
            top_dir,
            trial_name: trial.name.clone(),
            raw_size: data.len(),
            z_baseline,
            z_after: z_baseline,
            net_gain: 0,
            transform_us: 0,
            applied: false,
            msn_domain: None,
            error: None,
        };
    }

    let start = Instant::now();

    // Optional MSN extraction first
    let (work_data, msn_domain) = if trial.msn_extract {
        match cpac_msn::extract(data, file.path.to_str(), trial.msn_confidence) {
            Ok(result) if result.applied => {
                let meta_bytes =
                    cpac_msn::encode_metadata_compact(&result.metadata()).unwrap_or_default();
                let mut combined = meta_bytes;
                combined.extend_from_slice(&result.residual);
                (combined, result.domain_id)
            }
            Ok(_) => (data.to_vec(), None),
            Err(_) => (data.to_vec(), None),
        }
    } else {
        (data.to_vec(), None)
    };

    // Apply transform chain
    let transform_result = apply_transform_chain(&work_data, &trial.transforms, registry);

    let elapsed = start.elapsed();

    match transform_result {
        Ok(transformed) => {
            let z_after = zstd_compress(&transformed, level).len();
            TrialResult {
                file_name: file.name.clone(),
                extension: file.extension.clone(),
                rel_dir: file.rel_dir.clone(),
                top_dir,
                trial_name: trial.name.clone(),
                raw_size: data.len(),
                z_baseline,
                z_after,
                net_gain: z_baseline as i64 - z_after as i64,
                transform_us: elapsed.as_micros() as u64,
                applied: true,
                msn_domain,
                error: None,
            }
        }
        Err(e) => TrialResult {
            file_name: file.name.clone(),
            extension: file.extension.clone(),
            rel_dir: file.rel_dir.clone(),
            top_dir,
            trial_name: trial.name.clone(),
            raw_size: data.len(),
            z_baseline,
            z_after: z_baseline,
            net_gain: 0,
            transform_us: elapsed.as_micros() as u64,
            applied: false,
            msn_domain,
            error: Some(e.to_string()),
        },
    }
}

// ---------------------------------------------------------------------------
// Corpus sweep
// ---------------------------------------------------------------------------

/// Run all trials across all files in the corpus.
///
/// The `progress_fn` callback is called for each file processed with
/// `(file_index, total_files, file_name)`.
pub fn run_experiment(
    experiment: &Experiment,
    files: &[CorpusFile],
    registry: &TransformRegistry,
    progress_fn: impl Fn(usize, usize, &str),
) -> Vec<TrialResult> {
    let total = files.len();
    let mut all_results = Vec::with_capacity(total * experiment.trials.len());

    for (i, file) in files.iter().enumerate() {
        progress_fn(i, total, &file.name);

        let data = match std::fs::read(&file.path) {
            Ok(d) => d,
            Err(_) => continue,
        };

        for trial in &experiment.trials {
            // Apply trial-level filters
            if !trial.ext_filter.is_empty()
                && !trial.ext_filter.iter().any(|e| e == &file.extension)
            {
                continue;
            }

            let result = run_trial(file, &data, trial, experiment.zstd_level, registry);
            all_results.push(result);
        }
    }

    all_results
}

// ---------------------------------------------------------------------------
// Aggregation
// ---------------------------------------------------------------------------

/// Aggregate trial results by a grouping key.
#[must_use]
pub fn aggregate_results(
    results: &[TrialResult],
    key_fn: impl Fn(&TrialResult) -> String,
) -> Vec<GroupResult> {
    let mut groups: HashMap<(String, String), GroupResult> = HashMap::new();

    for r in results {
        let key = key_fn(r);
        let entry = groups
            .entry((key.clone(), r.trial_name.clone()))
            .or_insert_with(|| GroupResult {
                key: key.clone(),
                trial_name: r.trial_name.clone(),
                ..GroupResult::default()
            });

        entry.file_count += 1;
        if r.applied {
            entry.applied_count += 1;
        }
        if r.net_gain > 0 {
            entry.helped_count += 1;
        }
        if r.net_gain < 0 {
            entry.hurt_count += 1;
        }
        entry.total_z_baseline += r.z_baseline as i64;
        entry.total_z_after += r.z_after as i64;
        entry.total_net_gain += r.net_gain;
        if r.net_gain > entry.best_gain {
            entry.best_gain = r.net_gain;
        }
        if r.net_gain < entry.worst_gain {
            entry.worst_gain = r.net_gain;
        }
    }

    let mut out: Vec<GroupResult> = groups.into_values().collect();
    out.sort_by(|a, b| {
        a.trial_name
            .cmp(&b.trial_name)
            .then(b.total_net_gain.cmp(&a.total_net_gain))
    });
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_empty_chain_is_passthrough() {
        let data = b"hello world";
        let registry = TransformRegistry::with_builtins();
        let result = apply_transform_chain(data, &[], &registry).unwrap();
        assert_eq!(result, data);
    }

    #[test]
    fn apply_delta_chain() {
        let data: Vec<u8> = (0u8..=100).collect();
        let registry = TransformRegistry::with_builtins();
        let chain = vec![TransformSpec {
            name: "delta".into(),
            params: serde_json::Value::Null,
        }];
        let result = apply_transform_chain(&data, &chain, &registry).unwrap();
        // Delta of sequential bytes: first stays, rest become 1
        assert_eq!(result[0], 0);
        assert!(result[1..].iter().all(|&b| b == 1));
    }
}
