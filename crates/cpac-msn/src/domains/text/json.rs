// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! JSON domain handler with semantic field extraction.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use serde_json::Value;
use std::collections::HashMap;

/// JSON domain handler.
///
/// Extracts repeated field names and structure from JSON data.
/// Target compression: 50-100x on repetitive JSON.
pub struct JsonDomain;

impl JsonDomain {
    /// Extract field names from JSON value recursively.
    fn extract_field_names(value: &Value, fields: &mut Vec<String>) {
        match value {
            Value::Object(map) => {
                for (key, val) in map {
                    fields.push(key.clone());
                    Self::extract_field_names(val, fields);
                }
            }
            Value::Array(arr) => {
                for val in arr {
                    Self::extract_field_names(val, fields);
                }
            }
            _ => {}
        }
    }

    /// Compact JSON by extracting repeated field names.
    fn compact_json(value: &Value, field_map: &HashMap<String, u32>) -> Value {
        match value {
            Value::Object(map) => {
                let mut new_map = serde_json::Map::new();
                for (key, val) in map {
                    // Replace field name with index if it appears multiple times
                    let new_key = if let Some(&idx) = field_map.get(key) {
                        format!("${}", idx)
                    } else {
                        key.clone()
                    };
                    new_map.insert(new_key, Self::compact_json(val, field_map));
                }
                Value::Object(new_map)
            }
            Value::Array(arr) => {
                Value::Array(arr.iter().map(|v| Self::compact_json(v, field_map)).collect())
            }
            other => other.clone(),
        }
    }

    /// Reconstruct JSON by restoring field names from index.
    fn expand_json(value: &Value, field_names: &[String]) -> Value {
        match value {
            Value::Object(map) => {
                let mut new_map = serde_json::Map::new();
                for (key, val) in map {
                    let new_key = if key.starts_with('$') {
                        if let Ok(idx) = key[1..].parse::<usize>() {
                            field_names.get(idx).cloned().unwrap_or_else(|| key.clone())
                        } else {
                            key.clone()
                        }
                    } else {
                        key.clone()
                    };
                    new_map.insert(new_key, Self::expand_json(val, field_names));
                }
                Value::Object(new_map)
            }
            Value::Array(arr) => {
                Value::Array(arr.iter().map(|v| Self::expand_json(v, field_names)).collect())
            }
            other => other.clone(),
        }
    }
}

impl Domain for JsonDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "text.json",
            name: "JSON",
            extensions: &[".json", ".jsonl"],
            mime_types: &["application/json", "text/json"],
            magic_bytes: &[b"{", b"["],
        }
    }

    fn detect(&self, data: &[u8], filename: Option<&str>) -> f64 {
        // Check filename extension
        if let Some(fname) = filename {
            if fname.ends_with(".json") || fname.ends_with(".jsonl") {
                return 0.9;
            }
        }

        // Check magic bytes
        let trimmed = data.iter().skip_while(|b| b.is_ascii_whitespace()).copied().take(10).collect::<Vec<_>>();
        if trimmed.is_empty() {
            return 0.0;
        }

        if trimmed[0] == b'{' || trimmed[0] == b'[' {
            // Try to parse as JSON
            if serde_json::from_slice::<Value>(data).is_ok() {
                return 0.95;
            }
            return 0.6;
        }

        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        // Parse JSON
        let value: Value = serde_json::from_slice(data)
            .map_err(|e| CpacError::CompressFailed(format!("JSON parse error: {}", e)))?;

        // Extract all field names
        let mut field_names = Vec::new();
        Self::extract_field_names(&value, &mut field_names);

        // Count field name frequencies
        let mut field_counts: HashMap<String, u32> = HashMap::new();
        for name in &field_names {
            *field_counts.entry(name.clone()).or_insert(0) += 1;
        }

        // Build field map for repeated names (appearing 2+ times)
        let mut field_map: HashMap<String, u32> = HashMap::new();
        let mut repeated_fields: Vec<String> = field_counts
            .iter()
            .filter(|(_, &count)| count >= 2)
            .map(|(name, _)| name.clone())
            .collect();
        repeated_fields.sort();
        for (idx, name) in repeated_fields.iter().enumerate() {
            field_map.insert(name.clone(), idx as u32);
        }

        // Compact JSON with field indices
        let compacted = Self::compact_json(&value, &field_map);
        let residual = serde_json::to_vec(&compacted)
            .map_err(|e| CpacError::CompressFailed(format!("JSON serialize error: {}", e)))?;

        // Store repeated field names in extraction result
        let mut fields = HashMap::new();
        fields.insert("field_names".to_string(), Value::Array(
            repeated_fields.into_iter().map(Value::String).collect()
        ));
        fields.insert("original_size".to_string(), Value::Number(data.len().into()));

        Ok(ExtractionResult {
            fields,
            residual,
            metadata: HashMap::new(),
            domain_id: "text.json".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        // Extract field names from metadata
        let field_names_value = result.fields.get("field_names")
            .ok_or_else(|| CpacError::DecompressFailed("Missing field_names".into()))?;
        
        let field_names: Vec<String> = if let Value::Array(arr) = field_names_value {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid field_names format".into()));
        };

        // Parse compacted JSON
        let compacted: Value = serde_json::from_slice(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("JSON parse error: {}", e)))?;

        // Expand JSON with original field names
        let expanded = Self::expand_json(&compacted, &field_names);

        // Serialize back to bytes
        serde_json::to_vec(&expanded)
            .map_err(|e| CpacError::DecompressFailed(format!("JSON serialize error: {}", e)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_domain_detection() {
        let domain = JsonDomain;
        
        // Valid JSON
        assert!(domain.detect(br#"{"key": "value"}"#, None) > 0.9);
        assert!(domain.detect(br#"[1, 2, 3]"#, None) > 0.9);
        
        // Filename detection
        assert!(domain.detect(b"", Some("test.json")) > 0.8);
        
        // Non-JSON
        assert!(domain.detect(b"plain text", None) < 0.1);
    }

    #[test]
    fn json_domain_roundtrip() {
        let domain = JsonDomain;
        let data = br#"{"name":"Alice","age":30,"name":"Bob","age":25}"#;
        
        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();
        
        // Parse both to compare structure (order may differ)
        let original: Value = serde_json::from_slice(data).unwrap();
        let recovered: Value = serde_json::from_slice(&reconstructed).unwrap();
        
        assert_eq!(original, recovered);
    }

    #[test]
    fn json_domain_compression_on_repetitive() {
        let domain = JsonDomain;
        
        // Highly repetitive JSON
        let data = br#"[
            {"user":"alice","score":100,"level":5},
            {"user":"bob","score":200,"level":10},
            {"user":"charlie","score":150,"level":7}
        ]"#;
        
        let result = domain.extract(data).unwrap();
        
        // Residual should be smaller than original (field names extracted)
        assert!(result.residual.len() < data.len());
        
        // Should have extracted field names
        assert!(result.fields.contains_key("field_names"));
        
        // Verify roundtrip
        let reconstructed = domain.reconstruct(&result).unwrap();
        let original: Value = serde_json::from_slice(data).unwrap();
        let recovered: Value = serde_json::from_slice(&reconstructed).unwrap();
        assert_eq!(original, recovered);
    }
}
