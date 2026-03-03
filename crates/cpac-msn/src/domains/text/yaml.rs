// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! YAML domain handler with key extraction.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;

/// YAML domain handler.
///
/// Extracts repeated keys from YAML documents.
/// Target compression: 15-40x on structured YAML.
pub struct YamlDomain;

impl Domain for YamlDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "text.yaml",
            name: "YAML",
            extensions: &[".yaml", ".yml"],
            mime_types: &["application/x-yaml", "text/yaml"],
            magic_bytes: &[],
        }
    }

    fn detect(&self, data: &[u8], filename: Option<&str>) -> f64 {
        if let Some(fname) = filename {
            if fname.ends_with(".yaml") || fname.ends_with(".yml") {
                return 0.9;
            }
        }

        let text = std::str::from_utf8(data).unwrap_or("");
        
        // Check for YAML patterns
        let has_yaml_markers = text.lines().take(20).filter(|line| {
            line.contains(':') || line.trim().starts_with('-') || line.contains("---")
        }).count() > 5;

        if has_yaml_markers {
            return 0.7;
        }

        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("YAML decode: {}", e)))?;

        // Simple key extraction from YAML (key: value patterns)
        let mut key_freq: HashMap<String, usize> = HashMap::new();
        
        for line in text.lines() {
            if let Some(colon_pos) = line.find(':') {
                let key = line[..colon_pos].trim();
                if !key.is_empty() && !key.starts_with('#') {
                    *key_freq.entry(key.to_string()).or_insert(0) += 1;
                }
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

        // Compact YAML by replacing keys
        let mut compacted = text.to_string();
        for (key, idx) in &key_map {
            let pattern = format!("{}:", key);
            let replacement = format!("@Y{}:", idx);
            compacted = compacted.replace(&pattern, &replacement);
        }

        let mut fields = HashMap::new();
        fields.insert("keys".to_string(), serde_json::Value::Array(
            repeated_keys.iter().map(|(k, _)| serde_json::Value::String(k.clone())).collect()
        ));

        Ok(ExtractionResult {
            fields,
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "text.yaml".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let keys_value = result.fields.get("keys")
            .ok_or_else(|| CpacError::DecompressFailed("Missing keys".into()))?;

        let keys: Vec<String> = if let serde_json::Value::Array(arr) = keys_value {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid keys format".into()));
        };

        let mut reconstructed = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {}", e)))?
            .to_string();

        // Expand placeholders
        for (idx, key) in keys.iter().enumerate() {
            let placeholder = format!("@Y{}:", idx);
            let original = format!("{}:", key);
            reconstructed = reconstructed.replace(&placeholder, &original);
        }

        Ok(reconstructed.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_domain_detection() {
        let domain = YamlDomain;
        // Need more lines with colons to trigger detection
        let data = b"name: Alice\nage: 30\ncity: NYC\nname: Bob\nage: 25\ncity: LA\nstatus: active";
        assert!(domain.detect(data, None) > 0.6);
        assert!(domain.detect(b"", Some("test.yaml")) > 0.8);
    }

    #[test]
    fn yaml_domain_roundtrip() {
        let domain = YamlDomain;
        let data = b"name: Alice\nage: 30\nname: Bob\nage: 25";

        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }
}
