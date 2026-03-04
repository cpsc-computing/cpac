// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Field LZ: Structure-aware LZ compression at field boundaries.
//!
//! Matches occur at fixed-width element boundaries instead of arbitrary
//! byte positions, producing better results on structured binary data.

use std::collections::HashMap;

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for field LZ (wire format).
pub const TRANSFORM_ID: u8 = 3;

const FLAG_LITERAL: u8 = 0;
const FLAG_MATCH: u8 = 1;

/// Compress data using field-aligned LZ matching.
pub fn field_lz_encode(data: &[u8], field_width: usize) -> CpacResult<Vec<u8>> {
    let n = data.len();
    if n == 0 {
        let mut header = vec![field_width as u8];
        header.extend_from_slice(&0u16.to_le_bytes());
        header.extend_from_slice(&0u32.to_le_bytes());
        return Ok(header);
    }
    if !n.is_multiple_of(field_width) {
        return Err(CpacError::Transform(format!(
            "field_lz: data length {n} not divisible by field_width {field_width}"
        )));
    }
    let num_fields = n / field_width;
    let mut hash_table: HashMap<&[u8], usize> = HashMap::new();
    let mut tokens: Vec<Vec<u8>> = Vec::new();
    let mut literals: Vec<&[u8]> = Vec::new();
    let mut pos = 0;

    while pos < num_fields {
        let field = &data[pos * field_width..(pos + 1) * field_width];
        let prev_pos = hash_table.get(field).copied();

        if let Some(pp) = prev_pos {
            if pos - pp <= 65535 {
                let match_offset = pos - pp;
                let mut match_len = 1usize;
                while pos + match_len < num_fields {
                    let cur =
                        &data[(pos + match_len) * field_width..(pos + match_len + 1) * field_width];
                    let src_idx = pp + (match_len % match_offset);
                    let src = &data[src_idx * field_width..(src_idx + 1) * field_width];
                    if cur != src {
                        break;
                    }
                    match_len += 1;
                }
                // Flush literals
                if !literals.is_empty() {
                    let mut tok = vec![FLAG_LITERAL];
                    tok.extend_from_slice(&(literals.len() as u16).to_le_bytes());
                    for lit in &literals {
                        tok.extend_from_slice(lit);
                    }
                    tokens.push(tok);
                    literals.clear();
                }
                let mut tok = vec![FLAG_MATCH];
                tok.extend_from_slice(&(match_offset as u16).to_le_bytes());
                tok.extend_from_slice(&(match_len as u16).to_le_bytes());
                tokens.push(tok);
                for k in 0..match_len {
                    let fb = &data[(pos + k) * field_width..(pos + k + 1) * field_width];
                    hash_table.insert(fb, pos + k);
                }
                pos += match_len;
                continue;
            }
        }
        hash_table.insert(field, pos);
        literals.push(field);
        pos += 1;
    }
    if !literals.is_empty() {
        let mut tok = vec![FLAG_LITERAL];
        tok.extend_from_slice(&(literals.len() as u16).to_le_bytes());
        for lit in &literals {
            tok.extend_from_slice(lit);
        }
        tokens.push(tok);
    }
    let payload: Vec<u8> = tokens.into_iter().flatten().collect();
    let mut out = Vec::with_capacity(7 + payload.len());
    out.push(field_width as u8);
    out.extend_from_slice(&(num_fields as u16).to_le_bytes());
    out.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    out.extend_from_slice(&payload);
    Ok(out)
}

/// Decompress field LZ data.
pub fn field_lz_decode(data: &[u8]) -> CpacResult<Vec<u8>> {
    if data.len() < 7 {
        return Err(CpacError::Transform("field_lz: insufficient header".into()));
    }
    let field_width = data[0] as usize;
    let _num_fields = u16::from_le_bytes([data[1], data[2]]) as usize;
    let payload_len = u32::from_le_bytes([data[3], data[4], data[5], data[6]]) as usize;
    let end = 7 + payload_len;
    if data.len() < end {
        return Err(CpacError::Transform("field_lz: truncated payload".into()));
    }
    let mut output = Vec::new();
    let mut offset = 7;
    while offset < end {
        let flag = data[offset];
        offset += 1;
        if flag == FLAG_LITERAL {
            let count = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
            offset += 2;
            let lit_bytes = count * field_width;
            output.extend_from_slice(&data[offset..offset + lit_bytes]);
            offset += lit_bytes;
        } else if flag == FLAG_MATCH {
            let match_offset = u16::from_le_bytes([data[offset], data[offset + 1]]) as usize;
            let match_len = u16::from_le_bytes([data[offset + 2], data[offset + 3]]) as usize;
            offset += 4;
            let current_field = output.len() / field_width;
            let mut src_start = (current_field - match_offset) * field_width;
            for _ in 0..match_len {
                let chunk = output[src_start..src_start + field_width].to_vec();
                output.extend_from_slice(&chunk);
                src_start += field_width;
            }
        } else {
            return Err(CpacError::Transform(format!(
                "field_lz: unknown flag: {flag}"
            )));
        }
    }
    Ok(output)
}

/// Detect repeating field patterns (for preprocess heuristic).
#[must_use] 
pub fn detect_repeating_fields(data: &[u8], width: usize) -> f64 {
    let n = data.len();
    if n < width * 8 {
        return 0.0;
    }
    let num_fields = (n / width).min(1024);
    let mut seen: HashMap<&[u8], bool> = HashMap::new();
    let mut repeats = 0;
    for i in 0..num_fields {
        let field = &data[i * width..(i + 1) * width];
        if seen.contains_key(field) {
            repeats += 1;
        }
        seen.insert(field, true);
    }
    f64::from(repeats) / num_fields as f64
}

/// Field LZ transform node.
pub struct FieldLzTransform;

impl TransformNode for FieldLzTransform {
    fn name(&self) -> &'static str {
        "field_lz"
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
            CpacType::Serial(data) => {
                for &fw in &[4usize, 8, 2] {
                    if data.len() >= fw * 8 && data.len().is_multiple_of(fw) {
                        let score = detect_repeating_fields(data, fw);
                        if score > 0.3 {
                            return Some(score * 5.0);
                        }
                    }
                }
                None
            }
            _ => None,
        }
    }
    fn encode(&self, input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        match input {
            CpacType::Serial(data) => {
                // Auto-detect best field width
                let fw = [4usize, 8, 2]
                    .iter()
                    .copied()
                    .filter(|&w| data.len().is_multiple_of(w) && data.len() >= w * 8)
                    .max_by(|&a, &b| {
                        detect_repeating_fields(&data, a)
                            .partial_cmp(&detect_repeating_fields(&data, b))
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                    .unwrap_or(4);
                let encoded = field_lz_encode(&data, fw)?;
                Ok((CpacType::Serial(encoded), Vec::new()))
            }
            _ => Err(CpacError::Transform("field_lz: unsupported type".into())),
        }
    }
    fn decode(&self, input: CpacType, _metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                let decoded = field_lz_decode(&data)?;
                Ok(CpacType::Serial(decoded))
            }
            _ => Err(CpacError::Transform("field_lz: unsupported type".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_repeating_fields() {
        // 4-byte fields with repeating patterns
        let mut data = Vec::new();
        for _ in 0..50 {
            data.extend_from_slice(&[0x01, 0x02, 0x03, 0x04]);
            data.extend_from_slice(&[0x05, 0x06, 0x07, 0x08]);
        }
        let encoded = field_lz_encode(&data, 4).unwrap();
        let decoded = field_lz_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
        assert!(encoded.len() < data.len());
    }

    #[test]
    fn roundtrip_no_matches() {
        let data: Vec<u8> = (0..40).collect(); // all unique 4-byte fields
        let encoded = field_lz_encode(&data, 4).unwrap();
        let decoded = field_lz_decode(&encoded).unwrap();
        assert_eq!(decoded, data);
    }

    #[test]
    fn roundtrip_empty() {
        let encoded = field_lz_encode(&[], 4).unwrap();
        let decoded = field_lz_decode(&encoded).unwrap();
        assert!(decoded.is_empty());
    }
}
