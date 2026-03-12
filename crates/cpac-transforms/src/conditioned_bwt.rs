// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Conditioned BWT transform (Phase 4).
//!
//! Splits data into entropy-homogeneous streams via `cpac_conditioning::partition`,
//! applies BWT+MTF+RLE independently to each qualifying stream, then serializes
//! the modified partition.  On decode, the BWT chain is reversed per-stream before
//! merging back to the original byte order.
//!
//! This improves compression because BWT is most effective on text-like data with
//! moderate entropy.  Partitioning first ensures binary/high-entropy segments are
//! not mixed into the sort, which would degrade the BWT's run-creation effect.
//!
//! Wire format:
//!   payload = serialized partition (with BWT'd streams)
//!   metadata = `[applied_mask: 1][per-applied-stream: original_idx(4 LE)]`
//!
//! `applied_mask` is a bitmask of which stream indices had BWT applied (up to 8 streams).

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::bwt;
use crate::mtf;
use crate::rle;
use crate::traits::{TransformContext, TransformNode};

/// Transform ID for conditioned_bwt (wire format).
pub const TRANSFORM_ID: u8 = 26;

/// Minimum total data size to attempt this transform.
const MIN_SIZE: usize = 512;

/// Minimum individual stream size worth applying BWT to.
const MIN_STREAM_SIZE: usize = 128;

/// Minimum number of distinct non-empty streams to justify partitioning.
const MIN_DISTINCT_STREAMS: usize = 2;

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Conditioned BWT transform node.
pub struct ConditionedBwtTransform;

impl TransformNode for ConditionedBwtTransform {
    fn name(&self) -> &str {
        "conditioned_bwt"
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
                // Most beneficial for mixed-content data with text-like portions
                if ctx.entropy_estimate > 2.0
                    && ctx.entropy_estimate < 6.5
                    && ctx.ascii_ratio > 0.50
                {
                    // Stronger gain than plain conditioning or plain BWT alone
                    Some(ctx.ascii_ratio * 4.0 * (1.0 - ctx.entropy_estimate / 8.0))
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

                // Step 1: Partition into entropy-homogeneous streams
                let partition = cpac_conditioning::partition(&data);

                let non_empty = partition.streams.iter().filter(|s| !s.is_empty()).count();
                if non_empty < MIN_DISTINCT_STREAMS {
                    return Ok((CpacType::Serial(data), Vec::new()));
                }

                // Step 2: Apply BWT chain to qualifying streams
                let mut applied_mask: u8 = 0;
                let mut bwt_indices: Vec<u32> = Vec::new();
                let mut modified_streams = partition.streams.clone();

                for (i, modified) in modified_streams.iter_mut().enumerate() {
                    let stream = &partition.streams[i];
                    if stream.len() >= MIN_STREAM_SIZE && stream.len() <= bwt::BWT_MAX_SIZE {
                        // Try BWT+MTF+RLE on this stream
                        if let Ok((bwt_data, original_idx)) = bwt::bwt_encode(stream) {
                            let mtf_data = mtf::mtf_encode(&bwt_data)
                                .unwrap_or(bwt_data);
                            let (rle_data, _) = rle::rle_encode(&mtf_data);

                            // Only apply if it actually helps
                            if rle_data.len() < stream.len() {
                                applied_mask |= 1 << i;
                                bwt_indices.push(original_idx as u32);
                                *modified = rle_data;
                            }
                        }
                    }
                }

                // If no stream benefited from BWT, bail
                if applied_mask == 0 {
                    return Ok((CpacType::Serial(data), Vec::new()));
                }

                // Step 3: Serialize the modified partition
                let modified_partition = cpac_conditioning::PartitionResult {
                    streams: modified_streams,
                    position_map: partition.position_map,
                };
                let serialized = cpac_conditioning::serialize_partition(&modified_partition);

                // Only apply if total output is smaller
                if serialized.len() >= data.len() {
                    return Ok((CpacType::Serial(data), Vec::new()));
                }

                // Step 4: Build metadata
                let mut meta = Vec::with_capacity(1 + bwt_indices.len() * 4);
                meta.push(applied_mask);
                for idx in &bwt_indices {
                    meta.extend_from_slice(&idx.to_le_bytes());
                }

                Ok((CpacType::Serial(serialized), meta))
            }
            _ => Err(CpacError::Transform(
                "conditioned_bwt: expected Serial input".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                if metadata.is_empty() {
                    // Passthrough — transform was not applied
                    return Ok(CpacType::Serial(data));
                }

                let applied_mask = metadata[0];

                // Read BWT indices for applied streams
                let applied_count = applied_mask.count_ones() as usize;
                if metadata.len() < 1 + applied_count * 4 {
                    return Err(CpacError::Transform(
                        "conditioned_bwt: metadata truncated".into(),
                    ));
                }
                let mut bwt_indices = Vec::with_capacity(applied_count);
                let mut offset = 1;
                for _ in 0..applied_count {
                    let idx = u32::from_le_bytes([
                        metadata[offset],
                        metadata[offset + 1],
                        metadata[offset + 2],
                        metadata[offset + 3],
                    ]);
                    bwt_indices.push(idx as usize);
                    offset += 4;
                }

                // Step 1: Deserialize partition
                let mut partition = cpac_conditioning::deserialize_partition(&data)?;

                // Step 2: Reverse BWT chain on applied streams
                let mut idx_cursor = 0;
                for i in 0..4 {
                    if applied_mask & (1 << i) != 0 {
                        let original_idx = bwt_indices[idx_cursor];
                        idx_cursor += 1;

                        // Un-RLE
                        let mtf_data = rle::rle_decode(&partition.streams[i])?;
                        // Un-MTF
                        let bwt_data = mtf::mtf_decode(&mtf_data)?;
                        // Un-BWT
                        let original = bwt::bwt_decode(&bwt_data, original_idx)?;
                        partition.streams[i] = original;
                    }
                }

                // Step 3: Merge back to original order
                let merged = cpac_conditioning::merge(&partition)?;
                Ok(CpacType::Serial(merged))
            }
            _ => Err(CpacError::Transform(
                "conditioned_bwt: expected Serial input".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_mixed_content() {
        let t = ConditionedBwtTransform;
        // Mix of text and structured data
        let mut data = Vec::new();
        data.extend_from_slice(
            b"The quick brown fox jumps over the lazy dog. ".repeat(15).as_slice(),
        );
        // Add some numeric/structural bytes
        for i in 0u8..200 {
            data.push(i);
            data.push(0x00);
            data.push(0xFF);
        }
        data.extend_from_slice(
            b"Another repeated text segment for testing. ".repeat(10).as_slice(),
        );

        let ctx = TransformContext {
            entropy_estimate: 4.5,
            ascii_ratio: 0.75,
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
        let t = ConditionedBwtTransform;
        let data = b"tiny data";
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
    fn roundtrip_text_heavy() {
        let t = ConditionedBwtTransform;
        let data = b"abcdefghijklmnop ".repeat(50);
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
}
