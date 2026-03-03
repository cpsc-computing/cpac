// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! DAG-based transform composition and profile system.
//!
//! Provides `TransformRegistry`, `TransformDAG`, and `ProfileCache`
//! for composing and executing transform pipelines.

pub mod dag;
pub mod profile;
pub mod registry;

pub use dag::{deserialize_dag_descriptor, serialize_dag_descriptor, TransformDAG};
pub use profile::{Profile, ProfileCache};
pub use registry::TransformRegistry;
