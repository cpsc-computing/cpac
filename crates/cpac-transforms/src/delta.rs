// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Delta encoding transform.
//!
//! Byte-level: `output[i] = input[i] - input[i-1]` (wrapping).
//! Converts slowly-changing sequences (timestamps, sensor data) into
//! small-magnitude deltas that compress much better under entropy coding.

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for delta encoding (wire format).
pub const TRANSFORM_ID: u8 = 5;

// ---------------------------------------------------------------------------
// Byte-level delta (matches Python delta_encode_8 / delta_decode_8)
// ---------------------------------------------------------------------------

/// Apply byte-level delta encoding.
///
/// First byte is stored as-is; subsequent bytes store `(cur - prev) & 0xFF`.
#[must_use]
pub fn delta_encode(data: &[u8]) -> Vec<u8> {
    if data.len() < 2 {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(data.len());
    out.push(data[0]);
    for i in 1..data.len() {
        out.push(data[i].wrapping_sub(data[i - 1]));
    }
    out
}

/// Reverse byte-level delta encoding.
#[must_use]
pub fn delta_decode(data: &[u8]) -> Vec<u8> {
    if data.len() < 2 {
        return data.to_vec();
    }
    let mut out = Vec::with_capacity(data.len());
    out.push(data[0]);
    for i in 1..data.len() {
        out.push(out[i - 1].wrapping_add(data[i]));
    }
    out
}

// ---------------------------------------------------------------------------
// i64 sequence delta (for IntColumn paths in Phase 3+)
// ---------------------------------------------------------------------------

/// Delta-encode a slice of i64 values.
#[must_use]
pub fn delta_encode_i64(values: &[i64]) -> Vec<i64> {
    if values.is_empty() {
        return Vec::new();
    }
    let mut deltas = Vec::with_capacity(values.len());
    deltas.push(values[0]);
    for i in 1..values.len() {
        deltas.push(values[i].wrapping_sub(values[i - 1]));
    }
    deltas
}

/// Reverse i64 delta encoding.
#[must_use]
pub fn delta_decode_i64(deltas: &[i64]) -> Vec<i64> {
    if deltas.is_empty() {
        return Vec::new();
    }
    let mut values = Vec::with_capacity(deltas.len());
    values.push(deltas[0]);
    for i in 1..deltas.len() {
        values.push(values[i - 1].wrapping_add(deltas[i]));
    }
    values
}

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Delta transform node for the compression DAG.
pub struct DeltaTransform;

impl TransformNode for DeltaTransform {
    fn name(&self) -> &'static str {
        "delta"
    }

    fn id(&self) -> u8 {
        TRANSFORM_ID
    }

    fn accepts(&self) -> &[TypeTag] {
        &[TypeTag::Serial, TypeTag::IntColumn]
    }

    fn produces(&self) -> TypeTag {
        TypeTag::Serial
    }

    fn estimate_gain(&self, input: &CpacType, ctx: &TransformContext) -> Option<f64> {
        // Delta benefits slowly-changing data (low byte-to-byte diffs).
        // Heuristic: beneficial when entropy < 6.0 and data is binary-ish.
        if ctx.entropy_estimate > 6.0 || ctx.data_size < 32 {
            return None;
        }
        match input {
            CpacType::Serial(data) => {
                let avg_diff = average_byte_diff(data);
                if avg_diff < 30.0 {
                    Some(30.0 - avg_diff) // higher gain for lower avg diff
                } else {
                    None
                }
            }
            CpacType::IntColumn { .. } => Some(1.0), // almost always beneficial
            _ => None,
        }
    }

    fn encode(&self, input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        match input {
            CpacType::Serial(data) => {
                // Use SIMD-accelerated path when available
                let encoded = crate::simd::delta_encode_fast(&data);
                Ok((CpacType::Serial(encoded), Vec::new()))
            }
            CpacType::IntColumn {
                values,
                original_width,
            } => {
                let deltas = delta_encode_i64(&values);
                Ok((
                    CpacType::IntColumn {
                        values: deltas,
                        original_width,
                    },
                    Vec::new(),
                ))
            }
            _ => Err(CpacError::Transform("delta: unsupported input type".into())),
        }
    }

    fn decode(&self, input: CpacType, _metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => Ok(CpacType::Serial(delta_decode(&data))),
            CpacType::IntColumn {
                values,
                original_width,
            } => {
                let restored = delta_decode_i64(&values);
                Ok(CpacType::IntColumn {
                    values: restored,
                    original_width,
                })
            }
            _ => Err(CpacError::Transform("delta: unsupported input type".into())),
        }
    }
}

/// Average absolute byte-to-byte difference (used for gain estimation).
fn average_byte_diff(data: &[u8]) -> f64 {
    if data.len() < 2 {
        return 128.0;
    }
    let sum: u64 = data
        .windows(2)
        .map(|w| u64::from((i16::from(w[1]) - i16::from(w[0])).unsigned_abs()))
        .sum();
    sum as f64 / (data.len() - 1) as f64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_bytes_simple() {
        let data = vec![10, 12, 14, 16, 18, 20];
        let encoded = delta_encode(&data);
        let decoded = delta_decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn roundtrip_bytes_wrapping() {
        // Test wrapping around 0/255 boundary
        let data = vec![250, 5, 10, 0, 255];
        let encoded = delta_encode(&data);
        let decoded = delta_decode(&encoded);
        assert_eq!(decoded, data);
    }

    #[test]
    fn roundtrip_empty() {
        assert_eq!(delta_decode(&delta_encode(&[])), Vec::<u8>::new());
    }

    #[test]
    fn roundtrip_single() {
        assert_eq!(delta_decode(&delta_encode(&[42])), vec![42]);
    }

    #[test]
    fn roundtrip_i64() {
        let values = vec![100, 105, 110, 108, 120, 200];
        let deltas = delta_encode_i64(&values);
        let restored = delta_decode_i64(&deltas);
        assert_eq!(restored, values);
    }

    #[test]
    fn monotonic_reduces_magnitude() {
        let data: Vec<u8> = (0u8..=100).collect();
        let encoded = delta_encode(&data);
        // After delta, all values should be 0 or 1 (small)
        assert_eq!(encoded[0], 0);
        for &b in &encoded[1..] {
            assert_eq!(b, 1);
        }
    }

    #[test]
    fn transform_node_serial() {
        let t = DeltaTransform;
        let ctx = TransformContext {
            entropy_estimate: 3.0,
            ascii_ratio: 0.0,
            data_size: 100,
        };
        let input = CpacType::Serial(vec![10, 12, 14, 16, 18]);
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        assert!(meta.is_empty());
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::Serial(d) => assert_eq!(d, vec![10, 12, 14, 16, 18]),
            _ => panic!("expected Serial"),
        }
    }
}
