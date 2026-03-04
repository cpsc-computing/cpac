// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Parse int: converts ASCII numeric string columns to typed integer columns.
//!
//! This is a text→binary transform that enables delta, zigzag, and
//! `range_pack` to work on data that was originally textual CSV/TSV.

use cpac_types::{CpacError, CpacResult, CpacType, TypeTag};

use crate::traits::{TransformContext, TransformNode};

/// Transform ID for `parse_int` (wire format).
pub const TRANSFORM_ID: u8 = 11;

/// Try to parse a list of strings as integers.
///
/// Returns `(success, values)`. Empty strings map to 0.
#[must_use] 
pub fn parse_int_column(strings: &[String]) -> (bool, Vec<i64>) {
    let mut values = Vec::with_capacity(strings.len());
    for s in strings {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            values.push(0);
            continue;
        }
        match trimmed.parse::<i64>() {
            Ok(v) => values.push(v),
            Err(_) => return (false, Vec::new()),
        }
    }
    (true, values)
}

/// Determine the minimum byte width needed for a set of i64 values.
fn detect_width(values: &[i64]) -> u8 {
    if values.is_empty() {
        return 1;
    }
    let abs_max = values.iter().map(|v| v.unsigned_abs()).max().unwrap_or(0);
    if abs_max <= 0x7F {
        1
    } else if abs_max <= 0x7FFF {
        2
    } else if abs_max <= 0x7FFF_FFFF {
        4
    } else {
        8
    }
}

/// Parse int transform node.
pub struct ParseIntTransform;

impl TransformNode for ParseIntTransform {
    fn name(&self) -> &'static str {
        "parse_int"
    }
    fn id(&self) -> u8 {
        TRANSFORM_ID
    }
    fn accepts(&self) -> &[TypeTag] {
        &[TypeTag::StringColumn]
    }
    fn produces(&self) -> TypeTag {
        TypeTag::IntColumn
    }
    fn estimate_gain(&self, input: &CpacType, _ctx: &TransformContext) -> Option<f64> {
        match input {
            CpacType::StringColumn { values, .. } => {
                if values.len() < 4 {
                    return None;
                }
                // Sample first 100 values
                let sample: Vec<&String> = values.iter().take(100).collect();
                let parseable = sample
                    .iter()
                    .filter(|s| s.trim().is_empty() || s.trim().parse::<i64>().is_ok())
                    .count();
                if parseable == sample.len() {
                    Some(3.0) // text→binary is typically 2-5x savings
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
                let (success, int_values) = parse_int_column(&values);
                if !success {
                    return Err(CpacError::Transform(
                        "parse_int: not all values are integers".into(),
                    ));
                }
                let width = detect_width(&int_values);
                Ok((
                    CpacType::IntColumn {
                        values: int_values,
                        original_width: width,
                    },
                    Vec::new(),
                ))
            }
            _ => Err(CpacError::Transform("parse_int: unsupported type".into())),
        }
    }
    fn decode(&self, input: CpacType, _metadata: &[u8]) -> CpacResult<CpacType> {
        match input {
            CpacType::IntColumn { values, .. } => {
                let strings: Vec<String> = values.iter().map(std::string::ToString::to_string).collect();
                let total_bytes: usize = strings.iter().map(std::string::String::len).sum();
                Ok(CpacType::StringColumn {
                    values: strings,
                    total_bytes,
                })
            }
            _ => Err(CpacError::Transform("parse_int: unsupported type".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_valid_ints() {
        let strings: Vec<String> = vec!["100", "-50", "0", "42", ""]
            .into_iter()
            .map(String::from)
            .collect();
        let (ok, values) = parse_int_column(&strings);
        assert!(ok);
        assert_eq!(values, vec![100, -50, 0, 42, 0]);
    }

    #[test]
    fn parse_invalid() {
        let strings: Vec<String> = vec!["100", "abc", "42"]
            .into_iter()
            .map(String::from)
            .collect();
        let (ok, _) = parse_int_column(&strings);
        assert!(!ok);
    }

    #[test]
    fn transform_roundtrip() {
        let t = ParseIntTransform;
        let ctx = TransformContext {
            entropy_estimate: 4.0,
            ascii_ratio: 1.0,
            data_size: 100,
        };
        let values: Vec<String> = vec!["10", "20", "30", "-5"]
            .into_iter()
            .map(String::from)
            .collect();
        let input = CpacType::StringColumn {
            values: values.clone(),
            total_bytes: 10,
        };
        let (encoded, meta) = t.encode(input, &ctx).unwrap();
        let decoded = t.decode(encoded, &meta).unwrap();
        match decoded {
            CpacType::StringColumn { values: v, .. } => {
                assert_eq!(v, vec!["10", "20", "30", "-5"]);
            }
            _ => panic!("expected StringColumn"),
        }
    }
}
