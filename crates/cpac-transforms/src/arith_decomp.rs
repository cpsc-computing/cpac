// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Arithmetic decomposition transform.
//!
//! Decomposes integer values into `(quotient, remainder)` pairs for a chosen
//! modulus. If the modulus matches a hidden period in the data, the quotient
//! stream becomes monotone and the remainder stream becomes periodic — both
//! highly compressible.
//!
//! Wire format:
//! `[modulus: 2 LE][count: 4 LE][quotients: count × varint][remainders: count × 1 byte]`

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};
use crate::zigzag::{decode_varint, encode_varint};

/// Transform ID for arith_decomp (wire format).
pub const TRANSFORM_ID: u8 = 20;

// ---------------------------------------------------------------------------
// Core encode/decode
// ---------------------------------------------------------------------------

/// Score a modulus by measuring combined "run-friendliness" of quotient
/// and remainder streams. Higher is better.
fn score_modulus(values: &[i64], modulus: u16) -> f64 {
    if values.len() < 2 {
        return 0.0;
    }
    let m = modulus as i64;

    // Count runs in remainder stream
    let mut rem_runs = 1usize;
    let mut prev_rem = values[0] % m;
    for &v in &values[1..] {
        let r = v % m;
        if r != prev_rem {
            rem_runs += 1;
            prev_rem = r;
        }
    }

    // Count monotonic segments in quotient stream
    let mut mono_segments = 1usize;
    let mut prev_q = values[0] / m;
    let mut prev_dir: i64 = 0;
    for &v in &values[1..] {
        let q = v / m;
        let dir = (q - prev_q).signum();
        if dir != 0 && dir != prev_dir && prev_dir != 0 {
            mono_segments += 1;
        }
        if dir != 0 {
            prev_dir = dir;
        }
        prev_q = q;
    }

    let n = values.len() as f64;
    let rem_score = 1.0 - (rem_runs as f64 / n);
    let quo_score = 1.0 - (mono_segments as f64 / n);
    rem_score + quo_score
}

/// Find the best modulus from a set of candidates.
fn find_best_modulus(values: &[i64]) -> u16 {
    // Try common periods and powers of 2
    let candidates: Vec<u16> = vec![
        2, 3, 4, 5, 6, 7, 8, 10, 12, 16, 24, 32, 60, 64, 100, 128, 256,
    ];

    candidates
        .into_iter()
        .max_by(|&a, &b| {
            let sa = score_modulus(values, a);
            let sb = score_modulus(values, b);
            sa.partial_cmp(&sb)
                .unwrap_or(std::cmp::Ordering::Equal)
                // Tiebreaker: prefer smaller moduli (fewer remainder bits)
                .then(b.cmp(&a))
        })
        .unwrap_or(256)
}

/// Encode integer column using arithmetic decomposition.
///
/// Returns `(encoded_bytes, modulus_used)`.
pub fn arith_decomp_encode(values: &[i64], modulus: u16) -> Vec<u8> {
    let m = modulus as i64;
    let mut out = Vec::new();
    out.extend_from_slice(&modulus.to_le_bytes());
    out.extend_from_slice(&(values.len() as u32).to_le_bytes());

    // Quotients as zigzag varints
    for &v in values {
        let q = if v >= 0 { v / m } else { (v - m + 1) / m };
        let zz = if q >= 0 {
            (q as u64) << 1
        } else {
            (((-q) as u64) << 1) - 1
        };
        out.extend_from_slice(&encode_varint(zz));
    }

    // Remainders as raw bytes (modulus ≤ 256 → fits in u8)
    for &v in values {
        let r = ((v % m) + m) % m;
        out.push(r as u8);
    }

    out
}

/// Decode arithmetic decomposition.
pub fn arith_decomp_decode(data: &[u8]) -> CpacResult<Vec<i64>> {
    if data.len() < 6 {
        return Err(CpacError::Transform("arith_decomp: too short".into()));
    }

    let modulus = u16::from_le_bytes([data[0], data[1]]) as i64;
    let count =
        u32::from_le_bytes([data[2], data[3], data[4], data[5]]) as usize;

    // Read quotients
    let mut offset = 6;
    let mut quotients = Vec::with_capacity(count);
    for _ in 0..count {
        let (zz, consumed) = decode_varint(&data[offset..])?;
        offset += consumed;
        let q = if zz & 1 == 0 {
            (zz >> 1) as i64
        } else {
            -(((zz + 1) >> 1) as i64)
        };
        quotients.push(q);
    }

    // Read remainders
    if offset + count > data.len() {
        return Err(CpacError::Transform(
            "arith_decomp: truncated remainders".into(),
        ));
    }
    let mut values = Vec::with_capacity(count);
    for i in 0..count {
        let r = data[offset + i] as i64;
        values.push(quotients[i] * modulus + r);
    }

    Ok(values)
}

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Arithmetic decomposition transform node.
pub struct ArithDecompTransform;

impl TransformNode for ArithDecompTransform {
    fn name(&self) -> &str {
        "arith_decomp"
    }

    fn id(&self) -> u8 {
        TRANSFORM_ID
    }

    fn accepts(&self) -> &[TypeTag] {
        &[TypeTag::IntColumn]
    }

    fn produces(&self) -> TypeTag {
        TypeTag::Serial
    }

    fn estimate_gain(&self, input: &CpacType, _ctx: &TransformContext) -> Option<f64> {
        match input {
            CpacType::IntColumn { values, .. } => {
                if values.len() < 32 {
                    return None;
                }
                let best_mod = find_best_modulus(values);
                let score = score_modulus(values, best_mod);
                if score > 1.0 {
                    Some(score * 3.0)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn encode(&self, input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        match input {
            CpacType::IntColumn { values, .. } => {
                if values.is_empty() {
                    return Ok((CpacType::Serial(Vec::new()), Vec::new()));
                }
                let modulus = find_best_modulus(&values);
                let encoded = arith_decomp_encode(&values, modulus);

                // Only apply if it's smaller than raw i64 encoding
                let raw_size = values.len() * 8;
                if encoded.len() >= raw_size {
                    // Passthrough: serialize as-is
                    let mut raw = Vec::with_capacity(raw_size);
                    for &v in &values {
                        raw.extend_from_slice(&v.to_le_bytes());
                    }
                    return Ok((CpacType::Serial(raw), Vec::new()));
                }

                Ok((CpacType::Serial(encoded), vec![1])) // marker: decomposed
            }
            _ => Err(CpacError::Transform(
                "arith_decomp: expected IntColumn input".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                if metadata.is_empty() || data.is_empty() {
                    // Passthrough
                    return Ok(CpacType::Serial(data));
                }
                let values = arith_decomp_decode(&data)?;
                Ok(CpacType::IntColumn {
                    values,
                    original_width: 8,
                })
            }
            _ => Err(CpacError::Transform(
                "arith_decomp: expected Serial input".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_periodic() {
        // Periodic data: 0,1,2,3,0,1,2,3,...  mod 4 should decompose well
        let values: Vec<i64> = (0..100).map(|i| i % 4).collect();
        let encoded = arith_decomp_encode(&values, 4);
        let decoded = arith_decomp_decode(&encoded).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn roundtrip_monotone() {
        let values: Vec<i64> = (0..100).collect();
        let modulus = 10u16;
        let encoded = arith_decomp_encode(&values, modulus);
        let decoded = arith_decomp_decode(&encoded).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn roundtrip_negative() {
        let values: Vec<i64> = (-50..50).collect();
        let modulus = 7u16;
        let encoded = arith_decomp_encode(&values, modulus);
        let decoded = arith_decomp_decode(&encoded).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn best_modulus_periodic() {
        // Data with period 5
        let values: Vec<i64> = (0..200).map(|i| i % 5).collect();
        let best = find_best_modulus(&values);
        // Should find 5 as the best modulus
        assert_eq!(best, 5, "expected modulus 5 for period-5 data");
    }

    #[test]
    fn transform_node_roundtrip() {
        let t = ArithDecompTransform;
        let values: Vec<i64> = (0..200).map(|i| i * 7 + i % 3).collect();
        let ctx = TransformContext {
            entropy_estimate: 4.0,
            ascii_ratio: 0.0,
            data_size: 1600,
        };
        let input = CpacType::IntColumn {
            values: values.clone(),
            original_width: 8,
        };
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::IntColumn { values: restored, .. } => {
                assert_eq!(restored, values);
            }
            CpacType::Serial(_) => {
                // Passthrough if no benefit — also valid
            }
            _ => panic!("unexpected type"),
        }
    }

    #[test]
    fn score_measures_periodicity() {
        let periodic: Vec<i64> = (0..100).map(|i| i % 4).collect();
        let random: Vec<i64> = (0..100).map(|i| i * 37 % 251).collect();
        assert!(
            score_modulus(&periodic, 4) > score_modulus(&random, 4),
            "periodic data should score higher"
        );
    }
}
