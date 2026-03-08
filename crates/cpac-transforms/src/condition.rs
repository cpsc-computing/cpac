// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Entropy conditioning transform.
//!
//! Splits data into entropy-homogeneous streams (structural, numeric, text,
//! high-entropy) so each can be compressed with a better-tuned symbol
//! distribution. On decode, streams are merged back into original byte order.
//!
//! Wire format: metadata stores the serialized partition (position map +
//! stream lengths). Payload is the concatenated conditioned streams.

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for condition (wire format).
pub const TRANSFORM_ID: u8 = 23;

/// Minimum size to attempt conditioning (overhead isn't worth it for tiny data).
const MIN_SIZE: usize = 256;

/// Minimum number of distinct non-empty streams to justify conditioning.
/// If all bytes fall into one class, conditioning adds overhead with no benefit.
const MIN_DISTINCT_STREAMS: usize = 2;

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Entropy conditioning transform node.
pub struct ConditionTransform;

impl TransformNode for ConditionTransform {
    fn name(&self) -> &str {
        "condition"
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

    fn estimate_gain(&self, input: &CpacType, ctx: &TransformContext) -> Option<f64> {
        match input {
            CpacType::Serial(data) if data.len() >= MIN_SIZE => {
                // Only beneficial for mixed-content data (moderate entropy + text-like)
                if ctx.entropy_estimate > 2.0 && ctx.entropy_estimate < 7.0 && ctx.ascii_ratio > 0.50 {
                    // Rough heuristic: more entropy variance = more benefit
                    Some((ctx.entropy_estimate / 8.0) * ctx.ascii_ratio * 3.0)
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

                let result = cpac_conditioning::partition(&data);

                // Check if conditioning is worthwhile
                let non_empty = result.streams.iter().filter(|s| !s.is_empty()).count();
                if non_empty < MIN_DISTINCT_STREAMS {
                    return Ok((CpacType::Serial(data), Vec::new()));
                }

                let serialized = cpac_conditioning::serialize_partition(&result);

                // Only apply if the serialized form is smaller than original
                if serialized.len() >= data.len() {
                    return Ok((CpacType::Serial(data), Vec::new()));
                }

                // Metadata stores the partition; payload is the serialized form
                // (contains position map + streams — the decoder reconstructs via merge)
                let meta_len = (serialized.len() as u32).to_le_bytes();
                let meta = meta_len.to_vec();

                Ok((CpacType::Serial(serialized), meta))
            }
            _ => Err(CpacError::Transform(
                "condition: expected Serial input".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                if metadata.is_empty() {
                    return Ok(CpacType::Serial(data));
                }
                // Deserialize partition from payload and merge
                let result = cpac_conditioning::deserialize_partition(&data)?;
                let merged = cpac_conditioning::merge(&result)?;
                Ok(CpacType::Serial(merged))
            }
            _ => Err(CpacError::Transform(
                "condition: expected Serial input".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_json() {
        let t = ConditionTransform;
        let data = br#"{"name": "Alice", "age": 30, "scores": [95, 87, 92], "active": true}"#
            .repeat(10);
        let ctx = TransformContext {
            entropy_estimate: 4.5,
            ascii_ratio: 1.0,
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

    #[test]
    fn passthrough_small() {
        let t = ConditionTransform;
        let data = b"tiny";
        let ctx = TransformContext {
            entropy_estimate: 3.0,
            ascii_ratio: 1.0,
            data_size: data.len(),
        };
        let input = CpacType::Serial(data.to_vec());
        let (_, meta) = t.encode(input, &ctx).unwrap();
        assert!(meta.is_empty());
    }

    #[test]
    fn passthrough_uniform() {
        let t = ConditionTransform;
        // All same class (HighEntropy) — conditioning adds no value
        let data = vec![0xFFu8; 300];
        let ctx = TransformContext {
            entropy_estimate: 0.0,
            ascii_ratio: 0.0,
            data_size: data.len(),
        };
        let input = CpacType::Serial(data.clone());
        let (_, meta) = t.encode(input, &ctx).unwrap();
        assert!(meta.is_empty());
    }
}
