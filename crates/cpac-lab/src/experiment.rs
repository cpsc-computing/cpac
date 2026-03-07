// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! YAML-driven experiment definitions.
//!
//! An experiment file describes one or more trials to run against a corpus.
//! Each trial specifies a transform chain and optional filters.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// ---------------------------------------------------------------------------
// YAML schema
// ---------------------------------------------------------------------------

/// Top-level experiment definition loaded from YAML.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Experiment {
    /// Human-readable experiment name.
    pub name: String,

    /// Optional description.
    #[serde(default)]
    pub description: String,

    /// Corpus root directory (can be overridden by CLI).
    #[serde(default)]
    pub corpus: Option<PathBuf>,

    /// Default zstd compression level for all trials.
    #[serde(default = "default_zstd_level")]
    pub zstd_level: i32,

    /// Maximum file size in bytes (0 = no limit).
    #[serde(default)]
    pub max_file_size: u64,

    /// File extension filters (empty = all text extensions).
    #[serde(default)]
    pub extensions: Vec<String>,

    /// Exclude these extensions.
    #[serde(default)]
    pub exclude_extensions: Vec<String>,

    /// The list of trials to run.
    pub trials: Vec<TrialDef>,

    /// Grouping for summary output.
    #[serde(default)]
    pub group_by: Vec<GroupBy>,
}

/// A single trial: a named transform chain to benchmark.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrialDef {
    /// Trial name (used in output columns).
    pub name: String,

    /// Transform chain to apply (in order). Empty = baseline (zstd only).
    #[serde(default)]
    pub transforms: Vec<TransformSpec>,

    /// Optional: only apply to files matching these MSN domains.
    #[serde(default)]
    pub domain_filter: Vec<String>,

    /// Optional: only apply to files matching these extensions.
    #[serde(default)]
    pub ext_filter: Vec<String>,

    /// Override zstd level for this trial.
    #[serde(default)]
    pub zstd_level: Option<i32>,

    /// Whether to run MSN extraction before the transform chain.
    #[serde(default)]
    pub msn_extract: bool,

    /// Minimum MSN confidence (only relevant when `msn_extract` is true).
    #[serde(default = "default_msn_confidence")]
    pub msn_confidence: f64,
}

/// Specification for a single transform in a chain.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransformSpec {
    /// Transform name (must match `TransformNode::name()`).
    pub name: String,

    /// Optional parameters (transform-specific, serialized as JSON).
    #[serde(default)]
    pub params: serde_json::Value,
}

/// Grouping dimension for summary output.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum GroupBy {
    Extension,
    Domain,
    Directory,
}

fn default_zstd_level() -> i32 {
    3
}

fn default_msn_confidence() -> f64 {
    0.3
}

// ---------------------------------------------------------------------------
// Loading
// ---------------------------------------------------------------------------

/// Load an experiment from a YAML file.
///
/// # Errors
///
/// Returns an error if the file cannot be read or parsed.
pub fn load_experiment(path: &Path) -> cpac_types::CpacResult<Experiment> {
    let content = std::fs::read_to_string(path)
        .map_err(|e| cpac_types::CpacError::IoError(format!("read experiment: {e}")))?;
    parse_experiment(&content)
}

/// Parse an experiment from a YAML string.
///
/// # Errors
///
/// Returns an error if the YAML is invalid.
pub fn parse_experiment(yaml: &str) -> cpac_types::CpacResult<Experiment> {
    serde_yaml::from_str(yaml)
        .map_err(|e| cpac_types::CpacError::Other(format!("parse experiment YAML: {e}")))
}

// ---------------------------------------------------------------------------
// Validation
// ---------------------------------------------------------------------------

/// Validate an experiment definition.
///
/// # Errors
///
/// Returns an error if the experiment is invalid (e.g. empty trials).
pub fn validate_experiment(exp: &Experiment) -> cpac_types::CpacResult<()> {
    if exp.trials.is_empty() {
        return Err(cpac_types::CpacError::Other(
            "experiment has no trials".into(),
        ));
    }
    for trial in &exp.trials {
        if trial.name.is_empty() {
            return Err(cpac_types::CpacError::Other(
                "trial name cannot be empty".into(),
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_minimal_experiment() {
        let yaml = r#"
name: baseline
trials:
  - name: zstd_only
"#;
        let exp = parse_experiment(yaml).unwrap();
        assert_eq!(exp.name, "baseline");
        assert_eq!(exp.trials.len(), 1);
        assert_eq!(exp.trials[0].name, "zstd_only");
        assert!(exp.trials[0].transforms.is_empty());
        assert_eq!(exp.zstd_level, 3);
    }

    #[test]
    fn parse_full_experiment() {
        let yaml = r#"
name: delta_study
description: Test delta transform on log files
corpus: .work/benchdata
zstd_level: 3
max_file_size: 104857600
extensions: [log, txt]
group_by: [extension, domain]
trials:
  - name: baseline
  - name: delta_only
    transforms:
      - name: delta
  - name: delta_zigzag
    transforms:
      - name: delta
      - name: zigzag
    ext_filter: [log]
  - name: msn_plus_delta
    msn_extract: true
    msn_confidence: 0.5
    transforms:
      - name: delta
"#;
        let exp = parse_experiment(yaml).unwrap();
        assert_eq!(exp.name, "delta_study");
        assert_eq!(exp.trials.len(), 4);
        assert_eq!(exp.extensions, vec!["log", "txt"]);
        assert_eq!(exp.group_by, vec![GroupBy::Extension, GroupBy::Domain]);

        let t2 = &exp.trials[2];
        assert_eq!(t2.transforms.len(), 2);
        assert_eq!(t2.transforms[0].name, "delta");
        assert_eq!(t2.transforms[1].name, "zigzag");
        assert_eq!(t2.ext_filter, vec!["log"]);

        let t3 = &exp.trials[3];
        assert!(t3.msn_extract);
        assert!((t3.msn_confidence - 0.5).abs() < f64::EPSILON);
    }

    #[test]
    fn validate_empty_trials() {
        let exp = Experiment {
            name: "bad".into(),
            description: String::new(),
            corpus: None,
            zstd_level: 3,
            max_file_size: 0,
            extensions: vec![],
            exclude_extensions: vec![],
            trials: vec![],
            group_by: vec![],
        };
        assert!(validate_experiment(&exp).is_err());
    }
}
