// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Transform Laboratory: YAML-driven benchmarking harness for CPAC transforms.
//!
//! Provides a systematic framework for testing individual transforms and chains
//! against a corpus and measuring per-file compressed-size deltas.
//!
//! # Usage
//!
//! 1. Write a YAML experiment file defining trials (transform chains)
//! 2. Run `transform_study --experiment <file.yaml> --corpus <dir>`
//! 3. Inspect per-file and grouped results
//!
//! # Example YAML
//!
//! ```yaml
//! name: delta_study
//! zstd_level: 3
//! extensions: [log, txt]
//! group_by: [extension, domain]
//! trials:
//!   - name: baseline
//!   - name: delta_only
//!     transforms:
//!       - name: delta
//!   - name: msn_plus_delta
//!     msn_extract: true
//!     transforms:
//!       - name: delta
//! ```

#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss
)]

pub mod calibrate;
pub mod collector;
pub mod experiment;
pub mod report;
pub mod runner;

pub use collector::{collect_files, CollectOptions, CorpusFile};
pub use experiment::{
    load_experiment, parse_experiment, validate_experiment, Experiment, GroupBy, TransformSpec,
    TrialDef,
};
pub use runner::{aggregate_results, run_experiment, run_trial, GroupResult, TrialResult};
