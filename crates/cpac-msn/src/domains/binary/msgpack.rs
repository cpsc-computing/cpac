// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! `MessagePack` domain handler with structure extraction.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use rmp_serde::{decode, encode};
use serde_json::Value;
use std::collections::HashMap;

/// `MessagePack` domain handler.
///
/// Extracts keys and structure from `MessagePack` data.
/// Target compression: 30-60x on structured `MessagePack`.
pub struct MsgPackDomain;

impl Domain for MsgPackDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "binary.msgpack",
            name: "MessagePack",
            extensions: &[".msgpack", ".mp"],
            mime_types: &["application/msgpack", "application/x-msgpack"],
            magic_bytes: &[],
        }
    }

    fn detect(&self, data: &[u8], filename: Option<&str>) -> f64 {
        if let Some(fname) = filename {
            if std::path::Path::new(fname)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("msgpack"))
                || std::path::Path::new(fname)
                    .extension()
                    .is_some_and(|e| e.eq_ignore_ascii_case("mp"))
            {
                return 0.9;
            }
        }

        // MessagePack detection must be strict to avoid false positives
        // Only detect if:
        // 1. Data successfully parses as MessagePack
        // 2. AND it's not plain ASCII text (MessagePack is binary)
        // 3. AND it's structured (object or array)

        // Check if it's mostly ASCII text (indicates not MessagePack)
        #[allow(clippy::cast_precision_loss)]
        let ascii_ratio =
            data.iter().filter(|&&b| (32u8..127u8).contains(&b)).count() as f64 / data.len() as f64;
        if ascii_ratio > 0.9 {
            // Likely plain text, not MessagePack
            return 0.0;
        }

        // Try to parse as MessagePack and check structure
        if let Ok(value) = decode::from_slice::<Value>(data) {
            // Only consider structured data (objects/arrays)
            match value {
                Value::Object(ref map) if !map.is_empty() => return 0.7,
                Value::Array(ref arr) if arr.len() > 1 => return 0.6,
                _ => return 0.0, // Plain strings/numbers are not MessagePack-specific
            }
        }

        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let value: Value = decode::from_slice(data)
            .map_err(|e| CpacError::CompressFailed(format!("MessagePack decode: {e}")))?;

        // Extract all keys recursively
        let mut key_freq: HashMap<String, usize> = HashMap::new();
        extract_keys(&value, &mut key_freq);

        // Extract keys with frequency >= 2
        let mut repeated_keys: Vec<(String, usize)> = key_freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated_keys.sort_by(|a, b| b.1.cmp(&a.1));

        // Build key map
        let mut key_map: HashMap<String, u32> = HashMap::new();
        #[allow(clippy::cast_possible_truncation)]
        for (idx, (key, _)) in repeated_keys.iter().enumerate() {
            key_map.insert(key.clone(), idx as u32); // bounded by key count
        }

        // Compact value by replacing keys
        let compacted = compact_value(&value, &key_map);

        // Serialize compacted value
        let residual = encode::to_vec(&compacted)
            .map_err(|e| CpacError::CompressFailed(format!("MessagePack encode: {e}")))?;

        let mut fields = HashMap::new();
        fields.insert(
            "keys".to_string(),
            Value::Array(
                repeated_keys
                    .iter()
                    .map(|(k, _)| Value::String(k.clone()))
                    .collect(),
            ),
        );

        Ok(ExtractionResult {
            fields,
            residual,
            metadata: HashMap::new(),
            domain_id: "binary.msgpack".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let keys_value = result
            .fields
            .get("keys")
            .ok_or_else(|| CpacError::DecompressFailed("Missing keys".into()))?;

        let keys: Vec<String> = if let Value::Array(arr) = keys_value {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid keys format".into()));
        };

        let compacted: Value = decode::from_slice(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("MessagePack decode: {e}")))?;

        let expanded = expand_value(&compacted, &keys);

        encode::to_vec(&expanded)
            .map_err(|e| CpacError::DecompressFailed(format!("MessagePack encode: {e}")))
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
                    let new_key = key_map
                        .get(k)
                        .map_or_else(|| k.clone(), |idx| format!("$K{idx}"));
                    (new_key, compact_value(v, key_map))
                })
                .collect();
            Value::Object(new_map)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(|v| compact_value(v, key_map)).collect()),
        _ => value.clone(),
    }
}

fn expand_value(value: &Value, keys: &[String]) -> Value {
    match value {
        Value::Object(map) => {
            let new_map: serde_json::Map<String, Value> = map
                .iter()
                .map(|(k, v)| {
                    let orig_key = k
                        .strip_prefix("$K")
                        .and_then(|s| s.parse::<usize>().ok())
                        .and_then(|idx| keys.get(idx).cloned())
                        .unwrap_or_else(|| k.clone());
                    (orig_key, expand_value(v, keys))
                })
                .collect();
            Value::Object(new_map)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(|v| expand_value(v, keys)).collect()),
        _ => value.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn msgpack_domain_roundtrip() {
        let domain = MsgPackDomain;
        let data = r#"{"name":"Alice","age":30,"name":"Bob","age":25}"#;
        let value: Value = serde_json::from_str(data).unwrap();
        let msgpack_data = encode::to_vec(&value).unwrap();

        let result = domain.extract(&msgpack_data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        let orig: Value = decode::from_slice(&msgpack_data).unwrap();
        let recon: Value = decode::from_slice(&reconstructed).unwrap();
        assert_eq!(orig, recon);
    }
}
