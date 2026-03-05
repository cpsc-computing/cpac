// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Tokenize: per-column dictionary encoding for categorical strings.
//!
//! Builds a frequency-ordered dictionary, replaces each value with a
//! compact varint index. Most frequent values get the smallest indices.

use std::collections::HashMap;

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};
use crate::zigzag::{decode_varint, encode_varint};

/// Transform ID for tokenize (wire format).
pub const TRANSFORM_ID: u8 = 8;

const MAGIC: &[u8; 2] = b"TK";
const VERSION: u8 = 1;

/// Encode a list of strings using dictionary tokenization.
#[must_use]
pub fn tokenize_encode(values: &[String]) -> Vec<u8> {
    if values.is_empty() {
        let mut out = Vec::from(MAGIC.as_slice());
        out.push(VERSION);
        out.extend_from_slice(&encode_varint(0));
        out.extend_from_slice(&encode_varint(0));
        return out;
    }
    // Count frequencies
    let mut freq: HashMap<&str, usize> = HashMap::new();
    for v in values {
        *freq.entry(v.as_str()).or_insert(0) += 1;
    }
    // Sort by frequency descending
    let mut sorted_vals: Vec<&str> = freq.keys().copied().collect();
    sorted_vals.sort_by(|a, b| freq[b].cmp(&freq[a]).then_with(|| a.cmp(b)));
    let val_to_idx: HashMap<&str, usize> = sorted_vals
        .iter()
        .enumerate()
        .map(|(i, &v)| (v, i))
        .collect();

    let mut out = Vec::from(MAGIC.as_slice());
    out.push(VERSION);
    // Dictionary
    out.extend_from_slice(&encode_varint(sorted_vals.len() as u64));
    for &v in &sorted_vals {
        let vb = v.as_bytes();
        out.extend_from_slice(&encode_varint(vb.len() as u64));
        out.extend_from_slice(vb);
    }
    // Indices
    out.extend_from_slice(&encode_varint(values.len() as u64));
    for v in values {
        out.extend_from_slice(&encode_varint(val_to_idx[v.as_str()] as u64));
    }
    out
}

/// Decode tokenized values.
pub fn tokenize_decode(data: &[u8]) -> CpacResult<Vec<String>> {
    if data.len() < 3 || &data[0..2] != MAGIC {
        return Err(CpacError::Transform("tokenize: invalid frame".into()));
    }
    if data[2] != VERSION {
        return Err(CpacError::Transform("tokenize: unsupported version".into()));
    }
    let mut offset = 3;
    let (dict_count, consumed) = decode_varint(&data[offset..])?;
    offset += consumed;
    let mut dictionary = Vec::with_capacity(dict_count as usize);
    for _ in 0..dict_count {
        let (str_len, c) = decode_varint(&data[offset..])?;
        offset += c;
        let s = String::from_utf8_lossy(&data[offset..offset + str_len as usize]).into_owned();
        offset += str_len as usize;
        dictionary.push(s);
    }
    let (index_count, c) = decode_varint(&data[offset..])?;
    offset += c;
    let mut values = Vec::with_capacity(index_count as usize);
    for _ in 0..index_count {
        let (idx, c) = decode_varint(&data[offset..])?;
        offset += c;
        values.push(dictionary[idx as usize].clone());
    }
    Ok(values)
}

/// Tokenize transform node.
pub struct TokenizeTransform;

impl TransformNode for TokenizeTransform {
    fn name(&self) -> &'static str {
        "tokenize"
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
            CpacType::StringColumn { values, .. } => {
                if values.len() < 4 {
                    return None;
                }
                let unique: std::collections::HashSet<&str> =
                    values.iter().map(std::string::String::as_str).collect();
                let ratio = unique.len() as f64 / values.len() as f64;
                if ratio < 0.8 {
                    Some((1.0 - ratio) * 5.0)
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
                let encoded = tokenize_encode(&values);
                Ok((CpacType::Serial(encoded), Vec::new()))
            }
            _ => Err(CpacError::Transform("tokenize: unsupported type".into())),
        }
    }
    fn decode(&self, input: CpacType, _metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                let values = tokenize_decode(&data)?;
                let total_bytes: usize = values.iter().map(std::string::String::len).sum();
                Ok(CpacType::StringColumn {
                    values,
                    total_bytes,
                })
            }
            _ => Err(CpacError::Transform("tokenize: unsupported type".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_categorical() {
        let values: Vec<String> = vec![
            "GET", "POST", "GET", "GET", "PUT", "POST", "GET", "DELETE", "GET", "POST",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let encoded = tokenize_encode(&values);
        let decoded = tokenize_decode(&encoded).unwrap();
        assert_eq!(decoded, values);
        // Dictionary encoding should be smaller than raw
        let raw_size: usize = values.iter().map(|v| v.len() + 1).sum();
        assert!(encoded.len() < raw_size);
    }

    #[test]
    fn roundtrip_empty() {
        let encoded = tokenize_encode(&[]);
        let decoded = tokenize_decode(&encoded).unwrap();
        assert!(decoded.is_empty());
    }
}
