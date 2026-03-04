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
            .map_err(|e| CpacError::CompressFailed(format!("YAML decode: {e}")))?;

        // Simple key extraction from YAML (key: value patterns)
        let mut key_freq: HashMap<String, usize> = HashMap::new();

        for line in text.lines() {
            let trimmed = line.trim_start();
            // Skip comments and list-item markers.
            if trimmed.starts_with('#') || trimmed.starts_with('-') {
                continue;
            }
            if let Some(colon_pos) = trimmed.find(':') {
                let key = &trimmed[..colon_pos];
                if !key.is_empty() && !key.contains(' ') {
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

        let key_map: HashMap<String, u32> = repeated_keys
            .iter()
            .enumerate()
            .map(|(idx, (k, _))| (k.clone(), idx as u32))
            .collect();

        let residual = compact_yaml(text, &key_map);

        let mut fields = HashMap::new();
        fields.insert(
            "keys".to_string(),
            serde_json::Value::Array(
                repeated_keys
                    .iter()
                    .map(|(k, _)| serde_json::Value::String(k.clone()))
                    .collect(),
            ),
        );

        Ok(ExtractionResult {
            fields,
            residual: residual.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "text.yaml".to_string(),
        })
    }

    fn extract_with_fields(
        &self,
        data: &[u8],
        fields: &HashMap<String, serde_json::Value>,
    ) -> CpacResult<ExtractionResult> {
        // Use detection-phase key list for stable indices across streaming blocks.
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("YAML decode: {e}")))?;

        let keys: Vec<String> = match fields.get("keys") {
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => Vec::new(),
        };

        let key_map: HashMap<String, u32> = keys
            .iter()
            .enumerate()
            .map(|(i, k)| (k.clone(), i as u32))
            .collect();

        let residual = compact_yaml(text, &key_map);

        Ok(ExtractionResult {
            fields: fields.clone(),
            residual: residual.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "text.yaml".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let keys_value = result
            .fields
            .get("keys")
            .ok_or_else(|| CpacError::DecompressFailed("Missing keys".into()))?;

        let keys: Vec<String> = if let serde_json::Value::Array(arr) = keys_value {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid keys format".into()));
        };

        let compacted = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {e}")))?;

        Ok(expand_yaml(compacted, &keys).into_bytes())
    }
}

// ---------------------------------------------------------------------------
// Line-based helpers (no prefix-conflict issues)
// ---------------------------------------------------------------------------

/// Compact YAML key names using a stable index map.
///
/// Processes line-by-line; only replaces top-level keys (not list-item values
/// or comment lines) to avoid false-positive substitutions.
fn compact_yaml(text: &str, key_map: &HashMap<String, u32>) -> String {
    // Use split('\n') rather than lines() to preserve \r characters in CRLF
    // line endings and to naturally handle the trailing newline via the empty
    // final element that split produces (lines() strips it).
    let out: Vec<String> = text
        .split('\n')
        .map(|line| {
            // strip_suffix handles any trailing \r so key matching is clean,
            // but we keep the original `line` (with \r) for non-key lines.
            let trimmed = line.trim_start();
            // Key is always before the first colon; \r never appears there.
            let trimmed_clean = trimmed.trim_end_matches('\r');
            if trimmed_clean.starts_with('#') || trimmed_clean.starts_with('-') {
                return line.to_string();
            }
            if let Some(colon_pos) = trimmed_clean.find(':') {
                let key = &trimmed_clean[..colon_pos];
                if !key.is_empty() && !key.contains(' ') {
                    if let Some(&idx) = key_map.get(key) {
                        let indent_len = line.len() - trimmed.len();
                        let indent = &line[..indent_len];
                        // rest is from the original trimmed (preserves \r at end)
                        let rest = &trimmed[colon_pos..]; // includes colon + optional \r
                        return format!("{indent}@Y{idx}{rest}");
                    }
                }
            }
            line.to_string()
        })
        .collect();
    out.join("\n")
}

/// Expand `@Y{idx}` key placeholders back to original YAML keys.
fn expand_yaml(compacted: &str, keys: &[String]) -> String {
    // Use split('\n') to preserve \r in CRLF endings (same rationale as compact_yaml).
    let out: Vec<String> = compacted
        .split('\n')
        .map(|line| {
            let trimmed = line.trim_start();
            // Look for @Y at the start of the key position.
            // We match against the raw trimmed slice; any trailing \r is part of
            // the tail so it gets carried through to the reconstructed line.
            if let Some(rest) = trimmed.strip_prefix("@Y") {
                // Parse index up to the colon (colon always precedes any \r).
                if let Some(colon_pos) = rest.find(':') {
                    if let Ok(idx) = rest[..colon_pos].parse::<usize>() {
                        if let Some(key) = keys.get(idx) {
                            let indent_len = line.len() - trimmed.len();
                            let indent = &line[..indent_len];
                            let tail = &rest[colon_pos..]; // ":..." part, may include \r
                            return format!("{indent}{key}{tail}");
                        }
                    }
                }
            }
            line.to_string()
        })
        .collect();
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaml_domain_detection() {
        let domain = YamlDomain;
        let data =
            b"name: Alice\nage: 30\ncity: NYC\nname: Bob\nage: 25\ncity: LA\nstatus: active";
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

    /// Keys that are prefixes of other keys must not corrupt each other.
    #[test]
    fn yaml_no_prefix_conflict() {
        let domain = YamlDomain;
        // "name" is a prefix of "namespace"; the old global-replace approach
        // would corrupt "namespace" entries.
        let data = b"name: Alice\nnamespace: default\nname: Bob\nnamespace: prod\n";

        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }

    /// Streaming: extract_with_fields() uses stable detection-phase indices.
    #[test]
    fn yaml_streaming_consistent_indices() {
        let domain = YamlDomain;
        let block1 = b"name: Alice\nage: 30\nname: Bob\nage: 25\n";
        let block2 = b"name: Charlie\nage: 35\nname: Diana\nage: 28\n";

        let detection = domain.extract(block1).unwrap();

        let r1 = domain.extract_with_fields(block1, &detection.fields).unwrap();
        let r2 = domain.extract_with_fields(block2, &detection.fields).unwrap();

        assert_eq!(domain.reconstruct(&r1).unwrap(), block1.to_vec());
        assert_eq!(domain.reconstruct(&r2).unwrap(), block2.to_vec());
    }

    /// Two-block YAML streaming roundtrip.
    #[test]
    fn yaml_streaming_two_block_roundtrip() {
        let domain = YamlDomain;
        let block1 = b"host: server1\nport: 8080\nhost: server2\nport: 9090\n";
        let block2 = b"host: server3\nport: 7070\n";
        let original: Vec<u8> = [block1.as_slice(), block2.as_slice()].concat();

        let detection = domain.extract(block1).unwrap();
        let fields = detection.fields;

        let r1 = domain.extract_with_fields(block1, &fields).unwrap();
        let r2 = domain.extract_with_fields(block2, &fields).unwrap();

        let mut combined = domain.reconstruct(&r1).unwrap();
        combined.extend_from_slice(&domain.reconstruct(&r2).unwrap());

        assert_eq!(combined, original, "YAML two-block streaming roundtrip failed");
    }
}
