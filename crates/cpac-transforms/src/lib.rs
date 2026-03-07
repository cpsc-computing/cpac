// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Encoding transforms for the CPAC compression pipeline.
//!
//! Provides four core transforms (delta, zigzag, transpose, ROLZ) and a
//! preprocess orchestrator that selects and applies the optimal transform
//! chain based on SSR metrics.
//!
//! The preprocessed output uses a self-describing `TP` frame so the
//! decompressor can reverse transforms automatically.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::cast_possible_wrap,
    clippy::cast_ptr_alignment,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

pub mod arith_decomp;
pub mod bwt;
pub mod bwt_chain;
pub mod byte_plane;
pub mod context_split;
pub mod dedup;
pub mod delta;
pub mod field_lz;
pub mod float_split;
pub mod float_xor;
pub mod mtf;
pub mod normalize;
pub mod parse_int;
pub mod prefix;
pub mod range_pack;
pub mod rle;
pub mod rolz;
pub mod row_sort;
pub mod simd;
pub mod tokenize;
pub mod traits;
pub mod transpose;
pub mod vocab;
pub mod zigzag;

pub use traits::{TransformContext, TransformNode};

// Re-export transform structs for DAG use
pub use arith_decomp::ArithDecompTransform;
pub use bwt_chain::BwtChainTransform;
pub use byte_plane::BytePlaneTransform;
pub use context_split::ContextSplitTransform;
pub use dedup::DedupTransform;
pub use delta::DeltaTransform;
pub use field_lz::FieldLzTransform;
pub use float_split::FloatSplitTransform;
pub use float_xor::FloatXorTransform;
pub use normalize::NormalizeTransform;
pub use parse_int::ParseIntTransform;
pub use prefix::PrefixTransform;
pub use range_pack::RangePackTransform;
pub use rle::RleTransform;
pub use rolz::RolzTransform;
pub use row_sort::RowSortTransform;
pub use tokenize::TokenizeTransform;
pub use transpose::TransposeTransform;
pub use vocab::VocabTransform;
pub use zigzag::ZigzagTransform;

// ---------------------------------------------------------------------------
// TP frame format (matches Python preprocess.py)
// ---------------------------------------------------------------------------

/// Magic bytes for a preprocessed frame.
const TP_MAGIC: &[u8; 2] = b"TP";

/// TP frame version.
const TP_VERSION: u8 = 1;

/// Minimum data size to attempt preprocessing (avoid overhead on tiny data).
const MIN_PREPROCESS_SIZE: usize = 128;

/// Transform IDs (matching Python `TransformID` enum).
#[repr(u8)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformID {
    Transpose = 1,
    FloatSplit = 2,
    FieldLz = 3,
    Rolz = 4,
}

impl TransformID {
    fn from_byte(b: u8) -> Option<Self> {
        match b {
            1 => Some(TransformID::Transpose),
            2 => Some(TransformID::FloatSplit),
            3 => Some(TransformID::FieldLz),
            4 => Some(TransformID::Rolz),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Preprocess orchestrator
// ---------------------------------------------------------------------------

/// Analyze data and apply the optimal transform chain.
///
/// Uses SSR metrics from `ctx` to guide transform selection.
/// Returns `(data, metadata)` where `data` is the TP-framed output
/// (or raw bytes if no transform was applied) and `metadata` is empty
/// (all info is embedded in the TP frame).
#[must_use]
pub fn preprocess(data: &[u8], ctx: &TransformContext) -> (Vec<u8>, Vec<u8>) {
    let n = data.len();
    if n < MIN_PREPROCESS_SIZE {
        return (data.to_vec(), Vec::new());
    }

    let is_text = ctx.ascii_ratio > 0.85;
    let is_binary = !is_text;
    let ent = ctx.entropy_estimate;

    let mut best_data: Option<Vec<u8>> = None;
    let mut best_transform: Option<TransformID> = None;
    let mut best_params: Vec<u8> = Vec::new();

    // Strategy 1: Binary data with floating-point patterns → float_split
    if is_binary && ent < 6.5 && n >= 128 {
        if let Some(fw) = float_split::detect_float_data(data) {
            if let Ok(split) = float_split::float_split_encode_framed(data, fw) {
                if split.len() < n {
                    best_data = Some(split);
                    best_transform = Some(TransformID::FloatSplit);
                    best_params = Vec::new();
                }
            }
        }
    }

    // Strategy 2: Binary data with repeating field patterns → field_lz
    if best_transform.is_none() && is_binary && ent < 7.0 && n >= 256 {
        // Try common field widths
        for &fw in &[4usize, 8, 2] {
            if n.is_multiple_of(fw) && n >= fw * 8 {
                let repeat_ratio = field_lz::detect_repeating_fields(data, fw);
                if repeat_ratio > 0.3 {
                    if let Ok(compressed) = field_lz::field_lz_encode(data, fw) {
                        if compressed.len() < n {
                            best_data = Some(compressed);
                            best_transform = Some(TransformID::FieldLz);
                            best_params = Vec::new();
                            break;
                        }
                    }
                }
            }
        }
    }

    // Strategy 3: Binary data with fixed-width records → transpose
    if best_transform.is_none() && is_binary && ent < 7.0 {
        if let Some(width) = transpose::detect_record_width(data) {
            if n.is_multiple_of(width) {
                if let Ok(transposed) = transpose::transpose_encode(data, width) {
                    if transposed.len() <= n {
                        best_data = Some(transposed);
                        best_transform = Some(TransformID::Transpose);
                        best_params = (width as u16).to_le_bytes().to_vec();
                    }
                }
            }
        }
    }

    // Strategy 4: Medium-entropy data → ROLZ
    // Text: entropy >= 3.5 (lowered from 4.5 to capture log residuals near the floor).
    // Binary: entropy >= 3.0.
    if best_transform.is_none() {
        let min_ent = if is_text { 3.5 } else { 3.0 };
        if min_ent < ent && ent < 7.0 && n >= 512 {
            let compressed = rolz::rolz_encode(data);
            // Text: require 20% savings (threshold 0.80, narrowed from original 0.75 / 25%
            // to capture log residuals with moderate ROLZ gain without accepting marginal
            // results that hurt zstd's downstream ratio).
            // Binary: require 8% savings.
            let threshold = if is_text { 0.80 } else { 0.92 };
            if (compressed.len() as f64) < n as f64 * threshold {
                best_data = Some(compressed);
                best_transform = Some(TransformID::Rolz);
                best_params = Vec::new();
            }
        }
    }

    // No beneficial transform found — return raw data
    let (transform_id, transformed, params) = match (best_transform, best_data) {
        (Some(tid), Some(data)) => (tid, data, best_params),
        _ => return (data.to_vec(), Vec::new()),
    };

    // Build TP frame: ["TP"][version][count][ids...][per-transform: param_len(2)+params][payload]
    let mut frame = Vec::with_capacity(8 + params.len() + transformed.len());
    frame.extend_from_slice(TP_MAGIC);
    frame.push(TP_VERSION);
    frame.push(1); // transform_count = 1
    frame.push(transform_id as u8);
    frame.extend_from_slice(&(params.len() as u16).to_le_bytes());
    frame.extend_from_slice(&params);
    frame.extend_from_slice(&transformed);

    (frame, Vec::new())
}

/// Reverse preprocessing transforms.
///
/// Checks for the `TP` magic header in `data`. If found, reverses the
/// embedded transforms. The `_metadata` parameter is unused (all info
/// is embedded in the TP frame).
#[must_use]
pub fn unpreprocess(data: &[u8], _metadata: &[u8]) -> Vec<u8> {
    if data.len() < 4 || &data[0..2] != TP_MAGIC {
        return data.to_vec();
    }

    let version = data[2];
    if version != TP_VERSION {
        return data.to_vec();
    }

    let transform_count = data[3] as usize;
    let mut offset = 4;

    // Read transform IDs
    if offset + transform_count > data.len() {
        return data.to_vec();
    }
    let mut transform_ids = Vec::with_capacity(transform_count);
    for _ in 0..transform_count {
        transform_ids.push(data[offset]);
        offset += 1;
    }

    // Read per-transform params
    let mut transform_params = Vec::with_capacity(transform_count);
    for _ in 0..transform_count {
        if offset + 2 > data.len() {
            return data.to_vec();
        }
        let param_len = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
        offset += 2;
        if offset + param_len > data.len() {
            return data.to_vec();
        }
        transform_params.push(data[offset..offset + param_len].to_vec());
        offset += param_len;
    }

    // Extract payload
    let mut result = data[offset..].to_vec();

    // Reverse transforms in reverse order
    for i in (0..transform_count).rev() {
        let tid = transform_ids[i];
        let params = &transform_params[i];

        match TransformID::from_byte(tid) {
            Some(TransformID::Transpose) => {
                if params.len() >= 2 {
                    let width = u16::from_le_bytes([params[0], params[1]]) as usize;
                    if let Ok(decoded) = transpose::transpose_decode(&result, width) {
                        result = decoded;
                    }
                }
            }
            Some(TransformID::FloatSplit) => {
                if let Ok(decoded) = float_split::float_split_decode_framed(&result) {
                    result = decoded;
                }
            }
            Some(TransformID::FieldLz) => {
                if let Ok(decoded) = field_lz::field_lz_decode(&result) {
                    result = decoded;
                }
            }
            Some(TransformID::Rolz) => {
                if let Ok(decoded) = rolz::rolz_decode(&result) {
                    result = decoded;
                }
            }
            None => {
                // Unknown transform — bail, return raw data
                return data.to_vec();
            }
        }
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preprocess_small_data_passthrough() {
        let data = b"small";
        let ctx = TransformContext {
            entropy_estimate: 3.0,
            ascii_ratio: 0.5,
            data_size: data.len(),
        };
        let (out, meta) = preprocess(data, &ctx);
        assert_eq!(out, data);
        assert!(meta.is_empty());
    }

    #[test]
    fn preprocess_unpreprocess_transpose() {
        // Build structured data: 200 records of 4 bytes with columnar patterns
        let mut data = Vec::with_capacity(800);
        for i in 0u16..200 {
            data.push(0x01); // constant field
            data.push((i & 0xFF) as u8); // counter
            data.push(0xFF); // constant field
            data.push(0x00); // constant field
        }
        let ctx = TransformContext {
            entropy_estimate: 3.5,
            ascii_ratio: 0.1,
            data_size: data.len(),
        };
        let (preprocessed, meta) = preprocess(&data, &ctx);
        assert!(meta.is_empty());
        // Should have TP magic
        assert_eq!(&preprocessed[0..2], b"TP");
        // Should roundtrip
        let restored = unpreprocess(&preprocessed, &meta);
        assert_eq!(restored, data);
    }

    #[test]
    fn preprocess_unpreprocess_rolz() {
        // Repetitive medium-entropy data
        let data = b"The quick brown fox jumps over the lazy dog. ".repeat(30);
        let ctx = TransformContext {
            entropy_estimate: 4.8,
            ascii_ratio: 0.95,
            data_size: data.len(),
        };
        let (preprocessed, meta) = preprocess(&data, &ctx);
        assert!(meta.is_empty());
        // Should have TP magic (ROLZ applied)
        assert_eq!(&preprocessed[0..2], b"TP");
        // Should roundtrip
        let restored = unpreprocess(&preprocessed, &meta);
        assert_eq!(restored, data);
    }

    #[test]
    fn unpreprocess_non_tp_passthrough() {
        let data = b"not a TP frame";
        let result = unpreprocess(data, &[]);
        assert_eq!(result, data);
    }

    #[test]
    fn preprocess_high_entropy_passthrough() {
        // High-entropy data should not be preprocessed
        let data: Vec<u8> = (0u8..=255).cycle().take(512).collect();
        let ctx = TransformContext {
            entropy_estimate: 7.9,
            ascii_ratio: 0.3,
            data_size: data.len(),
        };
        let (out, _) = preprocess(&data, &ctx);
        // Should be passthrough (no TP magic)
        assert_ne!(&out[0..2], b"TP");
        assert_eq!(out, data);
    }
}
