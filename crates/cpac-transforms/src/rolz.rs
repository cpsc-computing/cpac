// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! ROLZ: Reduced Offset LZ Compression.
//!
//! Uses context-dependent match tables where each context (previous byte)
//! maintains its own ordered list of recent match positions. Matches are
//! encoded as `(bucket_index, length)` instead of `(absolute_offset, length)`,
//! using far fewer bits for offsets.
//!
//! Key properties:
//! - 256 context buckets × 64 entries each
//! - Matches referenced by bucket index (small), not absolute offset
//! - Better than flat LZ for medium-entropy data with local patterns

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for ROLZ encoding (wire format).
pub const TRANSFORM_ID: u8 = 4;

/// Entries per context bucket.
const BUCKET_SIZE: usize = 64;

/// Minimum match length in bytes.
const MIN_MATCH: usize = 3;

/// Max match length (capped at u8).
const MAX_MATCH: usize = 255;

/// Good-enough match length (stop searching).
const GOOD_MATCH: usize = 32;

// ---------------------------------------------------------------------------
// Core ROLZ encode/decode (matches Python rolz_encode / rolz_decode)
// ---------------------------------------------------------------------------

/// Get context from previous byte.
#[inline]
fn hash_context(data: &[u8], pos: usize) -> usize {
    if pos == 0 {
        0
    } else {
        data[pos - 1] as usize
    }
}

/// Ensure bucket doesn't exceed max size.
#[inline]
fn bucket_push(bucket: &mut Vec<usize>, pos: usize) {
    bucket.push(pos);
    if bucket.len() > BUCKET_SIZE {
        bucket.remove(0);
    }
}

/// Compress data using ROLZ.
///
/// Output format: `[original_len: 4 LE][token_count: 4 LE][tokens...]`
/// - Literal token: `[0][byte]`
/// - Match token:   `[1][bucket_idx][length]`
pub fn rolz_encode(data: &[u8]) -> Vec<u8> {
    let n = data.len();
    if n == 0 {
        return 0u32.to_le_bytes().to_vec();
    }

    // Per-context buckets
    let mut buckets: Vec<Vec<usize>> = (0..256).map(|_| Vec::new()).collect();

    // Collect tokens
    let mut tokens: Vec<Token> = Vec::new();
    let mut pos = 0;

    while pos < n {
        let ctx = hash_context(data, pos);
        let bucket = &buckets[ctx];

        let mut best_idx: usize = 0;
        let mut best_len: usize = 0;

        // Search bucket in reverse (most recent first)
        for (bi, &bpos) in bucket.iter().rev().enumerate() {
            if bpos >= pos {
                continue;
            }
            // Check match length
            let mut ml = 0;
            while pos + ml < n
                && bpos + ml < pos // no overlap
                && data[pos + ml] == data[bpos + ml]
                && ml < MAX_MATCH
            {
                ml += 1;
            }
            if ml >= MIN_MATCH && ml > best_len {
                best_len = ml;
                best_idx = bi;
                if ml >= GOOD_MATCH {
                    break;
                }
            }
        }

        if best_len >= MIN_MATCH {
            tokens.push(Token::Match {
                bucket_idx: best_idx as u8,
                length: best_len as u8,
            });
            // Insert all match positions into buckets
            for k in 0..best_len {
                let c = hash_context(data, pos + k);
                bucket_push(&mut buckets[c], pos + k);
            }
            pos += best_len;
        } else {
            tokens.push(Token::Literal(data[pos]));
            bucket_push(&mut buckets[ctx], pos);
            pos += 1;
        }
    }

    // Serialize
    let mut out = Vec::with_capacity(8 + tokens.len() * 3);
    out.extend_from_slice(&(n as u32).to_le_bytes());
    out.extend_from_slice(&(tokens.len() as u32).to_le_bytes());
    for tok in &tokens {
        match tok {
            Token::Literal(byte) => {
                out.push(0);
                out.push(*byte);
            }
            Token::Match { bucket_idx, length } => {
                out.push(1);
                out.push(*bucket_idx);
                out.push(*length);
            }
        }
    }
    out
}

/// Decompress ROLZ data.
pub fn rolz_decode(data: &[u8]) -> CpacResult<Vec<u8>> {
    if data.len() < 4 {
        return Err(CpacError::Transform(
            "rolz: insufficient data for header".into(),
        ));
    }
    let original_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
    if original_len == 0 {
        return Ok(Vec::new());
    }
    if data.len() < 8 {
        return Err(CpacError::Transform(
            "rolz: insufficient data for header".into(),
        ));
    }
    let token_count = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;

    let mut buckets: Vec<Vec<usize>> = (0..256).map(|_| Vec::new()).collect();
    let mut output = Vec::with_capacity(original_len);
    let mut offset = 8;

    for _ in 0..token_count {
        if offset >= data.len() {
            return Err(CpacError::Transform("rolz: truncated token stream".into()));
        }
        let flag = data[offset];
        offset += 1;

        match flag {
            0 => {
                // Literal
                if offset >= data.len() {
                    return Err(CpacError::Transform("rolz: truncated literal".into()));
                }
                let byte = data[offset];
                offset += 1;
                let pos = output.len();
                let ctx = if pos > 0 { output[pos - 1] as usize } else { 0 };
                bucket_push(&mut buckets[ctx], pos);
                output.push(byte);
            }
            1 => {
                // Match
                if offset + 1 >= data.len() {
                    return Err(CpacError::Transform("rolz: truncated match".into()));
                }
                let bucket_idx = data[offset] as usize;
                let match_len = data[offset + 1] as usize;
                offset += 2;

                let pos = output.len();
                let ctx = if pos > 0 { output[pos - 1] as usize } else { 0 };
                let bucket = &buckets[ctx];

                // Resolve bucket index (reversed search order)
                if bucket_idx >= bucket.len() {
                    return Err(CpacError::Transform(
                        "rolz: bucket index out of range".into(),
                    ));
                }
                let actual_idx = bucket.len() - 1 - bucket_idx;
                let src_pos = bucket[actual_idx];

                // Copy match and update buckets
                for k in 0..match_len {
                    let cur_pos = output.len();
                    let c = if cur_pos > 0 {
                        output[cur_pos - 1] as usize
                    } else {
                        0
                    };
                    bucket_push(&mut buckets[c], cur_pos);
                    output.push(output[src_pos + k]);
                }
            }
            _ => {
                return Err(CpacError::Transform(format!(
                    "rolz: unknown token flag: {flag}"
                )));
            }
        }
    }

    if output.len() != original_len {
        return Err(CpacError::Transform(format!(
            "rolz: size mismatch: expected {original_len}, got {}",
            output.len()
        )));
    }

    Ok(output)
}

// ---------------------------------------------------------------------------
// Internal token type
// ---------------------------------------------------------------------------

enum Token {
    Literal(u8),
    Match { bucket_idx: u8, length: u8 },
}

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// ROLZ transform node for the compression DAG.
pub struct RolzTransform;

impl TransformNode for RolzTransform {
    fn name(&self) -> &str {
        "rolz"
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
        // ROLZ works best on medium-entropy data with local patterns.
        let min_ent = if ctx.ascii_ratio > 0.85 { 4.5 } else { 3.0 };
        if ctx.entropy_estimate < min_ent || ctx.entropy_estimate > 7.0 || ctx.data_size < 512 {
            return None;
        }
        match input {
            CpacType::Serial(_) => Some(7.0 - ctx.entropy_estimate),
            _ => None,
        }
    }

    fn encode(&self, input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        match input {
            CpacType::Serial(data) => {
                let encoded = rolz_encode(&data);
                Ok((CpacType::Serial(encoded), Vec::new()))
            }
            _ => Err(CpacError::Transform("rolz: unsupported input type".into())),
        }
    }

    fn decode(&self, input: CpacType, _metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                let decoded = rolz_decode(&data)?;
                Ok(CpacType::Serial(decoded))
            }
            _ => Err(CpacError::Transform("rolz: unsupported input type".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_empty() {
        let encoded = rolz_encode(&[]);
        let decoded = rolz_decode(&encoded).unwrap();
        assert!(decoded.is_empty());
    }

    #[test]
    fn roundtrip_short() {
        let data = b"hello world!";
        let encoded = rolz_encode(data);
        let decoded = rolz_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn roundtrip_repetitive() {
        let data = b"abcabcabcabcabcabcabcabcabcabc".repeat(10);
        let encoded = rolz_encode(&data);
        let decoded = rolz_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
        // Repetitive data should compress
        assert!(encoded.len() < data.len());
    }

    #[test]
    fn roundtrip_binary() {
        let data: Vec<u8> = (0..1024).map(|i| (i % 50) as u8).collect();
        let encoded = rolz_encode(&data);
        let decoded = rolz_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn roundtrip_all_bytes() {
        let data: Vec<u8> = (0u8..=255).collect();
        let encoded = rolz_encode(&data);
        let decoded = rolz_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn decode_truncated_header() {
        assert!(rolz_decode(&[0, 0]).is_err());
    }

    #[test]
    fn transform_node_roundtrip() {
        let t = RolzTransform;
        let data = b"ROLZ test data with some repetition. ROLZ test data again.".repeat(5);
        let ctx = TransformContext {
            entropy_estimate: 4.0,
            ascii_ratio: 0.9,
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
