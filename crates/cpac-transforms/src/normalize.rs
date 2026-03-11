// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Normalization transform for structured text data.
//!
//! Strips optional whitespace, normalizes number formats, and canonicalizes
//! timestamps so the entropy coder sees a more uniform byte stream.
//!
//! Wire format (metadata):
//! `[original_len: 4 LE][mode: 1][diff_count: 4 LE][diffs...]`
//!
//! Where each diff is: `[offset: 4 LE][removed_len: 2 LE][removed_bytes...]`
//!
//! The diffs store what was removed/changed so reconstruction is lossless.

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for normalize (wire format).
pub const TRANSFORM_ID: u8 = 17;

/// Normalization mode flags.
const MODE_JSON_WS: u8 = 0x01;
const MODE_XML_WS: u8 = 0x02;

// ---------------------------------------------------------------------------
// Core encode/decode
// ---------------------------------------------------------------------------

/// A single diff recording what was removed at a given offset.
#[derive(Clone, Debug)]
struct NormDiff {
    /// Offset in the *original* stream where the removed bytes start.
    offset: u32,
    /// The removed bytes (whitespace, trailing zeros, etc.).
    removed: Vec<u8>,
}

/// Normalize a JSON-like byte stream by removing non-significant whitespace.
///
/// Returns `(normalized, diffs)` where diffs record every removal.
fn normalize_json_whitespace(data: &[u8]) -> (Vec<u8>, Vec<NormDiff>) {
    let mut out = Vec::with_capacity(data.len());
    let mut diffs = Vec::new();
    let mut i = 0;
    let mut in_string = false;
    let mut escape_next = false;

    while i < data.len() {
        let b = data[i];

        if escape_next {
            escape_next = false;
            out.push(b);
            i += 1;
            continue;
        }

        if b == b'\\' && in_string {
            escape_next = true;
            out.push(b);
            i += 1;
            continue;
        }

        if b == b'"' {
            in_string = !in_string;
            out.push(b);
            i += 1;
            continue;
        }

        if !in_string && (b == b' ' || b == b'\t') {
            // Check if this is non-significant whitespace (between tokens)
            let start = i;
            while i < data.len() && (data[i] == b' ' || data[i] == b'\t') {
                i += 1;
            }
            diffs.push(NormDiff {
                offset: start as u32,
                removed: data[start..i].to_vec(),
            });
            continue;
        }

        // Normalize newline-based pretty-printing: CRLF → nothing, LF → nothing
        // (only outside strings)
        if !in_string && (b == b'\n' || b == b'\r') {
            let start = i;
            while i < data.len() && (data[i] == b'\n' || data[i] == b'\r') {
                i += 1;
            }
            diffs.push(NormDiff {
                offset: start as u32,
                removed: data[start..i].to_vec(),
            });
            continue;
        }

        out.push(b);
        i += 1;
    }

    (out, diffs)
}

/// Normalize an XML byte stream by removing whitespace-only text nodes
/// (whitespace between `>` and `<`).  This targets pretty-printed XML where
/// indentation and blank lines between tags are redundant.
fn normalize_xml_whitespace(data: &[u8]) -> (Vec<u8>, Vec<NormDiff>) {
    let mut out = Vec::with_capacity(data.len());
    let mut diffs = Vec::new();
    let mut i = 0;

    while i < data.len() {
        // After a '>', scan for whitespace-only gap before '<'
        if data[i] == b'>' {
            out.push(b'>');
            i += 1;
            // Scan ahead for whitespace-only text node
            let ws_start = i;
            let mut j = i;
            while j < data.len()
                && (data[j] == b' ' || data[j] == b'\t' || data[j] == b'\n' || data[j] == b'\r')
            {
                j += 1;
            }
            // Only strip if the whitespace ends at '<' (pure inter-tag gap)
            if j > ws_start && j < data.len() && data[j] == b'<' {
                diffs.push(NormDiff {
                    offset: ws_start as u32,
                    removed: data[ws_start..j].to_vec(),
                });
                i = j; // skip the whitespace, next char is '<'
            }
            // else: whitespace followed by text content — keep it
        } else {
            out.push(data[i]);
            i += 1;
        }
    }

    (out, diffs)
}

/// Pick the best normalization mode for the input data.
///
/// Tries JSON whitespace stripping first (effective on JSON/YAML), then XML
/// whitespace stripping (effective on pretty-printed XML).  Returns the mode
/// that produces the largest reduction, or `None` if neither helps.
fn best_normalize_mode(data: &[u8]) -> Option<(Vec<u8>, Vec<NormDiff>, u8)> {
    let (json_norm, json_diffs) = normalize_json_whitespace(data);
    let json_savings = data.len().saturating_sub(json_norm.len());

    let (xml_norm, xml_diffs) = normalize_xml_whitespace(data);
    let xml_savings = data.len().saturating_sub(xml_norm.len());

    if json_savings == 0 && xml_savings == 0 {
        return None;
    }

    if json_savings >= xml_savings {
        Some((json_norm, json_diffs, MODE_JSON_WS))
    } else {
        Some((xml_norm, xml_diffs, MODE_XML_WS))
    }
}

/// Serialize diffs to metadata bytes.
fn encode_diffs(original_len: usize, mode: u8, diffs: &[NormDiff]) -> Vec<u8> {
    let mut meta = Vec::new();
    meta.extend_from_slice(&(original_len as u32).to_le_bytes());
    meta.push(mode);
    meta.extend_from_slice(&(diffs.len() as u32).to_le_bytes());
    for d in diffs {
        meta.extend_from_slice(&d.offset.to_le_bytes());
        meta.extend_from_slice(&(d.removed.len() as u16).to_le_bytes());
        meta.extend_from_slice(&d.removed);
    }
    meta
}

/// Deserialize diffs from metadata and reconstruct original data.
fn decode_diffs(normalized: &[u8], metadata: &[u8]) -> CpacResult<Vec<u8>> {
    if metadata.len() < 9 {
        return Err(CpacError::Transform("normalize: metadata too short".into()));
    }

    let original_len =
        u32::from_le_bytes([metadata[0], metadata[1], metadata[2], metadata[3]]) as usize;
    let _mode = metadata[4];
    let diff_count =
        u32::from_le_bytes([metadata[5], metadata[6], metadata[7], metadata[8]]) as usize;

    // Parse diffs
    let mut diffs = Vec::with_capacity(diff_count);
    let mut offset = 9;
    for _ in 0..diff_count {
        if offset + 6 > metadata.len() {
            return Err(CpacError::Transform("normalize: truncated diff".into()));
        }
        let orig_offset = u32::from_le_bytes([
            metadata[offset],
            metadata[offset + 1],
            metadata[offset + 2],
            metadata[offset + 3],
        ]) as usize;
        offset += 4;
        let removed_len = u16::from_le_bytes([metadata[offset], metadata[offset + 1]]) as usize;
        offset += 2;
        if offset + removed_len > metadata.len() {
            return Err(CpacError::Transform(
                "normalize: truncated diff payload".into(),
            ));
        }
        let removed = metadata[offset..offset + removed_len].to_vec();
        offset += removed_len;
        diffs.push(NormDiff {
            offset: orig_offset as u32,
            removed,
        });
    }

    // Reconstruct: walk through original positions, inserting removed bytes
    let mut out = Vec::with_capacity(original_len);
    let mut norm_pos = 0usize;
    let mut diff_idx = 0usize;

    let mut orig_pos = 0usize;
    while orig_pos < original_len {
        // Check if we need to insert a diff at this original position
        if diff_idx < diffs.len() && diffs[diff_idx].offset as usize == orig_pos {
            out.extend_from_slice(&diffs[diff_idx].removed);
            orig_pos += diffs[diff_idx].removed.len();
            diff_idx += 1;
        } else {
            if norm_pos < normalized.len() {
                out.push(normalized[norm_pos]);
                norm_pos += 1;
            }
            orig_pos += 1;
        }
    }

    Ok(out)
}

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Normalization transform node for the compression DAG.
pub struct NormalizeTransform;

impl TransformNode for NormalizeTransform {
    fn name(&self) -> &str {
        "normalize"
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
                if data.len() < 128 || ctx.ascii_ratio < 0.8 {
                    return None;
                }
                // Quick check: count whitespace that could be removed
                let ws_count = data
                    .iter()
                    .filter(|&&b| b == b' ' || b == b'\t' || b == b'\n' || b == b'\r')
                    .count();
                let ratio = ws_count as f64 / data.len() as f64;
                if ratio > 0.05 {
                    Some(ratio * data.len() as f64)
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
                let best = best_normalize_mode(&data);
                let (normalized, diffs, mode) = match best {
                    Some(b) => b,
                    None => return Ok((CpacType::Serial(data), Vec::new())),
                };
                let raw_savings = data.len().saturating_sub(normalized.len());
                if raw_savings == 0 {
                    return Ok((CpacType::Serial(data), Vec::new()));
                }
                // Estimate descriptor overhead before encoding:
                // each diff = 4B offset + 2B len + removed bytes.
                // Gate: if estimated descriptor > 50% of raw savings,
                // the transform is net-negative after entropy coding.
                let estimated_meta: usize =
                    9 + diffs.iter().map(|d| 6 + d.removed.len()).sum::<usize>();
                if estimated_meta > raw_savings / 2 {
                    return Ok((CpacType::Serial(data), Vec::new()));
                }
                let meta = encode_diffs(data.len(), mode, &diffs);
                // Guard: DAG descriptor uses u16 for per-step metadata length.
                if meta.len() > u16::MAX as usize {
                    return Ok((CpacType::Serial(data), Vec::new()));
                }
                Ok((CpacType::Serial(normalized), meta))
            }
            _ => Err(CpacError::Transform(
                "normalize: unsupported input type".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                if metadata.is_empty() {
                    // Passthrough — no normalization was applied
                    return Ok(CpacType::Serial(data));
                }
                let restored = decode_diffs(&data, metadata)?;
                Ok(CpacType::Serial(restored))
            }
            _ => Err(CpacError::Transform(
                "normalize: unsupported input type".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_json_whitespace() {
        let data = br#"{ "name" : "Alice" , "age" : 30 }"#;
        let (normalized, diffs) = normalize_json_whitespace(data);
        // Should have removed spaces around : and ,
        assert!(normalized.len() < data.len());
        let meta = encode_diffs(data.len(), MODE_JSON_WS, &diffs);
        let restored = decode_diffs(&normalized, &meta).unwrap();
        assert_eq!(restored, data.to_vec());
    }

    #[test]
    fn roundtrip_pretty_json() {
        let data = b"{\n  \"key\": \"value\",\n  \"num\": 42\n}";
        let (normalized, diffs) = normalize_json_whitespace(data);
        assert!(normalized.len() < data.len());
        let meta = encode_diffs(data.len(), MODE_JSON_WS, &diffs);
        let restored = decode_diffs(&normalized, &meta).unwrap();
        assert_eq!(restored, data.to_vec());
    }

    #[test]
    fn preserves_string_spaces() {
        let data = br#"{"msg":"hello world"}"#;
        let (normalized, _diffs) = normalize_json_whitespace(data);
        // Space inside string must be preserved
        assert_eq!(normalized, data.to_vec());
    }

    #[test]
    fn transform_node_roundtrip() {
        let t = NormalizeTransform;
        let data = b"{\n  \"name\": \"Alice\",\n  \"age\": 30\n}".to_vec();
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
    fn no_whitespace_passthrough() {
        let data = br#"{"a":1,"b":2}"#;
        let (normalized, diffs) = normalize_json_whitespace(data);
        assert_eq!(normalized.len(), data.len());
        assert!(diffs.is_empty());
    }

    #[test]
    fn roundtrip_crlf() {
        let data = b"{\r\n  \"key\": 1\r\n}";
        let (normalized, diffs) = normalize_json_whitespace(data);
        assert!(normalized.len() < data.len());
        let meta = encode_diffs(data.len(), MODE_JSON_WS, &diffs);
        let restored = decode_diffs(&normalized, &meta).unwrap();
        assert_eq!(restored, data.to_vec());
    }
}
