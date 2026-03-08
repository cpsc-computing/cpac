// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Constant-elimination transform.
//!
//! Detects data dominated by a single byte value and encodes it as
//! `(value, len, exception_bitmap, exceptions)`.
//!
//! Wire format (metadata):
//! `[dominant_byte: 1][original_len: 4 LE][exception_count: 4 LE]`
//!
//! Encoded payload: `[bitmap: ceil(original_len/8)][exception_bytes...]`
//!
//! The bitmap has bit=1 for positions that differ from the dominant byte.
//! `exception_bytes` stores only the non-dominant values in order.

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for const_elim (wire format).
pub const TRANSFORM_ID: u8 = 21;

/// Minimum dominance ratio to apply constant elimination.
const MIN_DOMINANCE: f64 = 0.90;

/// Minimum data size to consider.
const MIN_SIZE: usize = 64;

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Constant-elimination transform node.
pub struct ConstElimTransform;

impl TransformNode for ConstElimTransform {
    fn name(&self) -> &str {
        "const_elim"
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
            CpacType::Serial(data) if data.len() >= MIN_SIZE => {
                let (dominant, ratio) = dominant_byte(data);
                if ratio >= MIN_DOMINANCE {
                    // Rough estimate: encode exceptions only
                    let exceptions = data.iter().filter(|&&b| b != dominant).count();
                    let bitmap_bytes = data.len().div_ceil(8);
                    let encoded_size = bitmap_bytes + exceptions;
                    let savings = data.len() as f64 - encoded_size as f64;
                    if savings > 0.0 {
                        Some(savings / data.len() as f64 * 10.0)
                    } else {
                        None
                    }
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
                if data.len() < MIN_SIZE {
                    return Ok((CpacType::Serial(data), Vec::new()));
                }

                let (dominant, ratio) = dominant_byte(&data);
                if ratio < MIN_DOMINANCE {
                    return Ok((CpacType::Serial(data), Vec::new()));
                }

                let original_len = data.len();
                let bitmap_bytes = original_len.div_ceil(8);
                let mut bitmap = vec![0u8; bitmap_bytes];
                let mut exceptions = Vec::new();

                for (i, &b) in data.iter().enumerate() {
                    if b != dominant {
                        bitmap[i / 8] |= 1 << (i % 8);
                        exceptions.push(b);
                    }
                }

                // Only apply if it actually saves space
                let encoded_size = bitmap.len() + exceptions.len();
                if encoded_size >= original_len {
                    return Ok((CpacType::Serial(data), Vec::new()));
                }

                // Build payload: [bitmap][exceptions]
                let mut payload = Vec::with_capacity(encoded_size);
                payload.extend_from_slice(&bitmap);
                payload.extend_from_slice(&exceptions);

                // Build metadata: [dominant_byte:1][original_len:4 LE][exception_count:4 LE]
                let mut meta = Vec::with_capacity(9);
                meta.push(dominant);
                meta.extend_from_slice(&(original_len as u32).to_le_bytes());
                meta.extend_from_slice(&(exceptions.len() as u32).to_le_bytes());

                Ok((CpacType::Serial(payload), meta))
            }
            _ => Err(CpacError::Transform(
                "const_elim: expected Serial input".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(payload) => {
                if metadata.is_empty() {
                    // Passthrough — transform was not applied
                    return Ok(CpacType::Serial(payload));
                }
                if metadata.len() < 9 {
                    return Err(CpacError::Transform(
                        "const_elim: metadata too short".into(),
                    ));
                }

                let dominant = metadata[0];
                let original_len =
                    u32::from_le_bytes([metadata[1], metadata[2], metadata[3], metadata[4]])
                        as usize;
                let exception_count =
                    u32::from_le_bytes([metadata[5], metadata[6], metadata[7], metadata[8]])
                        as usize;

                let bitmap_bytes = original_len.div_ceil(8);
                if payload.len() < bitmap_bytes + exception_count {
                    return Err(CpacError::Transform("const_elim: payload too short".into()));
                }

                let bitmap = &payload[..bitmap_bytes];
                let exceptions = &payload[bitmap_bytes..bitmap_bytes + exception_count];

                let mut out = vec![dominant; original_len];
                let mut exc_idx = 0;
                for i in 0..original_len {
                    if bitmap[i / 8] & (1 << (i % 8)) != 0 {
                        if exc_idx >= exceptions.len() {
                            return Err(CpacError::Transform(
                                "const_elim: exception overflow".into(),
                            ));
                        }
                        out[i] = exceptions[exc_idx];
                        exc_idx += 1;
                    }
                }

                Ok(CpacType::Serial(out))
            }
            _ => Err(CpacError::Transform(
                "const_elim: expected Serial input".into(),
            )),
        }
    }
}

/// Find the most frequent byte and its dominance ratio.
fn dominant_byte(data: &[u8]) -> (u8, f64) {
    if data.is_empty() {
        return (0, 0.0);
    }
    let mut counts = [0u32; 256];
    for &b in data {
        counts[b as usize] += 1;
    }
    let mut best_byte = 0u8;
    let mut best_count = 0u32;
    for (i, &c) in counts.iter().enumerate() {
        if c > best_count {
            best_count = c;
            best_byte = i as u8;
        }
    }
    (best_byte, best_count as f64 / data.len() as f64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_dominant_zeros() {
        let t = ConstElimTransform;
        // 95% zeros with some exceptions
        let mut data = vec![0u8; 1000];
        for i in (0..1000).step_by(20) {
            data[i] = 0xFF;
        }
        let ctx = TransformContext {
            entropy_estimate: 1.0,
            ascii_ratio: 0.0,
            data_size: data.len(),
        };
        let input = CpacType::Serial(data.clone());
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        assert!(!meta.is_empty(), "transform should be applied");
        // Verify it's smaller
        if let CpacType::Serial(ref enc_data) = encoded {
            assert!(enc_data.len() < data.len());
        }
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::Serial(d) => assert_eq!(d, data),
            _ => panic!("expected Serial"),
        }
    }

    #[test]
    fn passthrough_diverse_data() {
        let t = ConstElimTransform;
        let data: Vec<u8> = (0..200).map(|i| (i % 256) as u8).collect();
        let ctx = TransformContext {
            entropy_estimate: 7.0,
            ascii_ratio: 0.3,
            data_size: data.len(),
        };
        let input = CpacType::Serial(data.clone());
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        assert!(meta.is_empty(), "should passthrough diverse data");
        match encoded {
            CpacType::Serial(d) => assert_eq!(d, data),
            _ => panic!("expected Serial"),
        }
    }

    #[test]
    fn empty_passthrough() {
        let t = ConstElimTransform;
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

    #[test]
    fn roundtrip_all_same() {
        let t = ConstElimTransform;
        let data = vec![0x42u8; 500];
        let ctx = TransformContext {
            entropy_estimate: 0.0,
            ascii_ratio: 1.0,
            data_size: data.len(),
        };
        let input = CpacType::Serial(data.clone());
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        assert!(!meta.is_empty());
        if let CpacType::Serial(ref enc_data) = encoded {
            // Should be just bitmap, no exceptions
            assert!(enc_data.len() < 100);
        }
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::Serial(d) => assert_eq!(d, data),
            _ => panic!("expected Serial"),
        }
    }
}
