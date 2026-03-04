// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! `TransformNode` trait — the interface every compression transform implements.

use cpac_types::{CpacResult, CpacType, TypeTag};

/// Context passed to transforms for SSR-guided decisions.
#[derive(Clone, Debug)]
pub struct TransformContext {
    /// Shannon entropy estimate (bits/byte).
    pub entropy_estimate: f64,
    /// Fraction of ASCII bytes.
    pub ascii_ratio: f64,
    /// Data size in bytes.
    pub data_size: usize,
}

/// A single transform node in the compression DAG.
///
/// Every transform is a pure function: `encode(input) -> (output, metadata)`
/// and `decode(input, metadata) -> output`. The metadata is stored in the
/// frame so the decoder can invert the transform without knowing the DAG.
pub trait TransformNode: Send + Sync {
    /// Human-readable name for debugging and frame metadata.
    fn name(&self) -> &str;

    /// Unique ID for frame serialization (1 byte).
    fn id(&self) -> u8;

    /// What input types this transform accepts.
    fn accepts(&self) -> &[TypeTag];

    /// What output type this transform produces.
    fn produces(&self) -> TypeTag;

    /// Estimate MDL gain on the given input (positive = beneficial).
    /// Returns `None` if transform is not applicable.
    fn estimate_gain(&self, input: &CpacType, ctx: &TransformContext) -> Option<f64>;

    /// Apply the forward transform (compression direction).
    /// Returns `(transformed_data, frame_metadata_for_decoder)`.
    fn encode(&self, input: CpacType, ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)>;

    /// Apply the inverse transform (decompression direction).
    fn decode(&self, input: CpacType, metadata: &[u8]) -> CpacResult<CpacType>;
}
