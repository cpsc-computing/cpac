// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Row-sorting transform for columnar/structured data.
//!
//! Sorts rows by the lowest-cardinality column so that identical or similar
//! rows cluster together, dramatically improving LZ77 match distances.
//!
//! Wire format (metadata):
//! `[row_count: 4 LE][sort_col: 2 LE][permutation: row_count × 4 LE indices]`
//!
//! The permutation stores the original row index for each position in the
//! sorted output, enabling exact reconstruction.

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for row_sort (wire format).
pub const TRANSFORM_ID: u8 = 16;

// ---------------------------------------------------------------------------
// Core encode/decode
// ---------------------------------------------------------------------------

/// Build the sort permutation for a `ColumnSet` by the lowest-cardinality column.
///
/// Returns `(sorted_column_set, metadata_bytes)`.
#[allow(clippy::type_complexity)]
pub fn row_sort_encode(columns: &[(String, CpacType)]) -> CpacResult<(Vec<(String, CpacType)>, Vec<u8>)> {
    if columns.is_empty() {
        return Ok((columns.to_vec(), Vec::new()));
    }

    // Determine row count from first column
    let row_count = column_len(&columns[0].1)?;
    if row_count == 0 {
        return Ok((columns.to_vec(), Vec::new()));
    }

    // Find lowest-cardinality column to use as sort key
    let (sort_col_idx, _) = columns
        .iter()
        .enumerate()
        .filter_map(|(i, (_, col))| {
            column_cardinality(col, row_count).map(|card| (i, card))
        })
        .min_by_key(|(_, card)| *card)
        .unwrap_or((0, row_count));

    // Build sort permutation based on sort key column
    let mut indices: Vec<u32> = (0..row_count as u32).collect();
    let sort_col = &columns[sort_col_idx].1;
    sort_indices_by_column(&mut indices, sort_col)?;

    // Apply permutation to all columns
    let sorted_columns: Vec<(String, CpacType)> = columns
        .iter()
        .map(|(name, col)| {
            let sorted = apply_permutation(col, &indices);
            (name.clone(), sorted)
        })
        .collect();

    // Encode metadata: [row_count: 4LE][sort_col: 2LE][permutation: N×4LE]
    let mut meta = Vec::with_capacity(6 + row_count * 4);
    meta.extend_from_slice(&(row_count as u32).to_le_bytes());
    meta.extend_from_slice(&(sort_col_idx as u16).to_le_bytes());
    for &idx in &indices {
        meta.extend_from_slice(&idx.to_le_bytes());
    }

    Ok((sorted_columns, meta))
}

/// Reverse the row sort using the stored permutation.
pub fn row_sort_decode(
    columns: &[(String, CpacType)],
    metadata: &[u8],
) -> CpacResult<Vec<(String, CpacType)>> {
    if metadata.len() < 6 {
        return Ok(columns.to_vec());
    }

    let row_count = u32::from_le_bytes([metadata[0], metadata[1], metadata[2], metadata[3]]) as usize;
    let _sort_col = u16::from_le_bytes([metadata[4], metadata[5]]) as usize;

    if metadata.len() < 6 + row_count * 4 {
        return Err(CpacError::Transform("row_sort: truncated permutation".into()));
    }

    // Read permutation
    let mut perm = Vec::with_capacity(row_count);
    for i in 0..row_count {
        let off = 6 + i * 4;
        let idx = u32::from_le_bytes([
            metadata[off],
            metadata[off + 1],
            metadata[off + 2],
            metadata[off + 3],
        ]) as usize;
        perm.push(idx);
    }

    // Build inverse permutation: inverse[perm[i]] = i
    let mut inverse = vec![0usize; row_count];
    for (sorted_pos, &orig_pos) in perm.iter().enumerate() {
        if orig_pos < row_count {
            inverse[orig_pos] = sorted_pos;
        }
    }

    // Apply inverse permutation to restore original order
    let restored: Vec<(String, CpacType)> = columns
        .iter()
        .map(|(name, col)| {
            let inv_perm: Vec<u32> = inverse.iter().map(|&i| i as u32).collect();
            let restored_col = apply_permutation(col, &inv_perm);
            (name.clone(), restored_col)
        })
        .collect();

    Ok(restored)
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn column_len(col: &CpacType) -> CpacResult<usize> {
    match col {
        CpacType::IntColumn { values, .. } => Ok(values.len()),
        CpacType::FloatColumn { values, .. } => Ok(values.len()),
        CpacType::StringColumn { values, .. } => Ok(values.len()),
        _ => Err(CpacError::Transform("row_sort: unsupported column type".into())),
    }
}

fn column_cardinality(col: &CpacType, _row_count: usize) -> Option<usize> {
    match col {
        CpacType::IntColumn { values, .. } => {
            let unique: std::collections::HashSet<i64> = values.iter().copied().collect();
            Some(unique.len())
        }
        CpacType::StringColumn { values, .. } => {
            let unique: std::collections::HashSet<&str> =
                values.iter().map(String::as_str).collect();
            Some(unique.len())
        }
        _ => None,
    }
}

fn sort_indices_by_column(indices: &mut [u32], col: &CpacType) -> CpacResult<()> {
    match col {
        CpacType::IntColumn { values, .. } => {
            indices.sort_by_key(|&i| values[i as usize]);
        }
        CpacType::StringColumn { values, .. } => {
            indices.sort_by(|&a, &b| values[a as usize].cmp(&values[b as usize]));
        }
        CpacType::FloatColumn { values, .. } => {
            indices.sort_by(|&a, &b| {
                values[a as usize]
                    .partial_cmp(&values[b as usize])
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }
        _ => {
            return Err(CpacError::Transform(
                "row_sort: cannot sort by this column type".into(),
            ))
        }
    }
    Ok(())
}

fn apply_permutation(col: &CpacType, perm: &[u32]) -> CpacType {
    match col {
        CpacType::IntColumn {
            values,
            original_width,
        } => CpacType::IntColumn {
            values: perm.iter().map(|&i| values[i as usize]).collect(),
            original_width: *original_width,
        },
        CpacType::FloatColumn { values, precision } => CpacType::FloatColumn {
            values: perm.iter().map(|&i| values[i as usize]).collect(),
            precision: *precision,
        },
        CpacType::StringColumn { values, total_bytes } => {
            let new_vals: Vec<String> = perm
                .iter()
                .map(|&i| values[i as usize].clone())
                .collect();
            CpacType::StringColumn {
                values: new_vals,
                total_bytes: *total_bytes,
            }
        }
        other => other.clone(),
    }
}

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Row-sort transform node for the compression DAG.
pub struct RowSortTransform;

impl TransformNode for RowSortTransform {
    fn name(&self) -> &str {
        "row_sort"
    }

    fn id(&self) -> u8 {
        TRANSFORM_ID
    }

    fn accepts(&self) -> &[TypeTag] {
        &[TypeTag::ColumnSet]
    }

    fn produces(&self) -> TypeTag {
        TypeTag::ColumnSet
    }

    fn estimate_gain(&self, input: &CpacType, _ctx: &TransformContext) -> Option<f64> {
        match input {
            CpacType::ColumnSet { columns } => {
                if columns.is_empty() {
                    return None;
                }
                let row_count = column_len(&columns[0].1).ok()?;
                if row_count < 10 {
                    return None;
                }
                // Estimate: lower cardinality ratio → more clustering benefit
                let min_card = columns
                    .iter()
                    .filter_map(|(_, col)| column_cardinality(col, row_count))
                    .min()
                    .unwrap_or(row_count);
                let ratio = min_card as f64 / row_count as f64;
                if ratio < 0.5 {
                    Some((1.0 - ratio) * 10.0)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    fn encode(&self, input: CpacType, _ctx: &TransformContext) -> CpacResult<(CpacType, Vec<u8>)> {
        match input {
            CpacType::ColumnSet { columns } => {
                let (sorted, meta) = row_sort_encode(&columns)?;
                Ok((CpacType::ColumnSet { columns: sorted }, meta))
            }
            _ => Err(CpacError::Transform(
                "row_sort: expected ColumnSet input".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::ColumnSet { columns } => {
                let restored = row_sort_decode(&columns, metadata)?;
                Ok(CpacType::ColumnSet { columns: restored })
            }
            _ => Err(CpacError::Transform(
                "row_sort: expected ColumnSet input".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpac_types::FloatPrecision;

    #[test]
    fn roundtrip_int_columns() {
        let columns = vec![
            (
                "id".to_string(),
                CpacType::IntColumn {
                    values: vec![3, 1, 2, 1, 3, 2],
                    original_width: 4,
                },
            ),
            (
                "value".to_string(),
                CpacType::IntColumn {
                    values: vec![30, 10, 20, 11, 31, 21],
                    original_width: 8,
                },
            ),
        ];

        let (sorted, meta) = row_sort_encode(&columns).unwrap();
        // Verify sorted by lowest-cardinality column
        if let CpacType::IntColumn { values, .. } = &sorted[0].1 {
            // Should be sorted: 1, 1, 2, 2, 3, 3
            assert!(values.windows(2).all(|w| w[0] <= w[1]));
        }

        let restored = row_sort_decode(&sorted, &meta).unwrap();
        if let (CpacType::IntColumn { values: orig, .. }, CpacType::IntColumn { values: rest, .. }) =
            (&columns[1].1, &restored[1].1)
        {
            assert_eq!(orig, rest, "value column should roundtrip exactly");
        }
    }

    #[test]
    fn roundtrip_mixed_columns() {
        let columns = vec![
            (
                "category".to_string(),
                CpacType::StringColumn {
                    values: vec!["B", "A", "C", "A", "B", "C"]
                        .into_iter()
                        .map(String::from)
                        .collect(),
                    total_bytes: 6,
                },
            ),
            (
                "score".to_string(),
                CpacType::FloatColumn {
                    values: vec![2.0, 1.0, 3.0, 1.5, 2.5, 3.5],
                    precision: FloatPrecision::F64,
                },
            ),
        ];

        let (sorted, meta) = row_sort_encode(&columns).unwrap();
        let restored = row_sort_decode(&sorted, &meta).unwrap();

        if let (
            CpacType::StringColumn { values: orig, .. },
            CpacType::StringColumn { values: rest, .. },
        ) = (&columns[0].1, &restored[0].1)
        {
            assert_eq!(orig, rest);
        }
    }

    #[test]
    fn transform_node_roundtrip() {
        let t = RowSortTransform;
        let columns = vec![
            (
                "key".to_string(),
                CpacType::IntColumn {
                    values: vec![2, 1, 3, 1, 2, 3, 1, 2, 3, 1],
                    original_width: 4,
                },
            ),
            (
                "data".to_string(),
                CpacType::IntColumn {
                    values: vec![20, 10, 30, 11, 21, 31, 12, 22, 32, 13],
                    original_width: 8,
                },
            ),
        ];
        let ctx = TransformContext {
            entropy_estimate: 3.0,
            ascii_ratio: 0.0,
            data_size: 80,
        };
        let input = CpacType::ColumnSet {
            columns: columns.clone(),
        };
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        let decoded = t.decode(encoded, &meta).unwrap();

        match decoded {
            CpacType::ColumnSet { columns: restored } => {
                if let (
                    CpacType::IntColumn { values: orig, .. },
                    CpacType::IntColumn { values: rest, .. },
                ) = (&columns[1].1, &restored[1].1)
                {
                    assert_eq!(orig, rest, "data column roundtrip");
                }
            }
            _ => panic!("expected ColumnSet"),
        }
    }

    #[test]
    fn empty_columns() {
        let columns: Vec<(String, CpacType)> = vec![];
        let (sorted, meta) = row_sort_encode(&columns).unwrap();
        assert!(sorted.is_empty());
        assert!(meta.is_empty());
    }
}
