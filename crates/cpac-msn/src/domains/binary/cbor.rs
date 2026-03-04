// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! CBOR domain handler with structure extraction.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use serde_json::Value;
use std::collections::HashMap;

/// CBOR domain handler.
///
/// Extracts keys and structure from CBOR data.
/// Target compression: 25-50x on structured CBOR.
pub struct CborDomain;

impl Domain for CborDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "binary.cbor",
            name: "CBOR",
            extensions: &[".cbor"],
            mime_types: &["application/cbor"],
            magic_bytes: &[],
        }
    }

    fn detect(&self, data: &[u8], filename: Option<&str>) -> f64 {
        if let Some(fname) = filename {
            if std::path::Path::new(fname)
                .extension().is_some_and(|e| e.eq_ignore_ascii_case("cbor")) {
                return 0.9;
            }
        }

        // CBOR detection must be strict to avoid false positives
        // Check if it's mostly ASCII text (indicates not CBOR)
        #[allow(clippy::cast_precision_loss)]
        let ascii_ratio = data.iter().filter(|&&b| (32u8..127u8).contains(&b)).count() as f64 / data.len() as f64;
        if ascii_ratio > 0.9 {
            // Likely plain text, not CBOR
            return 0.0;
        }

        // Try to parse as CBOR and check structure
        match ciborium::from_reader::<ciborium::Value, _>(data) {
            Ok(value) => {
                // Only consider structured data (objects/arrays)
                match value {
                    ciborium::Value::Map(ref map) if !map.is_empty() => 0.7,
                    ciborium::Value::Array(ref arr) if arr.len() > 1 => 0.6,
                    _ => 0.0, // Plain strings/numbers are not CBOR-specific
                }
            }
            Err(_) => 0.0,
        }
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let cbor_value: ciborium::Value = ciborium::from_reader(data)
            .map_err(|e| CpacError::CompressFailed(format!("CBOR decode: {e}")))?;

        // Convert to JSON Value for easier key extraction
        let json_value = cbor_to_json(&cbor_value)
            .map_err(|e| CpacError::CompressFailed(format!("CBOR to JSON: {e}")))?;

        // Extract all keys recursively
        let mut key_freq: HashMap<String, usize> = HashMap::new();
        extract_keys(&json_value, &mut key_freq);

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
        let compacted = compact_value(&json_value, &key_map);
        let compacted_cbor = json_to_cbor(&compacted)
            .map_err(|e| CpacError::CompressFailed(format!("JSON to CBOR: {e}")))?;

        // Serialize compacted value
        let mut residual = Vec::new();
        ciborium::into_writer(&compacted_cbor, &mut residual)
            .map_err(|e| CpacError::CompressFailed(format!("CBOR encode: {e}")))?;

        let mut fields = HashMap::new();
        fields.insert("keys".to_string(), Value::Array(
            repeated_keys.iter().map(|(k, _)| Value::String(k.clone())).collect()
        ));

        Ok(ExtractionResult {
            fields,
            residual,
            metadata: HashMap::new(),
            domain_id: "binary.cbor".to_string(),
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

        let compacted_cbor: ciborium::Value = ciborium::from_reader(&result.residual[..])
            .map_err(|e| CpacError::DecompressFailed(format!("CBOR decode: {e}")))?;

        let compacted = cbor_to_json(&compacted_cbor)
            .map_err(|e| CpacError::DecompressFailed(format!("CBOR to JSON: {e}")))?;

        let expanded = expand_value(&compacted, &keys);
        let expanded_cbor = json_to_cbor(&expanded)
            .map_err(|e| CpacError::DecompressFailed(format!("JSON to CBOR: {e}")))?;

        let mut output = Vec::new();
        ciborium::into_writer(&expanded_cbor, &mut output)
            .map_err(|e| CpacError::DecompressFailed(format!("CBOR encode: {e}")))?;

        Ok(output)
    }
}

fn cbor_to_json(cbor: &ciborium::Value) -> Result<Value, String> {
    match cbor {
        ciborium::Value::Integer(i) => {
            // Convert ciborium Integer to i64
            let val: i64 = TryInto::<i64>::try_into(*i)
                .map_err(|_| "Integer out of range".to_string())?;
            Ok(Value::Number(val.into()))
        }
        ciborium::Value::Bytes(b) => Ok(Value::String(base64::Engine::encode(&base64::engine::general_purpose::STANDARD, b))),
        ciborium::Value::Float(f) => {
            serde_json::Number::from_f64(*f)
                .map(Value::Number)
                .ok_or_else(|| "Invalid float".to_string())
        }
        ciborium::Value::Text(s) => Ok(Value::String(s.clone())),
        ciborium::Value::Bool(b) => Ok(Value::Bool(*b)),
        ciborium::Value::Null => Ok(Value::Null),
        ciborium::Value::Array(arr) => {
            let json_arr: Result<Vec<_>, _> = arr.iter().map(cbor_to_json).collect();
            json_arr.map(Value::Array)
        }
        ciborium::Value::Map(map) => {
            let mut json_map = serde_json::Map::new();
            for (k, v) in map {
                let key = match k {
                    ciborium::Value::Text(s) => s.clone(),
                    _ => return Err("Non-string keys not supported".to_string()),
                };
                json_map.insert(key, cbor_to_json(v)?);
            }
            Ok(Value::Object(json_map))
        }
        _ => Err("Unsupported CBOR type".to_string()),
    }
}

fn json_to_cbor(json: &Value) -> Result<ciborium::Value, String> {
    match json {
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(ciborium::Value::Integer(i.into()))
            } else if let Some(f) = n.as_f64() {
                Ok(ciborium::Value::Float(f))
            } else {
                Err("Invalid number".to_string())
            }
        }
        Value::String(s) => Ok(ciborium::Value::Text(s.clone())),
        Value::Bool(b) => Ok(ciborium::Value::Bool(*b)),
        Value::Null => Ok(ciborium::Value::Null),
        Value::Array(arr) => {
            let cbor_arr: Result<Vec<_>, _> = arr.iter().map(json_to_cbor).collect();
            cbor_arr.map(ciborium::Value::Array)
        }
        Value::Object(map) => {
            let mut cbor_map = Vec::new();
            for (k, v) in map {
                cbor_map.push((ciborium::Value::Text(k.clone()), json_to_cbor(v)?));
            }
            Ok(ciborium::Value::Map(cbor_map))
        }
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
                    let new_key = key_map.get(k).map_or_else(|| k.clone(), |idx| format!("$C{idx}"));
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
                    let orig_key = k.strip_prefix("$C")
                        .and_then(|s| s.parse::<usize>().ok())
                        .and_then(|idx| keys.get(idx).cloned())
                        .unwrap_or_else(|| k.clone());
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
    fn cbor_domain_roundtrip() {
        let domain = CborDomain;
        
        // Create CBOR data
        let data_map: Vec<(ciborium::Value, ciborium::Value)> = vec![
            (ciborium::Value::Text("name".to_string()), ciborium::Value::Text("Alice".to_string())),
            (ciborium::Value::Text("age".to_string()), ciborium::Value::Integer(30.into())),
        ];
        let cbor_value = ciborium::Value::Map(data_map);
        
        let mut cbor_data = Vec::new();
        ciborium::into_writer(&cbor_value, &mut cbor_data).unwrap();

        let result = domain.extract(&cbor_data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        let orig: ciborium::Value = ciborium::from_reader(&cbor_data[..]).unwrap();
        let recon: ciborium::Value = ciborium::from_reader(&reconstructed[..]).unwrap();
        
        // Compare as JSON for easier equality check
        let orig_json = cbor_to_json(&orig).unwrap();
        let recon_json = cbor_to_json(&recon).unwrap();
        assert_eq!(orig_json, recon_json);
    }
}
