// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Predictive modeling for compression: encode residuals instead of raw values.
//!
//! Provides three deterministic predictors:
//! - **Order-1 delta**: `residual[i] = data[i] - data[i-1]`
//! - **Order-2 delta**: `residual[i] = data[i] - 2*data[i-1] + data[i-2]`
//! - **Context(2)**: hash previous 2 bytes → predict most frequent follower
//!
//! The `select_best` function trials each on a sample and picks the one
//! producing lowest residual Shannon entropy.

#![allow(
    clippy::cast_precision_loss,
    clippy::cast_possible_truncation,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

/// Predictor ID for wire format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum PredictorId {
    Delta1 = 0,
    Delta2 = 1,
    Context2 = 2,
}

impl PredictorId {
    /// Convert from byte.
    #[must_use]
    pub fn from_u8(b: u8) -> Option<Self> {
        match b {
            0 => Some(PredictorId::Delta1),
            1 => Some(PredictorId::Delta2),
            2 => Some(PredictorId::Context2),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Encode / Decode functions
// ---------------------------------------------------------------------------

/// Encode data using order-1 delta prediction.
/// Returns residuals: `residual[i] = data[i].wrapping_sub(data[i-1])`.
#[must_use]
pub fn encode_delta1(data: &[u8]) -> Vec<u8> {
    if data.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(data.len());
    out.push(data[0]); // first byte is literal
    for i in 1..data.len() {
        out.push(data[i].wrapping_sub(data[i - 1]));
    }
    out
}

/// Decode order-1 delta residuals back to original data.
#[must_use]
pub fn decode_delta1(residuals: &[u8]) -> Vec<u8> {
    if residuals.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(residuals.len());
    out.push(residuals[0]);
    for i in 1..residuals.len() {
        out.push(residuals[i].wrapping_add(out[i - 1]));
    }
    out
}

/// Encode data using order-2 delta prediction.
/// `residual[i] = data[i] - 2*data[i-1] + data[i-2]` (wrapping).
#[must_use]
pub fn encode_delta2(data: &[u8]) -> Vec<u8> {
    if data.len() < 3 {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(data.len());
    out.push(data[0]); // literal
    out.push(data[1]); // literal
    for i in 2..data.len() {
        let predicted = data[i - 1]
            .wrapping_mul(2)
            .wrapping_sub(data[i - 2]);
        out.push(data[i].wrapping_sub(predicted));
    }
    out
}

/// Decode order-2 delta residuals.
#[must_use]
pub fn decode_delta2(residuals: &[u8]) -> Vec<u8> {
    if residuals.len() < 3 {
        return residuals.to_vec();
    }
    let mut out = Vec::with_capacity(residuals.len());
    out.push(residuals[0]);
    out.push(residuals[1]);
    for i in 2..residuals.len() {
        let predicted = out[i - 1]
            .wrapping_mul(2)
            .wrapping_sub(out[i - 2]);
        out.push(residuals[i].wrapping_add(predicted));
    }
    out
}

/// Build a context-2 prediction table from data.
///
/// For each 2-byte context, stores the most frequent following byte.
/// Table is 65536 entries (256*256), one byte each.
fn build_context2_table(data: &[u8]) -> Vec<u8> {
    if data.len() < 3 {
        return vec![0u8; 65536];
    }
    // Count frequencies: context → byte → count
    let mut counts = vec![0u32; 65536 * 256];
    for window in data.windows(3) {
        let ctx = (window[0] as usize) << 8 | (window[1] as usize);
        let next = window[2] as usize;
        counts[ctx * 256 + next] += 1;
    }
    // Find most frequent for each context
    let mut table = vec![0u8; 65536];
    for (ctx, entry) in table.iter_mut().enumerate() {
        let base = ctx * 256;
        let mut best_byte = 0u8;
        let mut best_count = 0u32;
        for b in 0..256 {
            if counts[base + b] > best_count {
                best_count = counts[base + b];
                best_byte = b as u8;
            }
        }
        *entry = best_byte;
    }
    table
}

/// Encode data using order-2 context prediction.
/// Residuals: `residual[i] = data[i] - predicted[i]` where predicted
/// is the most frequent byte following the previous 2-byte context.
#[must_use]
pub fn encode_context2(data: &[u8]) -> (Vec<u8>, Vec<u8>) {
    if data.len() < 3 {
        return (data.to_vec(), Vec::new());
    }
    let table = build_context2_table(data);
    let mut residuals = Vec::with_capacity(data.len());
    residuals.push(data[0]); // literal
    residuals.push(data[1]); // literal
    for i in 2..data.len() {
        let ctx = (data[i - 2] as usize) << 8 | (data[i - 1] as usize);
        let predicted = table[ctx];
        residuals.push(data[i].wrapping_sub(predicted));
    }
    (residuals, table)
}

/// Decode context-2 residuals using the provided table.
#[must_use]
pub fn decode_context2(residuals: &[u8], table: &[u8]) -> Vec<u8> {
    if residuals.len() < 3 || table.len() < 65536 {
        return residuals.to_vec();
    }
    let mut out = Vec::with_capacity(residuals.len());
    out.push(residuals[0]);
    out.push(residuals[1]);
    for i in 2..residuals.len() {
        let ctx = (out[i - 2] as usize) << 8 | (out[i - 1] as usize);
        let predicted = table[ctx];
        out.push(residuals[i].wrapping_add(predicted));
    }
    out
}

/// Shannon entropy of a byte stream (bits per byte).
fn shannon_entropy(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut counts = [0u64; 256];
    for &b in data {
        counts[b as usize] += 1;
    }
    let n = data.len() as f64;
    let mut entropy = 0.0;
    for &c in &counts {
        if c > 0 {
            let p = c as f64 / n;
            entropy -= p * p.log2();
        }
    }
    entropy
}

/// Select the best predictor for the given data by trialing each on
/// a sample (up to 1 KB) and picking the one with lowest residual entropy.
///
/// Returns `(predictor_id, residual_entropy)`.
#[must_use]
pub fn select_best(data: &[u8]) -> (PredictorId, f64) {
    let sample = &data[..data.len().min(1024)];
    if sample.len() < 3 {
        return (PredictorId::Delta1, 8.0);
    }

    let d1 = encode_delta1(sample);
    let e1 = shannon_entropy(&d1);

    let d2 = encode_delta2(sample);
    let e2 = shannon_entropy(&d2);

    let (c2, _table) = encode_context2(sample);
    let ec = shannon_entropy(&c2);

    if e1 <= e2 && e1 <= ec {
        (PredictorId::Delta1, e1)
    } else if e2 <= ec {
        (PredictorId::Delta2, e2)
    } else {
        (PredictorId::Context2, ec)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_delta1() {
        let data: Vec<u8> = (0..200).map(|i| (i * 3 % 256) as u8).collect();
        let encoded = encode_delta1(&data);
        let decoded = decode_delta1(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn roundtrip_delta2() {
        let data: Vec<u8> = (0..200).map(|i| (i * 3 % 256) as u8).collect();
        let encoded = encode_delta2(&data);
        let decoded = decode_delta2(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn roundtrip_context2() {
        let data = b"the quick brown fox jumps over the lazy dog again and again the quick brown fox".to_vec();
        let (encoded, table) = encode_context2(&data);
        let decoded = decode_context2(&encoded, &table);
        assert_eq!(decoded, data);
    }

    #[test]
    fn select_best_on_counter() {
        // Counter data should prefer delta1
        let data: Vec<u8> = (0..200).map(|i| (i % 256) as u8).collect();
        let (id, _entropy) = select_best(&data);
        assert_eq!(id, PredictorId::Delta1);
    }

    #[test]
    fn empty_data() {
        assert!(encode_delta1(&[]).is_empty());
        assert!(decode_delta1(&[]).is_empty());
        assert!(encode_delta2(&[]).is_empty());
        assert!(decode_delta2(&[]).is_empty());
    }

    #[test]
    fn small_data_passthrough() {
        let data = vec![1u8, 2];
        assert_eq!(encode_delta2(&data), data);
        assert_eq!(decode_delta2(&data), data);
    }
}
