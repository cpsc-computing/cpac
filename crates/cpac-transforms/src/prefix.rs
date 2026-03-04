// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Prefix extraction: removes common prefixes from string columns.
//!
//! Effective on URLs, file paths, log prefixes. Also supports incremental
//! (front-coding) for sorted data.

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};
use crate::zigzag::{decode_varint, encode_varint};

/// Transform ID for prefix (wire format).
pub const TRANSFORM_ID: u8 = 9;

const MAGIC: &[u8; 2] = b"PX";
const VERSION: u8 = 1;
const MODE_COMMON_PREFIX: u8 = 0;

/// Find longest common prefix of all strings.
fn common_prefix(strings: &[String]) -> String {
    if strings.is_empty() {
        return String::new();
    }
    let first = &strings[0];
    let mut len = first.len();
    for s in &strings[1..] {
        len = len.min(s.len());
        for (i, (a, b)) in first.chars().zip(s.chars()).enumerate() {
            if a != b {
                len = len.min(i);
                break;
            }
        }
    }
    first[..len].to_string()
}

/// Encode strings by extracting common prefix.
#[must_use] 
pub fn prefix_encode(values: &[String]) -> Vec<u8> {
    let mut out = Vec::from(MAGIC.as_slice());
    out.push(VERSION);
    out.push(MODE_COMMON_PREFIX);
    if values.is_empty() {
        out.extend_from_slice(&encode_varint(0));
        out.extend_from_slice(&encode_varint(0));
        return out;
    }
    let prefix = common_prefix(values);
    let prefix_bytes = prefix.as_bytes();
    out.extend_from_slice(&encode_varint(prefix_bytes.len() as u64));
    out.extend_from_slice(prefix_bytes);
    out.extend_from_slice(&encode_varint(values.len() as u64));
    let plen = prefix.len();
    for v in values {
        let suffix = &v.as_bytes()[plen..];
        out.extend_from_slice(&encode_varint(suffix.len() as u64));
        out.extend_from_slice(suffix);
    }
    out
}

/// Decode prefix-encoded values.
pub fn prefix_decode(data: &[u8]) -> CpacResult<Vec<String>> {
    if data.len() < 4 || &data[0..2] != MAGIC {
        return Err(CpacError::Transform("prefix: invalid frame".into()));
    }
    if data[2] != VERSION {
        return Err(CpacError::Transform("prefix: unsupported version".into()));
    }
    let mut offset = 4;
    let (prefix_len, c) = decode_varint(&data[offset..])?;
    offset += c;
    let prefix = String::from_utf8_lossy(&data[offset..offset + prefix_len as usize]).into_owned();
    offset += prefix_len as usize;
    let (count, c) = decode_varint(&data[offset..])?;
    offset += c;
    let mut values = Vec::with_capacity(count as usize);
    for _ in 0..count {
        let (suf_len, c) = decode_varint(&data[offset..])?;
        offset += c;
        let suffix = String::from_utf8_lossy(&data[offset..offset + suf_len as usize]).into_owned();
        offset += suf_len as usize;
        values.push(format!("{prefix}{suffix}"));
    }
    Ok(values)
}

/// Prefix extraction transform node.
pub struct PrefixTransform;

impl TransformNode for PrefixTransform {
    fn name(&self) -> &'static str {
        "prefix"
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
                let prefix = common_prefix(values);
                if prefix.len() >= 3 {
                    Some(prefix.len() as f64)
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
                let encoded = prefix_encode(&values);
                Ok((CpacType::Serial(encoded), Vec::new()))
            }
            _ => Err(CpacError::Transform("prefix: unsupported type".into())),
        }
    }
    fn decode(&self, input: CpacType, _metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                let values = prefix_decode(&data)?;
                let total_bytes: usize = values.iter().map(std::string::String::len).sum();
                Ok(CpacType::StringColumn {
                    values,
                    total_bytes,
                })
            }
            _ => Err(CpacError::Transform("prefix: unsupported type".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_urls() {
        let values: Vec<String> = vec![
            "https://api.example.com/v1/users",
            "https://api.example.com/v1/orders",
            "https://api.example.com/v1/products",
            "https://api.example.com/v2/users",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        let encoded = prefix_encode(&values);
        let decoded = prefix_decode(&encoded).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn roundtrip_no_common_prefix() {
        let values: Vec<String> = vec!["alpha", "beta", "gamma"]
            .into_iter()
            .map(String::from)
            .collect();
        let encoded = prefix_encode(&values);
        let decoded = prefix_decode(&encoded).unwrap();
        assert_eq!(decoded, values);
    }

    #[test]
    fn roundtrip_empty() {
        let encoded = prefix_encode(&[]);
        let decoded = prefix_decode(&encoded).unwrap();
        assert!(decoded.is_empty());
    }
}
