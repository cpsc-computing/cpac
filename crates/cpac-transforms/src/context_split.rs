// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Context splitting transform.
//!
//! Splits each byte into multiple channels based on the previous byte's
//! context class. Each channel has lower entropy than the original mixed
//! stream, improving backend compression.
//!
//! Wire format:
//! `[num_contexts: 1][per-context: channel_len: 4 LE + channel_bytes...][context_map: N bytes]`
//!
//! The context_map records which context each byte in the original stream
//! belonged to, enabling reconstruction.

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for context_split (wire format).
pub const TRANSFORM_ID: u8 = 19;

/// Number of context classes.
const NUM_CONTEXTS: usize = 4;

// ---------------------------------------------------------------------------
// Core encode/decode
// ---------------------------------------------------------------------------

/// Map a byte to its context class (0..3).
#[inline]
fn context_class(b: u8) -> u8 {
    match b {
        0..=31 => 0,        // control characters
        32..=95 => 1,       // printable ASCII lower (space, digits, uppercase)
        96..=127 => 2,      // printable ASCII upper (lowercase, punctuation)
        128..=255 => 3,     // high bytes
    }
}

/// Split data into context-based channels.
///
/// Returns `(channels, context_map)`.
pub fn context_split_encode(data: &[u8]) -> (Vec<Vec<u8>>, Vec<u8>) {
    let mut channels: Vec<Vec<u8>> = vec![Vec::new(); NUM_CONTEXTS];
    let mut context_map = Vec::with_capacity(data.len());

    let mut prev_class = 0u8;
    for (i, &b) in data.iter().enumerate() {
        let ctx = if i == 0 {
            0 // first byte always goes to context 0
        } else {
            prev_class
        };
        channels[ctx as usize].push(b);
        context_map.push(ctx);
        prev_class = context_class(b);
    }

    (channels, context_map)
}

/// Reconstruct data from context channels and context map.
pub fn context_split_decode(
    channels: &[Vec<u8>],
    context_map: &[u8],
) -> CpacResult<Vec<u8>> {
    let mut channel_pos = [0usize; NUM_CONTEXTS];
    let mut out = Vec::with_capacity(context_map.len());

    for &ctx in context_map {
        let ci = ctx as usize;
        if ci >= channels.len() || channel_pos[ci] >= channels[ci].len() {
            return Err(CpacError::Transform(
                "context_split: channel underflow".into(),
            ));
        }
        out.push(channels[ci][channel_pos[ci]]);
        channel_pos[ci] += 1;
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Context-splitting transform node.
pub struct ContextSplitTransform;

impl TransformNode for ContextSplitTransform {
    fn name(&self) -> &str {
        "context_split"
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
                if data.len() < 256 {
                    return None;
                }
                // Most beneficial for mixed-content data (has both text and binary)
                if ctx.ascii_ratio > 0.3 && ctx.ascii_ratio < 0.9 && ctx.entropy_estimate > 4.0 {
                    Some(2.0)
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
                if data.is_empty() {
                    return Ok((CpacType::Serial(Vec::new()), Vec::new()));
                }

                let (channels, context_map) = context_split_encode(&data);

                // Pack channels into output stream
                let mut out = Vec::new();
                out.push(NUM_CONTEXTS as u8);
                for ch in &channels {
                    out.extend_from_slice(&(ch.len() as u32).to_le_bytes());
                    out.extend_from_slice(ch);
                }

                // Check if splitting provides any benefit.
                // The packed output = 1 (num_ctx) + 4*NUM_CONTEXTS (headers) + channel bytes.
                // The metadata is the context_map (stored separately).
                // Compare packed output against original size.
                let packed_size = 1 + NUM_CONTEXTS * 4 + data.len(); // channels sum == data.len()
                if packed_size >= data.len() {
                    // Packed output + context_map overhead exceeds original — skip.
                    return Ok((CpacType::Serial(data), Vec::new()));
                }

                // Store context map in metadata
                Ok((CpacType::Serial(out), context_map))
            }
            _ => Err(CpacError::Transform(
                "context_split: expected Serial input".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                if metadata.is_empty() {
                    // Passthrough
                    return Ok(CpacType::Serial(data));
                }

                if data.is_empty() {
                    return Ok(CpacType::Serial(Vec::new()));
                }

                // Parse channels from packed stream
                let num_ctx = data[0] as usize;
                let mut offset = 1;
                let mut channels = Vec::with_capacity(num_ctx);
                for _ in 0..num_ctx {
                    if offset + 4 > data.len() {
                        return Err(CpacError::Transform(
                            "context_split: truncated channel header".into(),
                        ));
                    }
                    let ch_len = u32::from_le_bytes([
                        data[offset],
                        data[offset + 1],
                        data[offset + 2],
                        data[offset + 3],
                    ]) as usize;
                    offset += 4;
                    if offset + ch_len > data.len() {
                        return Err(CpacError::Transform(
                            "context_split: truncated channel data".into(),
                        ));
                    }
                    channels.push(data[offset..offset + ch_len].to_vec());
                    offset += ch_len;
                }

                let restored = context_split_decode(&channels, metadata)?;
                Ok(CpacType::Serial(restored))
            }
            _ => Err(CpacError::Transform(
                "context_split: expected Serial input".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_mixed() {
        let data = b"Hello World\x00\xFF\x01test\x80data123";
        let (channels, ctx_map) = context_split_encode(data);
        let restored = context_split_decode(&channels, &ctx_map).unwrap();
        assert_eq!(restored, data.to_vec());
    }

    #[test]
    fn roundtrip_pure_ascii() {
        let data = b"the quick brown fox jumps over the lazy dog";
        let (channels, ctx_map) = context_split_encode(data);
        let restored = context_split_decode(&channels, &ctx_map).unwrap();
        assert_eq!(restored, data.to_vec());
    }

    #[test]
    fn roundtrip_empty() {
        let (channels, ctx_map) = context_split_encode(b"");
        assert!(channels.iter().all(|c| c.is_empty()));
        assert!(ctx_map.is_empty());
    }

    #[test]
    fn transform_node_roundtrip() {
        let t = ContextSplitTransform;
        let data = b"Hello World\x00\xFF\x01test\x80data123".repeat(20);
        let ctx = TransformContext {
            entropy_estimate: 5.0,
            ascii_ratio: 0.6,
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
    fn context_classes() {
        assert_eq!(context_class(0), 0);     // control
        assert_eq!(context_class(31), 0);    // control
        assert_eq!(context_class(32), 1);    // space
        assert_eq!(context_class(b'A'), 1);  // uppercase
        assert_eq!(context_class(b'a'), 2);  // lowercase
        assert_eq!(context_class(128), 3);   // high
        assert_eq!(context_class(255), 3);   // high
    }
}
