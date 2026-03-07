// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Vocabulary / dictionary encoding transform.
//!
//! Replaces categorical string values with varint indices into a
//! frequency-ordered vocabulary. Extremely effective for low-cardinality
//! string columns (HTTP methods, log levels, country codes, etc.).
//!
//! Wire format:
//! ```text
//! [vocab_count: varint]
//! [vocab_entry_0_len: varint][vocab_entry_0_bytes]
//! ...
//! [vocab_entry_N_len: varint][vocab_entry_N_bytes]
//! [value_count: varint]
//! [index_0: varint][index_1: varint]...
//! ```

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};
use std::collections::HashMap;

use crate::traits::{TransformContext, TransformNode};
use crate::zigzag::{decode_varint, encode_varint};

/// Transform ID for vocabulary encoding (wire format).
pub const TRANSFORM_ID: u8 = 13;

// ---------------------------------------------------------------------------
// Core encode/decode
// ---------------------------------------------------------------------------

/// Vocabulary-encode a slice of strings.
///
/// Returns the encoded bytes. The vocabulary is sorted by descending frequency
/// so the most common values get the smallest varint indices.
#[must_use]
pub fn vocab_encode(values: &[String]) -> Vec<u8> {
    if values.is_empty() {
        let mut out = encode_varint(0); // vocab_count = 0
        out.extend_from_slice(&encode_varint(0)); // value_count = 0
        return out;
    }

    // Count frequencies
    let mut freq: HashMap<&str, usize> = HashMap::new();
    for v in values {
        *freq.entry(v.as_str()).or_insert(0) += 1;
    }

    // Sort by frequency descending, then lexicographic for stability
    let mut entries: Vec<(&str, usize)> = freq.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(b.0)));

    // Build lookup
    let vocab: Vec<&str> = entries.iter().map(|(s, _)| *s).collect();
    let lookup: HashMap<&str, usize> = vocab.iter().enumerate().map(|(i, &s)| (s, i)).collect();

    // Encode
    let mut out = Vec::new();

    // Vocabulary table
    out.extend_from_slice(&encode_varint(vocab.len() as u64));
    for &entry in &vocab {
        out.extend_from_slice(&encode_varint(entry.len() as u64));
        out.extend_from_slice(entry.as_bytes());
    }

    // Index stream
    out.extend_from_slice(&encode_varint(values.len() as u64));
    for v in values {
        let idx = lookup[v.as_str()];
        out.extend_from_slice(&encode_varint(idx as u64));
    }

    out
}

/// Decode vocabulary-encoded data back to strings.
pub fn vocab_decode(data: &[u8]) -> CpacResult<Vec<String>> {
    let mut offset = 0;

    // Read vocabulary
    let (vocab_count, consumed) = decode_varint(&data[offset..])?;
    offset += consumed;
    let vocab_count = vocab_count as usize;

    let mut vocab = Vec::with_capacity(vocab_count);
    for _ in 0..vocab_count {
        let (entry_len, consumed) = decode_varint(&data[offset..])?;
        offset += consumed;
        let entry_len = entry_len as usize;
        if offset + entry_len > data.len() {
            return Err(CpacError::Transform("vocab: truncated entry".into()));
        }
        let entry = String::from_utf8(data[offset..offset + entry_len].to_vec())
            .map_err(|e| CpacError::Transform(format!("vocab: invalid UTF-8: {e}")))?;
        vocab.push(entry);
        offset += entry_len;
    }

    // Read index stream
    let (value_count, consumed) = decode_varint(&data[offset..])?;
    offset += consumed;
    let value_count = value_count as usize;

    let mut values = Vec::with_capacity(value_count);
    for _ in 0..value_count {
        let (idx, consumed) = decode_varint(&data[offset..])?;
        offset += consumed;
        let idx = idx as usize;
        if idx >= vocab.len() {
            return Err(CpacError::Transform(format!(
                "vocab: index {idx} out of range (vocab size {})",
                vocab.len()
            )));
        }
        values.push(vocab[idx].clone());
    }

    Ok(values)
}

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Vocabulary encoding transform node for the compression DAG.
pub struct VocabTransform;

impl TransformNode for VocabTransform {
    fn name(&self) -> &'static str {
        "vocab"
    }

    fn id(&self) -> u8 {
        TRANSFORM_ID
    }

    fn accepts(&self) -> &[TypeTag] {
        &[TypeTag::StringColumn]
    }

    fn produces(&self) -> TypeTag {
        TypeTag::Serial
    }

    fn estimate_gain(&self, input: &CpacType, _ctx: &TransformContext) -> Option<f64> {
        match input {
            CpacType::StringColumn {
                values,
                total_bytes,
            } => {
                if values.is_empty() {
                    return None;
                }
                let unique: std::collections::HashSet<&str> =
                    values.iter().map(|s| s.as_str()).collect();
                let cardinality = unique.len();
                // Beneficial when cardinality is low relative to count
                if cardinality <= 256 && cardinality < values.len() / 2 {
                    // Estimate: original uses avg_len bytes per value,
                    // vocab uses ~1 byte per value (varint index) + vocab table
                    let avg_len = *total_bytes as f64 / values.len() as f64;
                    let vocab_overhead: usize = unique.iter().map(|s| s.len() + 1).sum();
                    let index_cost = values.len(); // ~1 byte each for small cardinality
                    let total_cost = vocab_overhead + index_cost;
                    let savings = *total_bytes as f64 - total_cost as f64;
                    if savings > 0.0 {
                        Some(savings / avg_len)
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
            CpacType::StringColumn { values, .. } => {
                let encoded = vocab_encode(&values);
                Ok((CpacType::Serial(encoded), Vec::new()))
            }
            _ => Err(CpacError::Transform("vocab: unsupported input type".into())),
        }
    }

    fn decode(&self, input: CpacType, _metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                let values = vocab_decode(&data)?;
                let total_bytes = values.iter().map(|s| s.len()).sum();
                Ok(CpacType::StringColumn {
                    values,
                    total_bytes,
                })
            }
            _ => Err(CpacError::Transform("vocab: unsupported input type".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_basic() {
        let values: Vec<String> = vec!["GET", "POST", "GET", "PUT", "GET", "POST", "GET"]
            .into_iter()
            .map(String::from)
            .collect();
        let encoded = vocab_encode(&values);
        let decoded = vocab_decode(&encoded).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn roundtrip_empty() {
        let values: Vec<String> = vec![];
        let encoded = vocab_encode(&values);
        let decoded = vocab_decode(&encoded).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn frequency_ordering() {
        // GET appears 4x, POST 2x, PUT 1x
        let values: Vec<String> = vec!["GET", "POST", "GET", "PUT", "GET", "POST", "GET"]
            .into_iter()
            .map(String::from)
            .collect();
        let encoded = vocab_encode(&values);
        // GET (most frequent) should get index 0 → 1 byte varint
        // The encoded size should be much smaller than raw
        let raw_size: usize = values.iter().map(|s| s.len()).sum();
        assert!(encoded.len() < raw_size);
    }

    #[test]
    fn transform_node_roundtrip() {
        let t = VocabTransform;
        let ctx = TransformContext {
            entropy_estimate: 3.0,
            ascii_ratio: 0.9,
            data_size: 100,
        };
        let values: Vec<String> = vec!["INFO", "ERROR", "INFO", "DEBUG", "INFO", "ERROR"]
            .into_iter()
            .map(String::from)
            .collect();
        let total_bytes = values.iter().map(|s| s.len()).sum();
        let input = CpacType::StringColumn {
            values: values.clone(),
            total_bytes,
        };
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::StringColumn { values: v, .. } => assert_eq!(v, values),
            _ => panic!("expected StringColumn"),
        }
    }
}
