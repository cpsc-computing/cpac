// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! BWT → MTF → RLE chain transform (bzip2-style pipeline).
//!
//! Combines three transforms into a single DAG node. The BWT sorts the data
//! to create long runs of identical bytes, MTF converts those runs into
//! sequences of small integers (dominated by zeros), and RLE compresses
//! the zero-runs.
//!
//! Wire format:
//! `[original_idx: 4 LE][rle_len: 4 LE][rle_data...]`
//!
//! Metadata stores the BWT original index.

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::bwt;
use crate::mtf;
use crate::rle;
use crate::traits::{TransformContext, TransformNode};

/// Transform ID for bwt_chain (wire format).
pub const TRANSFORM_ID: u8 = 18;

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// BWT+MTF+RLE chain transform node.
pub struct BwtChainTransform;

impl TransformNode for BwtChainTransform {
    fn name(&self) -> &str {
        "bwt_chain"
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
            CpacType::Serial(data) => {
                // BWT is O(n^2) for our simple impl — only apply to medium-sized data
                if data.len() < 256 || data.len() > 1_000_000 {
                    return None;
                }
                // Most beneficial for text data with moderate entropy
                if ctx.ascii_ratio > 0.7 && ctx.entropy_estimate < 6.0 {
                    Some(ctx.ascii_ratio * 5.0)
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
                // Guard: BWT suffix-sort is O(n² log n) — skip for empty or large data.
                if data.is_empty() || data.len() > 1_000_000 {
                    return Ok((CpacType::Serial(data), Vec::new()));
                }

                // Step 1: BWT
                let (bwt_data, original_idx) = bwt::bwt_encode(&data)?;

                // Step 2: MTF
                let mtf_data = mtf::mtf_encode(&bwt_data)?;

                // Step 3: RLE
                let (rle_data, _runs) = rle::rle_encode(&mtf_data);

                // Only apply if the chain actually saves space
                if rle_data.len() >= data.len() {
                    return Ok((CpacType::Serial(data), Vec::new()));
                }

                // Metadata: just the BWT original index
                let mut meta = Vec::with_capacity(4);
                meta.extend_from_slice(&(original_idx as u32).to_le_bytes());

                Ok((CpacType::Serial(rle_data), meta))
            }
            _ => Err(CpacError::Transform(
                "bwt_chain: expected Serial input".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                if metadata.is_empty() {
                    // Passthrough — chain was not applied
                    return Ok(CpacType::Serial(data));
                }
                if metadata.len() < 4 {
                    return Err(CpacError::Transform(
                        "bwt_chain: metadata too short".into(),
                    ));
                }

                let original_idx = u32::from_le_bytes([
                    metadata[0],
                    metadata[1],
                    metadata[2],
                    metadata[3],
                ]) as usize;

                // Step 1: Un-RLE
                let mtf_data = rle::rle_decode(&data)?;

                // Step 2: Un-MTF
                let bwt_data = mtf::mtf_decode(&mtf_data)?;

                // Step 3: Un-BWT
                let original = bwt::bwt_decode(&bwt_data, original_idx)?;

                Ok(CpacType::Serial(original))
            }
            _ => Err(CpacError::Transform(
                "bwt_chain: expected Serial input".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_text() {
        let t = BwtChainTransform;
        let data = b"the quick brown fox jumps over the lazy dog and then some more text repeating itself the quick brown fox jumps over the lazy dog again and the quick brown fox".to_vec();
        let ctx = TransformContext {
            entropy_estimate: 4.0,
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
    fn roundtrip_repetitive() {
        let t = BwtChainTransform;
        let data = b"ababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababababab".to_vec();
        let ctx = TransformContext {
            entropy_estimate: 1.0,
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
    fn empty_passthrough() {
        let t = BwtChainTransform;
        let ctx = TransformContext {
            entropy_estimate: 4.0,
            ascii_ratio: 1.0,
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
