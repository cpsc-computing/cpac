// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Zigzag encoding transform.
//!
//! Maps signed integers to unsigned: `0 → 0, -1 → 1, 1 → 2, -2 → 3, …`
//! This makes small-magnitude negative numbers into small unsigned numbers,
//! which varint and entropy coders handle much better.

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for zigzag encoding (wire format).
pub const TRANSFORM_ID: u8 = 6;

// ---------------------------------------------------------------------------
// Core zigzag encode/decode (matches Python encode_zigzag / decode_zigzag)
// ---------------------------------------------------------------------------

/// Zigzag-encode a signed i64 to an unsigned u64.
#[inline]
#[must_use]
pub fn zigzag_encode(value: i64) -> u64 {
    ((value << 1) ^ (value >> 63)) as u64
}

/// Zigzag-decode an unsigned u64 back to signed i64.
#[inline]
#[must_use]
pub fn zigzag_decode(value: u64) -> i64 {
    ((value >> 1) as i64) ^ (-((value & 1) as i64))
}

// ---------------------------------------------------------------------------
// Varint encoding (unsigned LEB128)
// ---------------------------------------------------------------------------

/// Encode an unsigned integer as a varint (LEB128).
#[must_use]
pub fn encode_varint(mut value: u64) -> Vec<u8> {
    let mut buf = Vec::with_capacity(10);
    loop {
        let mut byte = (value & 0x7F) as u8;
        value >>= 7;
        if value != 0 {
            byte |= 0x80;
        }
        buf.push(byte);
        if value == 0 {
            break;
        }
    }
    buf
}

/// Decode a varint from a byte slice. Returns `(value, bytes_consumed)`.
pub fn decode_varint(data: &[u8]) -> CpacResult<(u64, usize)> {
    let mut result: u64 = 0;
    let mut shift = 0u32;
    for (i, &byte) in data.iter().enumerate() {
        result |= u64::from(byte & 0x7F) << shift;
        if byte & 0x80 == 0 {
            return Ok((result, i + 1));
        }
        shift += 7;
        if shift >= 64 {
            return Err(cpac_types::CpacError::Transform("varint overflow".into()));
        }
    }
    Err(cpac_types::CpacError::Transform("truncated varint".into()))
}

/// Encode a signed i64 as zigzag + varint.
#[must_use]
pub fn encode_signed_varint(value: i64) -> Vec<u8> {
    encode_varint(zigzag_encode(value))
}

/// Decode a signed varint. Returns `(value, bytes_consumed)`.
pub fn decode_signed_varint(data: &[u8]) -> CpacResult<(i64, usize)> {
    let (unsigned, consumed) = decode_varint(data)?;
    Ok((zigzag_decode(unsigned), consumed))
}

// ---------------------------------------------------------------------------
// Batch encode/decode for i64 slices (zigzag + varint framing)
// ---------------------------------------------------------------------------

/// Encode a slice of i64 values using zigzag + varint.
///
/// Format: `[count: varint][value0: signed_varint][value1: signed_varint]...`
#[must_use]
pub fn zigzag_encode_batch(values: &[i64]) -> Vec<u8> {
    let mut buf = encode_varint(values.len() as u64);
    for &v in values {
        buf.extend_from_slice(&encode_signed_varint(v));
    }
    buf
}

/// Decode a batch of zigzag+varint encoded i64 values.
pub fn zigzag_decode_batch(data: &[u8]) -> CpacResult<(Vec<i64>, usize)> {
    let (count, mut offset) = decode_varint(data)?;
    let count = count as usize;
    let mut values = Vec::with_capacity(count);
    for _ in 0..count {
        let (val, consumed) = decode_signed_varint(&data[offset..])?;
        values.push(val);
        offset += consumed;
    }
    Ok((values, offset))
}

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Zigzag transform node for the compression DAG.
pub struct ZigzagTransform;

impl TransformNode for ZigzagTransform {
    fn name(&self) -> &'static str {
        "zigzag"
    }

    fn id(&self) -> u8 {
        TRANSFORM_ID
    }

    fn accepts(&self) -> &[TypeTag] {
        &[TypeTag::IntColumn]
    }

    fn produces(&self) -> TypeTag {
        TypeTag::IntColumn
    }

    fn estimate_gain(&self, input: &CpacType, _ctx: &TransformContext) -> Option<f64> {
        // Zigzag is beneficial when there are negative values near zero.
        match input {
            CpacType::IntColumn { values, .. } => {
                let neg_count = values.iter().filter(|&&v| v < 0).count();
                if neg_count > 0 {
                    Some(neg_count as f64 / values.len() as f64 * 2.0)
                } else {
                    None // all non-negative, zigzag doesn't help
                }
            }
            _ => None,
        }
    }

    fn encode(&self, input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        match input {
            CpacType::IntColumn {
                values,
                original_width,
            } => {
                let encoded: Vec<i64> = values.iter().map(|&v| zigzag_encode(v) as i64).collect();
                Ok((
                    CpacType::IntColumn {
                        values: encoded,
                        original_width,
                    },
                    Vec::new(),
                ))
            }
            _ => Err(CpacError::Transform(
                "zigzag: unsupported input type".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, _metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::IntColumn {
                values,
                original_width,
            } => {
                let decoded: Vec<i64> = values.iter().map(|&v| zigzag_decode(v as u64)).collect();
                Ok(CpacType::IntColumn {
                    values: decoded,
                    original_width,
                })
            }
            _ => Err(CpacError::Transform(
                "zigzag: unsupported input type".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zigzag_basic_values() {
        assert_eq!(zigzag_encode(0), 0);
        assert_eq!(zigzag_encode(-1), 1);
        assert_eq!(zigzag_encode(1), 2);
        assert_eq!(zigzag_encode(-2), 3);
        assert_eq!(zigzag_encode(2), 4);
    }

    #[test]
    fn zigzag_roundtrip() {
        for v in [-1000, -1, 0, 1, 1000, i64::MIN, i64::MAX] {
            assert_eq!(zigzag_decode(zigzag_encode(v)), v);
        }
    }

    #[test]
    fn varint_roundtrip() {
        for v in [0u64, 1, 127, 128, 16383, 16384, u64::MAX] {
            let encoded = encode_varint(v);
            let (decoded, consumed) = decode_varint(&encoded).unwrap();
            assert_eq!(decoded, v);
            assert_eq!(consumed, encoded.len());
        }
    }

    #[test]
    fn signed_varint_roundtrip() {
        for v in [-1000i64, -1, 0, 1, 1000, i64::MIN, i64::MAX] {
            let encoded = encode_signed_varint(v);
            let (decoded, consumed) = decode_signed_varint(&encoded).unwrap();
            assert_eq!(decoded, v);
            assert_eq!(consumed, encoded.len());
        }
    }

    #[test]
    fn batch_roundtrip() {
        let values = vec![0, -1, 1, -1000, 1000, i64::MIN, i64::MAX];
        let encoded = zigzag_encode_batch(&values);
        let (decoded, _) = zigzag_decode_batch(&encoded).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn transform_node_roundtrip() {
        let t = ZigzagTransform;
        let ctx = TransformContext {
            entropy_estimate: 4.0,
            ascii_ratio: 0.0,
            data_size: 100,
        };
        let values = vec![-5, -1, 0, 1, 5, 100];
        let input = CpacType::IntColumn {
            values: values.clone(),
            original_width: 8,
        };
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::IntColumn { values: v, .. } => assert_eq!(v, values),
            _ => panic!("expected IntColumn"),
        }
    }
}
