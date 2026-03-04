// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! JSON domain handler — decomposes JSON arrays into typed columns.

use cpac_types::{CpacError, CpacResult, CpacType, DomainHint};
use std::collections::BTreeMap;

use crate::DomainHandler;

/// JSON domain handler.
pub struct JsonHandler;

impl DomainHandler for JsonHandler {
    fn name(&self) -> &'static str {
        "json"
    }
    fn domain_hint(&self) -> DomainHint {
        DomainHint::Json
    }
    fn can_handle(&self, data: &[u8]) -> bool {
        let trimmed = data
            .iter()
            .find(|b| !b.is_ascii_whitespace())
            .copied()
            .unwrap_or(0);
        trimmed == b'[' || trimmed == b'{'
    }
    fn decompose(&self, data: &[u8]) -> CpacResult<CpacType> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::Transform(format!("JSON: invalid UTF-8: {e}")))?;
        let value: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| CpacError::Transform(format!("JSON parse: {e}")))?;

        match value {
            serde_json::Value::Array(arr) => decompose_array(&arr),
            serde_json::Value::Object(_) => {
                // Wrap single object in array
                decompose_array(&[value])
            }
            _ => Ok(CpacType::Serial(data.to_vec())),
        }
    }
    fn reconstruct(&self, columns: &CpacType) -> CpacResult<Vec<u8>> {
        match columns {
            CpacType::ColumnSet { columns } => {
                let num_rows = match &columns[0].1 {
                    CpacType::StringColumn { values, .. } => values.len(),
                    CpacType::IntColumn { values, .. } => values.len(),
                    _ => 0,
                };
                let mut rows: Vec<serde_json::Map<String, serde_json::Value>> =
                    vec![serde_json::Map::new(); num_rows];
                for (name, col) in columns {
                    match col {
                        CpacType::StringColumn { values, .. } => {
                            for (i, v) in values.iter().enumerate() {
                                rows[i].insert(name.clone(), serde_json::Value::String(v.clone()));
                            }
                        }
                        CpacType::IntColumn { values, .. } => {
                            for (i, v) in values.iter().enumerate() {
                                rows[i]
                                    .insert(name.clone(), serde_json::Value::Number((*v).into()));
                            }
                        }
                        _ => {}
                    }
                }
                let arr: Vec<serde_json::Value> =
                    rows.into_iter().map(serde_json::Value::Object).collect();
                let json = serde_json::to_vec_pretty(&arr)
                    .map_err(|e| CpacError::Transform(format!("JSON serialize: {e}")))?;
                Ok(json)
            }
            _ => Err(CpacError::Transform(
                "JSON reconstruct: expected ColumnSet".into(),
            )),
        }
    }
}

/// Decompose a JSON array of objects into columns.
fn decompose_array(arr: &[serde_json::Value]) -> CpacResult<CpacType> {
    if arr.is_empty() {
        return Ok(CpacType::ColumnSet {
            columns: Vec::new(),
        });
    }

    // Collect all keys in order
    let mut keys: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for item in arr {
        if let serde_json::Value::Object(obj) = item {
            for (k, v) in obj {
                let col = keys.entry(k.clone()).or_default();
                col.push(value_to_string(v));
            }
        }
    }

    let typed_columns: Vec<(String, CpacType)> = keys
        .into_iter()
        .map(|(name, values)| {
            let total_bytes: usize = values.iter().map(std::string::String::len).sum();
            (
                name,
                CpacType::StringColumn {
                    values,
                    total_bytes,
                },
            )
        })
        .collect();

    Ok(CpacType::ColumnSet {
        columns: typed_columns,
    })
}

fn value_to_string(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => String::new(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decompose_json_array() {
        let data = br#"[{"name":"Alice","age":30},{"name":"Bob","age":25}]"#;
        let handler = JsonHandler;
        let result = handler.decompose(data).unwrap();
        match result {
            CpacType::ColumnSet { columns } => {
                assert_eq!(columns.len(), 2);
                // BTreeMap sorts keys: "age" before "name"
                assert_eq!(columns[0].0, "age");
                assert_eq!(columns[1].0, "name");
            }
            _ => panic!("expected ColumnSet"),
        }
    }

    #[test]
    fn detect_json_data() {
        let handler = JsonHandler;
        assert!(handler.can_handle(b"[1,2,3]"));
        assert!(handler.can_handle(b"{\"key\":1}"));
        assert!(!handler.can_handle(b"hello"));
    }
}
