// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! JSON structured logging domain handler.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use serde_json::Value;
use std::collections::HashMap;

/// JSON structured log domain handler.
///
/// Extracts repeated field names from line-delimited JSON logs.
/// Target compression: 30-60x on JSON logs.
pub struct JsonLogDomain;

impl Domain for JsonLogDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "log.json",
            name: "JSON Log",
            extensions: &[".jsonl", ".ndjson", ".log"],
            mime_types: &["application/x-ndjson"],
            magic_bytes: &[],
        }
    }

    fn detect(&self, data: &[u8], filename: Option<&str>) -> f64 {
        if let Some(fname) = filename {
            if fname.ends_with(".jsonl") || fname.ends_with(".ndjson") {
                return 0.9;
            }
        }

        let text = std::str::from_utf8(data).unwrap_or("");
        
        // Check if multiple lines are valid JSON objects
        let valid_json_lines = text.lines().take(10).filter(|line| {
            !line.trim().is_empty() && serde_json::from_str::<Value>(line).is_ok()
        }).count();

        if valid_json_lines >= 5 {
            return 0.8;
        }

        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("JSON log decode: {}", e)))?;

        let mut key_freq: HashMap<String, usize> = HashMap::new();

        // Parse each line as JSON
        for line in text.lines() {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                extract_keys(&value, &mut key_freq);
            }
        }

        // Extract keys with frequency >= 2
        let mut repeated_keys: Vec<(String, usize)> = key_freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated_keys.sort_by(|a, b| b.1.cmp(&a.1));

        // Build key map
        let mut key_map: HashMap<String, u32> = HashMap::new();
        for (idx, (key, _)) in repeated_keys.iter().enumerate() {
            key_map.insert(key.clone(), idx as u32);
        }

        // Compact each line
        let mut compacted_lines = Vec::new();
        for line in text.lines() {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                let compacted = compact_value(&value, &key_map);
                if let Ok(compacted_str) = serde_json::to_string(&compacted) {
                    compacted_lines.push(compacted_str);
                } else {
                    compacted_lines.push(line.to_string());
                }
            } else {
                compacted_lines.push(line.to_string());
            }
        }

        let mut fields = HashMap::new();
        fields.insert("keys".to_string(), Value::Array(
            repeated_keys.iter().map(|(k, _)| Value::String(k.clone())).collect()
        ));

        Ok(ExtractionResult {
            fields,
            residual: compacted_lines.join("\n").into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.json".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let keys_value = result.fields.get("keys")
            .ok_or_else(|| CpacError::DecompressFailed("Missing keys".into()))?;

        let keys: Vec<String> = if let Value::Array(arr) = keys_value {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid keys format".into()));
        };

        let text = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {}", e)))?;

        let mut reconstructed_lines = Vec::new();
        for line in text.lines() {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                let expanded = expand_value(&value, &keys);
                if let Ok(expanded_str) = serde_json::to_string(&expanded) {
                    reconstructed_lines.push(expanded_str);
                } else {
                    reconstructed_lines.push(line.to_string());
                }
            } else {
                reconstructed_lines.push(line.to_string());
            }
        }

        Ok(reconstructed_lines.join("\n").into_bytes())
    }
}

fn extract_keys(value: &Value, key_freq: &mut HashMap<String, usize>) {
    match value {
        Value::Object(map) => {
            for (key, val) in map {
                *key_freq.entry(key.clone()).or_insert(0) += 1;
                extract_keys(val, key_freq);
            }
        }
        Value::Array(arr) => {
            for val in arr {
                extract_keys(val, key_freq);
            }
        }
        _ => {}
    }
}

fn compact_value(value: &Value, key_map: &HashMap<String, u32>) -> Value {
    match value {
        Value::Object(map) => {
            let new_map: serde_json::Map<String, Value> = map
                .iter()
                .map(|(k, v)| {
                    let new_key = key_map.get(k)
                        .map(|idx| format!("$L{}", idx))
                        .unwrap_or_else(|| k.clone());
                    (new_key, compact_value(v, key_map))
                })
                .collect();
            Value::Object(new_map)
        }
        Value::Array(arr) => {
            Value::Array(arr.iter().map(|v| compact_value(v, key_map)).collect())
        }
        _ => value.clone(),
    }
}

fn expand_value(value: &Value, keys: &[String]) -> Value {
    match value {
        Value::Object(map) => {
            let new_map: serde_json::Map<String, Value> = map
                .iter()
                .map(|(k, v)| {
                    let orig_key = if k.starts_with("$L") {
                        let idx = k[2..].parse::<usize>().unwrap_or(0);
                        keys.get(idx).cloned().unwrap_or_else(|| k.clone())
                    } else {
                        k.clone()
                    };
                    (orig_key, expand_value(v, keys))
                })
                .collect();
            Value::Object(new_map)
        }
        Value::Array(arr) => {
            Value::Array(arr.iter().map(|v| expand_value(v, keys)).collect())
        }
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_log_domain_roundtrip() {
        let domain = JsonLogDomain;
        let data = b"{\"level\":\"info\",\"msg\":\"test1\"}\n{\"level\":\"error\",\"msg\":\"test2\"}";

        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        // Parse both to verify semantic equivalence
        let orig_lines: Vec<Value> = std::str::from_utf8(data).unwrap()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();
        let recon_lines: Vec<Value> = std::str::from_utf8(&reconstructed).unwrap()
            .lines()
            .filter_map(|l| serde_json::from_str(l).ok())
            .collect();

        assert_eq!(orig_lines, recon_lines);
    }
}
