// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Run-Length Encoding (RLE) pre-pass transform.
//!
//! For data with long runs of identical values (NULLs, zeros, repeated
//! categories), explicit RLE is more efficient than LZ77 across large
//! distances.
//!
//! Wire format: `[original_len: 4 LE][pairs: (value: 1 byte, count: varint)...]`

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};
use crate::zigzag::{decode_varint, encode_varint};

/// Transform ID for RLE (wire format).
pub const TRANSFORM_ID: u8 = 14;

// ---------------------------------------------------------------------------
// Core encode/decode
// ---------------------------------------------------------------------------

/// RLE-encode a byte slice.
///
/// Returns `(encoded_bytes, run_count)`.
#[must_use]
pub fn rle_encode(data: &[u8]) -> (Vec<u8>, usize) {
    if data.is_empty() {
        return (Vec::new(), 0);
    }

    let mut out = Vec::new();
    out.extend_from_slice(&(data.len() as u32).to_le_bytes());

    let mut run_count = 0usize;
    let mut i = 0;
    while i < data.len() {
        let value = data[i];
        let mut count = 1usize;
        while i + count < data.len() && data[i + count] == value {
            count += 1;
        }
        out.push(value);
        out.extend_from_slice(&encode_varint(count as u64));
        run_count += 1;
        i += count;
    }

    (out, run_count)
}

/// RLE-decode back to original bytes.
pub fn rle_decode(data: &[u8]) -> CpacResult<Vec<u8>> {
    if data.len() < 4 {
        return Err(CpacError::Transform("rle: insufficient header".into()));
    }
    let original_len =
        u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    let mut out = Vec::with_capacity(original_len);
    let mut offset = 4;

    while offset < data.len() && out.len() < original_len {
        let value = data[offset];
        offset += 1;
        let (count, consumed) = decode_varint(&data[offset..])?;
        offset += consumed;
        let count = count as usize;
        out.extend(std::iter::repeat_n(value, count));
    }

    if out.len() != original_len {
        return Err(CpacError::Transform(format!(
            "rle: decoded {} bytes, expected {original_len}",
            out.len()
        )));
    }

    Ok(out)
}

/// Compute average run length for a byte slice.
#[must_use]
pub fn average_run_length(data: &[u8]) -> f64 {
    if data.is_empty() {
        return 0.0;
    }
    let mut runs = 0usize;
    let mut i = 0;
    while i < data.len() {
        let value = data[i];
        while i < data.len() && data[i] == value {
            i += 1;
        }
        runs += 1;
    }
    data.len() as f64 / runs as f64
}

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// RLE transform node for the compression DAG.
pub struct RleTransform;

impl TransformNode for RleTransform {
    fn name(&self) -> &'static str {
        "rle"
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
                if data.len() < 64 {
                    return None;
                }
                let avg_run = average_run_length(data);
                if avg_run >= 4.0 {
                    // Bigger runs = bigger savings
                    Some(avg_run * 2.0)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn encode(&self, input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        match input {
            CpacType::Serial(data) => {
                let (encoded, _runs) = rle_encode(&data);
                // Only apply if RLE actually saves space
                if encoded.len() < data.len() {
                    Ok((CpacType::Serial(encoded), Vec::new()))
                } else {
                    Ok((CpacType::Serial(data), Vec::new()))
                }
            }
            _ => Err(CpacError::Transform("rle: unsupported input type".into())),
        }
    }

    fn decode(&self, input: CpacType, _metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                // Check if this is actually RLE-encoded (has 4-byte length header)
                if data.len() >= 4 {
                    match rle_decode(&data) {
                        Ok(decoded) => Ok(CpacType::Serial(decoded)),
                        Err(_) => Ok(CpacType::Serial(data)), // passthrough if not RLE
                    }
                } else {
                    Ok(CpacType::Serial(data))
                }
            }
            _ => Err(CpacError::Transform("rle: unsupported input type".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_runs() {
        let data: Vec<u8> = vec![0; 100]
            .into_iter()
            .chain(vec![1; 50])
            .chain(vec![2; 200])
            .collect();
        let (encoded, runs) = rle_encode(&data);
        assert_eq!(runs, 3);
        assert!(encoded.len() < data.len());
        let decoded = rle_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn roundtrip_no_runs() {
        let data: Vec<u8> = (0..=255).collect();
        let (encoded, runs) = rle_encode(&data);
        assert_eq!(runs, 256);
        let decoded = rle_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn roundtrip_empty() {
        let (encoded, runs) = rle_encode(&[]);
        assert_eq!(runs, 0);
        assert!(encoded.is_empty());
    }

    #[test]
    fn average_run_length_computation() {
        let data = vec![0; 100];
        assert!((average_run_length(&data) - 100.0).abs() < f64::EPSILON);

        let data: Vec<u8> = (0..100).collect();
        assert!((average_run_length(&data) - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn transform_node_roundtrip() {
        let t = RleTransform;
        let data: Vec<u8> = vec![0; 100]
            .into_iter()
            .chain(vec![1; 100])
            .chain(vec![2; 100])
            .collect();
        let ctx = TransformContext {
            entropy_estimate: 1.0,
            ascii_ratio: 0.0,
            data_size: data.len(),
        };
        let input = CpacType::Serial(data.clone());
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::Serial(d) => assert_eq!(d, data),
            _ => panic!("expected Serial"),
        }
    }
}
