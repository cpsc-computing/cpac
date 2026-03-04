// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Column dedup: stores identical columns once with group indices.

use std::collections::HashMap;

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};
use crate::zigzag::{decode_varint, encode_varint};

/// Transform ID for dedup (wire format).
pub const TRANSFORM_ID: u8 = 10;

const MAGIC: &[u8; 2] = b"DD";
const VERSION: u8 = 1;

/// Deduplicate a list of column byte slices.
///
/// Returns `(encoded, had_duplicates)`.
#[must_use] 
pub fn dedup_columns(columns: &[Vec<u8>]) -> (Vec<u8>, bool) {
    let mut out = Vec::from(MAGIC.as_slice());
    out.push(VERSION);
    if columns.is_empty() {
        out.extend_from_slice(&encode_varint(0));
        return (out, false);
    }
    // Group by content hash (using full bytes as key for simplicity)
    let mut groups: HashMap<&[u8], Vec<usize>> = HashMap::new();
    let mut order: Vec<&[u8]> = Vec::new();
    for (i, col) in columns.iter().enumerate() {
        let key: &[u8] = col.as_slice();
        groups.entry(key).or_insert_with(|| {
            order.push(key);
            Vec::new()
        });
        groups.get_mut(key).unwrap().push(i);
    }
    let has_dups = groups.values().any(|v| v.len() > 1);
    out.extend_from_slice(&encode_varint(order.len() as u64));
    for key in &order {
        let indices = &groups[key];
        out.extend_from_slice(&encode_varint(indices.len() as u64));
        for &idx in indices {
            out.extend_from_slice(&encode_varint(idx as u64));
        }
        out.extend_from_slice(&(key.len() as u32).to_le_bytes());
        out.extend_from_slice(key);
    }
    (out, has_dups)
}

/// Decode deduped columns. Returns list of `(indices, data)` groups.
pub fn dedup_columns_decode(data: &[u8]) -> CpacResult<Vec<(Vec<usize>, Vec<u8>)>> {
    if data.len() < 3 || &data[0..2] != MAGIC {
        return Err(CpacError::Transform("dedup: invalid frame".into()));
    }
    if data[2] != VERSION {
        return Err(CpacError::Transform("dedup: unsupported version".into()));
    }
    let mut offset = 3;
    let (num_groups, c) = decode_varint(&data[offset..])?;
    offset += c;
    let mut groups = Vec::with_capacity(num_groups as usize);
    for _ in 0..num_groups {
        let (group_size, c) = decode_varint(&data[offset..])?;
        offset += c;
        let mut indices = Vec::with_capacity(group_size as usize);
        for _ in 0..group_size {
            let (idx, c) = decode_varint(&data[offset..])?;
            offset += c;
            indices.push(idx as usize);
        }
        if offset + 4 > data.len() {
            return Err(CpacError::Transform("dedup: truncated data".into()));
        }
        let data_len = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;
        let col_data = data[offset..offset + data_len].to_vec();
        offset += data_len;
        groups.push((indices, col_data));
    }
    Ok(groups)
}

/// Reconstruct full column list from deduped groups.
#[must_use] 
pub fn reconstruct_columns(groups: &[(Vec<usize>, Vec<u8>)], num_columns: usize) -> Vec<Vec<u8>> {
    let mut result: Vec<Option<Vec<u8>>> = vec![None; num_columns];
    for (indices, col_data) in groups {
        for &idx in indices {
            if idx < num_columns {
                result[idx] = Some(col_data.clone());
            }
        }
    }
    result.into_iter().map(std::option::Option::unwrap_or_default).collect()
}

/// Dedup transform node.
pub struct DedupTransform;

impl TransformNode for DedupTransform {
    fn name(&self) -> &'static str {
        "dedup"
    }
    fn id(&self) -> u8 {
        TRANSFORM_ID
    }
    fn accepts(&self) -> &[TypeTag] {
        &[TypeTag::ColumnSet]
    }
    fn produces(&self) -> TypeTag {
        TypeTag::Serial
    }
    fn estimate_gain(&self, _input: &CpacType, _ctx: &TransformContext) -> Option<f64> {
        // Gain estimation requires inspecting ColumnSet — defer to DAG
        Some(1.0)
    }
    fn encode(&self, _input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        // Dedup operates on column sets — Phase 3+ DAG usage
        Err(CpacError::Transform(
            "dedup: requires ColumnSet (DAG-level)".into(),
        ))
    }
    fn decode(&self, input: CpacType, _metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::Serial(data) => {
                let groups = dedup_columns_decode(&data)?;
                let max_idx = groups
                    .iter()
                    .flat_map(|(indices, _)| indices.iter())
                    .copied()
                    .max()
                    .unwrap_or(0);
                let columns = reconstruct_columns(&groups, max_idx + 1);
                Ok(CpacType::ColumnSet {
                    columns: columns
                        .into_iter()
                        .enumerate()
                        .map(|(i, c)| (format!("col_{i}"), CpacType::Serial(c)))
                        .collect(),
                })
            }
            _ => Err(CpacError::Transform("dedup: unsupported type".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_with_dups() {
        let cols = vec![
            vec![1u8, 2, 3],
            vec![4, 5, 6],
            vec![1, 2, 3], // duplicate of col 0
            vec![7, 8, 9],
        ];
        let (encoded, has_dups) = dedup_columns(&cols);
        assert!(has_dups);
        let groups = dedup_columns_decode(&encoded).unwrap();
        let restored = reconstruct_columns(&groups, 4);
        assert_eq!(restored, cols);
    }

    #[test]
    fn no_dups() {
        let cols = vec![vec![1u8, 2], vec![3, 4], vec![5, 6]];
        let (encoded, has_dups) = dedup_columns(&cols);
        assert!(!has_dups);
        let groups = dedup_columns_decode(&encoded).unwrap();
        let restored = reconstruct_columns(&groups, 3);
        assert_eq!(restored, cols);
    }

    #[test]
    fn empty_columns() {
        let (encoded, _) = dedup_columns(&[]);
        let groups = dedup_columns_decode(&encoded).unwrap();
        assert!(groups.is_empty());
    }
}
