// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Stride-elimination transform.
//!
//! Detects arithmetic sequences in serial data interpreted as fixed-width
//! little-endian integers. When the data forms a constant-stride sequence,
//! it is encoded as `(start, stride, count, width)` plus a residual for
//! any values that deviate from the sequence.
//!
//! Wire format (metadata):
//! `[width: 1][start: 8 LE (i64)][stride: 8 LE (i64)][count: 4 LE]`
//!
//! Payload: `[residual_count: 4 LE][per-residual: index(4 LE) + value(width bytes LE)]`
//!
//! If all values match the arithmetic sequence, payload is just `[0u32]`.

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for stride_elim (wire format).
pub const TRANSFORM_ID: u8 = 22;

/// Minimum number of elements to attempt stride detection.
const MIN_ELEMENTS: usize = 8;

/// Maximum fraction of deviations allowed (still apply if ≤10% deviate).
const MAX_DEVIATION_RATIO: f64 = 0.10;

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Stride-elimination transform node.
pub struct StrideElimTransform;

impl TransformNode for StrideElimTransform {
    fn name(&self) -> &str {
        "stride_elim"
    }

    fn id(&self) -> u8 {
        TRANSFORM_ID
    }

    fn accepts(&self) -> &[TypeTag] {
        &[TypeTag::Serial]
    }

    fn produces(&self) -> TypeTag {
        TypeTag::Serial
    }

    fn estimate_gain(&self, input: &CpacType, _ctx: &TransformContext) -> Option<f64> {
        match input {
            CpacType::Serial(data) => {
                // Try each width; return best gain
                for &width in &[1u8, 2, 4, 8] {
                    let w = width as usize;
                    if data.len() < w * MIN_ELEMENTS || !data.len().is_multiple_of(w) {
                        continue;
                    }
                    let count = data.len() / w;
                    let values = read_ints(data, width);
                    if values.len() < MIN_ELEMENTS {
                        continue;
                    }
                    let stride = values[1].wrapping_sub(values[0]);
                    let deviations = values
                        .iter()
                        .enumerate()
                        .filter(|(i, &v)| {
                            v != values[0].wrapping_add(stride.wrapping_mul(*i as i64))
                        })
                        .count();
                    let ratio = deviations as f64 / count as f64;
                    if ratio <= MAX_DEVIATION_RATIO {
                        // Metadata: 21 bytes. Payload: 4 + deviations*(4+width).
                        let encoded_size = 4 + deviations * (4 + w);
                        let savings = data.len() as f64 - encoded_size as f64;
                        if savings > 0.0 {
                            return Some(savings / data.len() as f64 * 10.0);
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }

    fn encode(&self, input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        match input {
            CpacType::Serial(data) => {
                // Try widths in order: 8, 4, 2, 1 (prefer wider for better compression)
                for &width in &[8u8, 4, 2, 1] {
                    let w = width as usize;
                    if data.len() < w * MIN_ELEMENTS || !data.len().is_multiple_of(w) {
                        continue;
                    }
                    let values = read_ints(&data, width);
                    if values.len() < MIN_ELEMENTS {
                        continue;
                    }

                    let start = values[0];
                    let stride = values[1].wrapping_sub(values[0]);
                    let count = values.len();

                    // Collect deviations
                    let mut residuals: Vec<(u32, i64)> = Vec::new();
                    for (i, &v) in values.iter().enumerate() {
                        let expected = start.wrapping_add(stride.wrapping_mul(i as i64));
                        if v != expected {
                            residuals.push((i as u32, v));
                        }
                    }

                    let ratio = residuals.len() as f64 / count as f64;
                    if ratio > MAX_DEVIATION_RATIO {
                        continue;
                    }

                    // Check if it actually saves space
                    let payload_size = 4 + residuals.len() * (4 + w);
                    if payload_size >= data.len() {
                        continue;
                    }

                    // Build metadata: [width:1][start:8 LE][stride:8 LE][count:4 LE]
                    let mut meta = Vec::with_capacity(21);
                    meta.push(width);
                    meta.extend_from_slice(&start.to_le_bytes());
                    meta.extend_from_slice(&stride.to_le_bytes());
                    meta.extend_from_slice(&(count as u32).to_le_bytes());

                    // Build payload: [residual_count:4 LE][per-residual: index(4)+value(width)]
                    let mut payload = Vec::with_capacity(payload_size);
                    payload.extend_from_slice(&(residuals.len() as u32).to_le_bytes());
                    for (idx, val) in &residuals {
                        payload.extend_from_slice(&idx.to_le_bytes());
                        write_int(&mut payload, *val, width);
                    }

                    return Ok((CpacType::Serial(payload), meta));
                }

                // No stride pattern found — passthrough
                Ok((CpacType::Serial(data), Vec::new()))
            }
            _ => Err(CpacError::Transform(
                "stride_elim: expected Serial input".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(payload) => {
                if metadata.is_empty() {
                    return Ok(CpacType::Serial(payload));
                }
                if metadata.len() < 21 {
                    return Err(CpacError::Transform(
                        "stride_elim: metadata too short".into(),
                    ));
                }

                let width = metadata[0];
                let w = width as usize;
                let start = i64::from_le_bytes([
                    metadata[1],
                    metadata[2],
                    metadata[3],
                    metadata[4],
                    metadata[5],
                    metadata[6],
                    metadata[7],
                    metadata[8],
                ]);
                let stride = i64::from_le_bytes([
                    metadata[9],
                    metadata[10],
                    metadata[11],
                    metadata[12],
                    metadata[13],
                    metadata[14],
                    metadata[15],
                    metadata[16],
                ]);
                let count =
                    u32::from_le_bytes([metadata[17], metadata[18], metadata[19], metadata[20]])
                        as usize;

                if payload.len() < 4 {
                    return Err(CpacError::Transform(
                        "stride_elim: payload too short".into(),
                    ));
                }

                let residual_count =
                    u32::from_le_bytes([payload[0], payload[1], payload[2], payload[3]]) as usize;

                // Reconstruct the arithmetic sequence
                let mut values: Vec<i64> = (0..count)
                    .map(|i| start.wrapping_add(stride.wrapping_mul(i as i64)))
                    .collect();

                // Apply residuals
                let mut offset = 4;
                for _ in 0..residual_count {
                    if offset + 4 + w > payload.len() {
                        return Err(CpacError::Transform(
                            "stride_elim: truncated residual".into(),
                        ));
                    }
                    let idx = u32::from_le_bytes([
                        payload[offset],
                        payload[offset + 1],
                        payload[offset + 2],
                        payload[offset + 3],
                    ]) as usize;
                    offset += 4;
                    let val = read_int(&payload[offset..], width);
                    offset += w;
                    if idx < values.len() {
                        values[idx] = val;
                    }
                }

                // Convert back to bytes
                let mut out = Vec::with_capacity(count * w);
                for &v in &values {
                    write_int(&mut out, v, width);
                }

                Ok(CpacType::Serial(out))
            }
            _ => Err(CpacError::Transform(
                "stride_elim: expected Serial input".into(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Integer helpers
// ---------------------------------------------------------------------------

/// Read serial data as a vector of signed integers with given byte width.
fn read_ints(data: &[u8], width: u8) -> Vec<i64> {
    let w = width as usize;
    if w == 0 || data.len() < w {
        return Vec::new();
    }
    data.chunks_exact(w)
        .map(|chunk| read_int(chunk, width))
        .collect()
}

/// Read a single LE integer from a byte slice.
fn read_int(data: &[u8], width: u8) -> i64 {
    match width {
        1 => i64::from(data[0] as i8),
        2 => i64::from(i16::from_le_bytes([data[0], data[1]])),
        4 => i64::from(i32::from_le_bytes([data[0], data[1], data[2], data[3]])),
        8 => i64::from_le_bytes([
            data[0], data[1], data[2], data[3], data[4], data[5], data[6], data[7],
        ]),
        _ => 0,
    }
}

/// Write a signed integer as LE bytes.
fn write_int(out: &mut Vec<u8>, val: i64, width: u8) {
    match width {
        1 => out.push(val as u8),
        2 => out.extend_from_slice(&(val as i16).to_le_bytes()),
        4 => out.extend_from_slice(&(val as i32).to_le_bytes()),
        8 => out.extend_from_slice(&val.to_le_bytes()),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_perfect_stride_u32() {
        let t = StrideElimTransform;
        // 0, 10, 20, 30, ... (100 values as i32 LE)
        let mut data = Vec::new();
        for i in 0..100i32 {
            data.extend_from_slice(&(i * 10).to_le_bytes());
        }
        let ctx = TransformContext {
            entropy_estimate: 4.0,
            ascii_ratio: 0.0,
            data_size: data.len(),
        };
        let input = CpacType::Serial(data.clone());
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        assert!(!meta.is_empty(), "stride should be detected");
        if let CpacType::Serial(ref enc_data) = encoded {
            assert!(enc_data.len() < data.len(), "should compress");
        }
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::Serial(d) => assert_eq!(d, data),
            _ => panic!("expected Serial"),
        }
    }

    #[test]
    fn roundtrip_stride_with_deviations() {
        let t = StrideElimTransform;
        let mut data = Vec::new();
        for i in 0..100i32 {
            let val = if i == 50 || i == 75 {
                999 // deviations
            } else {
                i * 5
            };
            data.extend_from_slice(&val.to_le_bytes());
        }
        let ctx = TransformContext {
            entropy_estimate: 4.0,
            ascii_ratio: 0.0,
            data_size: data.len(),
        };
        let input = CpacType::Serial(data.clone());
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        assert!(!meta.is_empty());
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::Serial(d) => assert_eq!(d, data),
            _ => panic!("expected Serial"),
        }
    }

    #[test]
    fn roundtrip_u8_stride() {
        let t = StrideElimTransform;
        // Byte-width stride: 0, 2, 4, 6, ... (128 values as i16 LE)
        let mut data = Vec::new();
        for i in 0..128i16 {
            data.extend_from_slice(&(i * 2).to_le_bytes());
        }
        let data = data;
        let ctx = TransformContext {
            entropy_estimate: 4.0,
            ascii_ratio: 0.0,
            data_size: data.len(),
        };
        let input = CpacType::Serial(data.clone());
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        assert!(!meta.is_empty());
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::Serial(d) => assert_eq!(d, data),
            _ => panic!("expected Serial"),
        }
    }

    #[test]
    fn passthrough_random() {
        let t = StrideElimTransform;
        let data: Vec<u8> = (0..200).map(|i| ((i * 37 + 13) % 256) as u8).collect();
        let ctx = TransformContext {
            entropy_estimate: 7.0,
            ascii_ratio: 0.3,
            data_size: data.len(),
        };
        let input = CpacType::Serial(data.clone());
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        assert!(meta.is_empty(), "should passthrough random data");
        match encoded {
            CpacType::Serial(d) => assert_eq!(d, data),
            _ => panic!("expected Serial"),
        }
    }

    #[test]
    fn empty_passthrough() {
        let t = StrideElimTransform;
        let ctx = TransformContext {
            entropy_estimate: 0.0,
            ascii_ratio: 0.0,
            data_size: 0,
        };
        let input = CpacType::Serial(Vec::new());
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        assert!(meta.is_empty());
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::Serial(d) => assert!(d.is_empty()),
            _ => panic!("expected Serial"),
        }
    }
}
