// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Constraint projection transform.
//!
//! Analyzes a `ColumnSet` to classify integer columns as Fixed (constant),
//! Derived (linear function of another column), or Free (independent).
//! Eliminates Fixed and Derived columns, storing derivation rules in
//! metadata for lossless reconstruction on decode.
//!
//! Wire format (metadata):
//! `[version:1][original_col_count:2 LE][row_count:4 LE]`
//! Per original column:
//!   `[name_len:2 LE][name bytes][class_tag:1]`
//!   Fixed:         `[value:8 LE][original_width:1]`
//!   DerivedLinear: `[source_col:2 LE][multiplier:8 LE][offset:8 LE][original_width:1]`
//!   Free:          (nothing extra)

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for projection (wire format).
pub const TRANSFORM_ID: u8 = 25;

/// Minimum number of integer columns to attempt projection.
const MIN_INT_COLS: usize = 2;

// ---------------------------------------------------------------------------
// TransformNode implementation
// ---------------------------------------------------------------------------

/// Constraint-projection transform node.
pub struct ProjectionTransform;

impl TransformNode for ProjectionTransform {
    fn name(&self) -> &str {
        "projection"
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
                let int_data = extract_int_data(columns);
                if int_data.len() < MIN_INT_COLS {
                    return None;
                }
                let classes = cpac_cas::classify_variables(&int_data);
                let eliminable = classes
                    .iter()
                    .filter(|(_, c)| !matches!(c, cpac_cas::VarClass::Free))
                    .count();
                if eliminable > 0 {
                    Some(eliminable as f64 * 3.0)
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
                // Collect integer column data and their original indices
                let int_col_map: Vec<(usize, String, Vec<i64>)> = columns
                    .iter()
                    .enumerate()
                    .filter_map(|(i, (name, ct))| match ct {
                        CpacType::IntColumn { values, .. } => {
                            Some((i, name.clone(), values.clone()))
                        }
                        _ => None,
                    })
                    .collect();

                if int_col_map.len() < MIN_INT_COLS {
                    return Ok((CpacType::ColumnSet { columns }, Vec::new()));
                }

                let int_data: Vec<(String, Vec<i64>)> = int_col_map
                    .iter()
                    .map(|(_, name, vals)| (name.clone(), vals.clone()))
                    .collect();

                let classifications = cpac_cas::classify_variables(&int_data);

                let eliminable = classifications
                    .iter()
                    .filter(|(_, c)| !matches!(c, cpac_cas::VarClass::Free))
                    .count();
                if eliminable == 0 {
                    return Ok((CpacType::ColumnSet { columns }, Vec::new()));
                }

                // Build class map indexed by original column position.
                // Non-int columns stay Free.
                let mut col_class: Vec<cpac_cas::VarClass> =
                    vec![cpac_cas::VarClass::Free; columns.len()];
                for (idx_in_int, (orig_idx, _, _)) in int_col_map.iter().enumerate() {
                    let mut cls = classifications[idx_in_int].1.clone();
                    // Remap source_col from int-subset index to original index
                    if let cpac_cas::VarClass::DerivedLinear {
                        source_col,
                        multiplier,
                        offset,
                    } = &cls
                    {
                        let source_orig = int_col_map[*source_col].0;
                        cls = cpac_cas::VarClass::DerivedLinear {
                            source_col: source_orig,
                            multiplier: *multiplier,
                            offset: *offset,
                        };
                    }
                    col_class[*orig_idx] = cls;
                }

                // Determine row count
                let row_count = columns
                    .iter()
                    .filter_map(|(_, ct)| match ct {
                        CpacType::IntColumn { values, .. } => Some(values.len()),
                        _ => None,
                    })
                    .next()
                    .unwrap_or(0) as u32;

                // Build metadata
                let mut meta = Vec::new();
                meta.push(1u8); // version
                meta.extend_from_slice(&(columns.len() as u16).to_le_bytes());
                meta.extend_from_slice(&row_count.to_le_bytes());

                let mut output_columns = Vec::new();

                for (i, (name, ct)) in columns.into_iter().enumerate() {
                    let name_bytes = name.as_bytes();
                    meta.extend_from_slice(&(name_bytes.len() as u16).to_le_bytes());
                    meta.extend_from_slice(name_bytes);

                    match &col_class[i] {
                        cpac_cas::VarClass::Fixed { value } => {
                            meta.push(1u8);
                            meta.extend_from_slice(&value.to_le_bytes());
                            let ow = match &ct {
                                CpacType::IntColumn { original_width, .. } => *original_width,
                                _ => 8,
                            };
                            meta.push(ow);
                        }
                        cpac_cas::VarClass::DerivedLinear {
                            source_col,
                            multiplier,
                            offset,
                        } => {
                            meta.push(2u8);
                            meta.extend_from_slice(&(*source_col as u16).to_le_bytes());
                            meta.extend_from_slice(&multiplier.to_le_bytes());
                            meta.extend_from_slice(&offset.to_le_bytes());
                            let ow = match &ct {
                                CpacType::IntColumn { original_width, .. } => *original_width,
                                _ => 8,
                            };
                            meta.push(ow);
                        }
                        cpac_cas::VarClass::Free => {
                            meta.push(0u8);
                            output_columns.push((name, ct));
                        }
                    }
                }

                Ok((
                    CpacType::ColumnSet {
                        columns: output_columns,
                    },
                    meta,
                ))
            }
            _ => Err(CpacError::Transform(
                "projection: expected ColumnSet input".into(),
            )),
        }
    }

    fn decode(&self, input: CpacType, metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::ColumnSet {
                columns: free_columns,
            } => {
                if metadata.is_empty() {
                    return Ok(CpacType::ColumnSet {
                        columns: free_columns,
                    });
                }
                if metadata.len() < 7 {
                    return Err(CpacError::Transform(
                        "projection: metadata too short".into(),
                    ));
                }

                let _version = metadata[0];
                let original_count =
                    u16::from_le_bytes([metadata[1], metadata[2]]) as usize;
                let row_count =
                    u32::from_le_bytes([metadata[3], metadata[4], metadata[5], metadata[6]])
                        as usize;

                let mut pos = 7;
                let mut col_specs: Vec<(String, ColSpec)> =
                    Vec::with_capacity(original_count);

                for _ in 0..original_count {
                    if pos + 2 > metadata.len() {
                        return Err(CpacError::Transform(
                            "projection: truncated meta".into(),
                        ));
                    }
                    let name_len =
                        u16::from_le_bytes([metadata[pos], metadata[pos + 1]]) as usize;
                    pos += 2;
                    if pos + name_len > metadata.len() {
                        return Err(CpacError::Transform(
                            "projection: truncated name".into(),
                        ));
                    }
                    let name =
                        String::from_utf8_lossy(&metadata[pos..pos + name_len]).to_string();
                    pos += name_len;

                    if pos >= metadata.len() {
                        return Err(CpacError::Transform(
                            "projection: truncated tag".into(),
                        ));
                    }
                    let tag = metadata[pos];
                    pos += 1;

                    match tag {
                        0 => col_specs.push((name, ColSpec::Free)),
                        1 => {
                            if pos + 9 > metadata.len() {
                                return Err(CpacError::Transform(
                                    "projection: truncated Fixed".into(),
                                ));
                            }
                            let value = i64::from_le_bytes(
                                metadata[pos..pos + 8].try_into().unwrap(),
                            );
                            pos += 8;
                            let ow = metadata[pos];
                            pos += 1;
                            col_specs.push((
                                name,
                                ColSpec::Fixed {
                                    value,
                                    original_width: ow,
                                },
                            ));
                        }
                        2 => {
                            if pos + 19 > metadata.len() {
                                return Err(CpacError::Transform(
                                    "projection: truncated DerivedLinear".into(),
                                ));
                            }
                            let source_col = u16::from_le_bytes(
                                [metadata[pos], metadata[pos + 1]],
                            ) as usize;
                            pos += 2;
                            let multiplier = i64::from_le_bytes(
                                metadata[pos..pos + 8].try_into().unwrap(),
                            );
                            pos += 8;
                            let off = i64::from_le_bytes(
                                metadata[pos..pos + 8].try_into().unwrap(),
                            );
                            pos += 8;
                            let ow = metadata[pos];
                            pos += 1;
                            col_specs.push((
                                name,
                                ColSpec::DerivedLinear {
                                    source_col,
                                    multiplier,
                                    offset: off,
                                    original_width: ow,
                                },
                            ));
                        }
                        _ => {
                            return Err(CpacError::Transform(format!(
                                "projection: unknown class tag {tag}"
                            )));
                        }
                    }
                }

                // Reconstruct columns in three passes
                let mut result: Vec<Option<(String, CpacType)>> = vec![None; original_count];
                let mut free_iter = free_columns.into_iter();

                // Pass 1: place Free columns
                for (i, (_, spec)) in col_specs.iter().enumerate() {
                    if matches!(spec, ColSpec::Free) {
                        let col = free_iter.next().ok_or_else(|| {
                            CpacError::Transform(
                                "projection: not enough free columns".into(),
                            )
                        })?;
                        result[i] = Some(col);
                    }
                }

                // Pass 2: reconstruct Fixed columns
                for (i, (name, spec)) in col_specs.iter().enumerate() {
                    if let ColSpec::Fixed {
                        value,
                        original_width,
                    } = spec
                    {
                        result[i] = Some((
                            name.clone(),
                            CpacType::IntColumn {
                                values: vec![*value; row_count],
                                original_width: *original_width,
                            },
                        ));
                    }
                }

                // Pass 3: reconstruct DerivedLinear columns
                for (i, (name, spec)) in col_specs.iter().enumerate() {
                    if let ColSpec::DerivedLinear {
                        source_col,
                        multiplier,
                        offset,
                        original_width,
                    } = spec
                    {
                        let source_values = match &result[*source_col] {
                            Some((_, CpacType::IntColumn { values, .. })) => values.clone(),
                            _ => {
                                return Err(CpacError::Transform(format!(
                                    "projection: source column {source_col} not available"
                                )));
                            }
                        };
                        let derived: Vec<i64> = source_values
                            .iter()
                            .map(|&x| multiplier.wrapping_mul(x).wrapping_add(*offset))
                            .collect();
                        result[i] = Some((
                            name.clone(),
                            CpacType::IntColumn {
                                values: derived,
                                original_width: *original_width,
                            },
                        ));
                    }
                }

                let columns: Vec<(String, CpacType)> =
                    result.into_iter().flatten().collect();

                Ok(CpacType::ColumnSet { columns })
            }
            _ => Err(CpacError::Transform(
                "projection: expected ColumnSet input".into(),
            )),
        }
    }
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

/// Column reconstruction specification (from metadata).
enum ColSpec {
    Free,
    Fixed {
        value: i64,
        original_width: u8,
    },
    DerivedLinear {
        source_col: usize,
        multiplier: i64,
        offset: i64,
        original_width: u8,
    },
}

/// Extract integer column data from a `ColumnSet`.
fn extract_int_data(columns: &[(String, CpacType)]) -> Vec<(String, Vec<i64>)> {
    columns
        .iter()
        .filter_map(|(name, ct)| match ct {
            CpacType::IntColumn { values, .. } => Some((name.clone(), values.clone())),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_test_columnset() -> CpacType {
        // id:       [0, 1, 2, 3, 4]  (free)
        // constant: [42, 42, 42, 42, 42]  (fixed)
        // derived:  [5, 7, 9, 11, 13]  = 2*id + 5  (derived linear)
        CpacType::ColumnSet {
            columns: vec![
                (
                    "id".to_string(),
                    CpacType::IntColumn {
                        values: (0..5).collect(),
                        original_width: 4,
                    },
                ),
                (
                    "constant".to_string(),
                    CpacType::IntColumn {
                        values: vec![42; 5],
                        original_width: 4,
                    },
                ),
                (
                    "derived".to_string(),
                    CpacType::IntColumn {
                        values: (0..5).map(|i| 2 * i + 5).collect(),
                        original_width: 4,
                    },
                ),
            ],
        }
    }

    #[test]
    fn projection_roundtrip() {
        let t = ProjectionTransform;
        let input = make_test_columnset();
        let ctx = TransformContext {
            entropy_estimate: 3.0,
            ascii_ratio: 0.0,
            data_size: 60,
        };

        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        assert!(!meta.is_empty(), "metadata should be non-empty");

        // Output should only contain "id" column
        match &encoded {
            CpacType::ColumnSet { columns } => {
                assert_eq!(columns.len(), 1, "only free columns should remain");
                assert_eq!(columns[0].0, "id");
            }
            _ => panic!("expected ColumnSet output"),
        }

        // Decode should reconstruct all 3 columns
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::ColumnSet { columns } => {
                assert_eq!(columns.len(), 3);
                assert_eq!(columns[0].0, "id");
                assert_eq!(columns[1].0, "constant");
                assert_eq!(columns[2].0, "derived");
                if let CpacType::IntColumn { values, .. } = &columns[0].1 {
                    assert_eq!(values, &vec![0, 1, 2, 3, 4]);
                }
                if let CpacType::IntColumn { values, .. } = &columns[1].1 {
                    assert_eq!(values, &vec![42, 42, 42, 42, 42]);
                }
                if let CpacType::IntColumn { values, .. } = &columns[2].1 {
                    assert_eq!(values, &vec![5, 7, 9, 11, 13]);
                }
            }
            _ => panic!("expected ColumnSet"),
        }
    }

    #[test]
    fn projection_passthrough_too_few_columns() {
        let t = ProjectionTransform;
        let input = CpacType::ColumnSet {
            columns: vec![(
                "only".to_string(),
                CpacType::IntColumn {
                    values: vec![1, 2, 3],
                    original_width: 4,
                },
            )],
        };
        let ctx = TransformContext {
            entropy_estimate: 3.0,
            ascii_ratio: 0.0,
            data_size: 12,
        };
        let (_, meta) = t.encode(input, &ctx).unwrap();
        assert!(meta.is_empty(), "should passthrough with too few columns");
    }

    #[test]
    fn projection_estimate_gain() {
        let t = ProjectionTransform;
        let input = make_test_columnset();
        let ctx = TransformContext {
            entropy_estimate: 3.0,
            ascii_ratio: 0.0,
            data_size: 60,
        };
        let gain = t.estimate_gain(&input, &ctx);
        assert!(gain.is_some(), "should estimate positive gain");
        assert!(gain.unwrap() > 0.0);
    }
}
