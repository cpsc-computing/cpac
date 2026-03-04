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
            .map_err(|e| CpacError::CompressFailed(format!("JSON log decode: {e}")))?;

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

        let has_trailing_newline = text.ends_with('\n');

        let mut fields = HashMap::new();
        fields.insert("keys".to_string(), Value::Array(
            repeated_keys.iter().map(|(k, _)| Value::String(k.clone())).collect()
        ));
        fields.insert("trailing_newline".to_string(), Value::Bool(has_trailing_newline));

        Ok(ExtractionResult {
            fields,
            residual: compacted_lines.join("\n").into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.json".to_string(),
        })
    }

    fn extract_with_fields(
        &self,
        data: &[u8],
        fields: &HashMap<String, Value>,
    ) -> CpacResult<ExtractionResult> {
        // Line-aligned streaming extraction.
        //
        // Blocks can split mid-JSON-line; we process only the complete lines
        // (up to and including the last '\n') and carry the incomplete tail as
        // a verbatim suffix.  The residual wire format is:
        //
        //   [0x01 marker (1B)] [suffix_len: u32 LE (4B)] [compacted_lines...] [suffix...]
        //
        // reconstruct() detects the 0x01 prefix and uses the streaming path.

        // Get the key list from the detection-phase metadata.
        let keys: Vec<String> = match fields.get("keys") {
            Some(Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => Vec::new(),
        };

        // Build key → compact-index map.
        let key_map: HashMap<String, u32> = keys
            .iter()
            .enumerate()
            .map(|(idx, k)| (k.clone(), idx as u32))
            .collect();

        // Split at the last '\n' so we only process complete lines.
        let split_pos = data
            .iter()
            .rposition(|&b| b == b'\n')
            .map_or(0, |p| p + 1);
        let complete_data = &data[..split_pos];
        let suffix = &data[split_pos..];

        // Compact complete lines.
        let mut compacted_str = String::new();
        if !complete_data.is_empty() {
            let text = std::str::from_utf8(complete_data)
                .map_err(|e| CpacError::CompressFailed(format!("JSON log decode: {e}")))?;
            let has_trailing_newline = complete_data.ends_with(b"\n");

            let mut compacted_lines: Vec<String> = Vec::new();
            for line in text.lines() {
                if let Ok(value) = serde_json::from_str::<Value>(line) {
                    let compacted = compact_value(&value, &key_map);
                    if let Ok(s) = serde_json::to_string(&compacted) {
                        compacted_lines.push(s);
                    } else {
                        compacted_lines.push(line.to_string());
                    }
                } else {
                    compacted_lines.push(line.to_string());
                }
            }

            compacted_str = compacted_lines.join("\n");
            if has_trailing_newline {
                compacted_str.push('\n');
            }
        }

        // Encode streaming residual: [0x01][suffix_len: u32 LE][compacted][suffix]
        let compacted_bytes = compacted_str.as_bytes();
        let suffix_len = suffix.len() as u32;
        let mut residual = Vec::with_capacity(5 + compacted_bytes.len() + suffix.len());
        residual.push(0x01u8);
        residual.extend_from_slice(&suffix_len.to_le_bytes());
        residual.extend_from_slice(compacted_bytes);
        residual.extend_from_slice(suffix);

        Ok(ExtractionResult {
            fields: fields.clone(),
            residual,
            metadata: HashMap::new(),
            domain_id: "log.json".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        // Streaming path: residual starts with the 0x01 marker byte.
        if result.residual.first() == Some(&0x01u8) {
            return reconstruct_streaming_json_log(result);
        }

        // --- Legacy (non-streaming) path ---
        let keys_value = result.fields.get("keys")
            .ok_or_else(|| CpacError::DecompressFailed("Missing keys".into()))?;

        let keys: Vec<String> = if let Value::Array(arr) = keys_value {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid keys format".into()));
        };

        let text = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {e}")))?;

        let has_trailing_newline = result.fields.get("trailing_newline")
            .and_then(serde_json::Value::as_bool)
            .unwrap_or(false);

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

        let mut reconstructed = reconstructed_lines.join("\n");
        if has_trailing_newline {
            reconstructed.push('\n');
        }

        Ok(reconstructed.into_bytes())
    }
}

/// Reconstruct streaming JSON-log blocks (residual with 0x01 prefix).
fn reconstruct_streaming_json_log(result: &ExtractionResult) -> CpacResult<Vec<u8>> {
    // Residual layout: [0x01 (1B)][suffix_len: u32 LE (4B)][compacted_bytes][suffix_bytes]
    if result.residual.len() < 5 {
        return Err(CpacError::DecompressFailed(
            "streaming JSON log residual too short".into(),
        ));
    }

    let suffix_len = u32::from_le_bytes([
        result.residual[1],
        result.residual[2],
        result.residual[3],
        result.residual[4],
    ]) as usize;

    if result.residual.len() < 5 + suffix_len {
        return Err(CpacError::DecompressFailed(
            "streaming JSON log residual truncated".into(),
        ));
    }

    let payload_end = result.residual.len() - suffix_len;
    let compacted_bytes = &result.residual[5..payload_end];
    let suffix = &result.residual[payload_end..];

    // Get keys from the stored metadata fields.
    let keys: Vec<String> = match result.fields.get("keys") {
        Some(Value::Array(arr)) => arr
            .iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect(),
        _ => Vec::new(),
    };

    // No complete lines — the entire block was an incomplete fragment.
    if compacted_bytes.is_empty() {
        return Ok(suffix.to_vec());
    }

    let has_trailing_nl = compacted_bytes.ends_with(b"\n");
    let text = std::str::from_utf8(compacted_bytes)
        .map_err(|e| CpacError::DecompressFailed(format!("streaming UTF-8 decode: {e}")))?;

    let mut reconstructed_lines: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Ok(value) = serde_json::from_str::<Value>(line) {
            let expanded = expand_value(&value, &keys);
            if let Ok(s) = serde_json::to_string(&expanded) {
                reconstructed_lines.push(s);
            } else {
                reconstructed_lines.push(line.to_string());
            }
        } else {
            reconstructed_lines.push(line.to_string());
        }
    }

    let mut output = reconstructed_lines.join("\n").into_bytes();
    if has_trailing_nl {
        output.push(b'\n');
    }
    output.extend_from_slice(suffix);
    Ok(output)
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
                    let new_key = key_map.get(k).map_or_else(|| k.clone(), |idx| format!("$L{idx}"));
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
                    let orig_key = k.strip_prefix("$L")
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

    /// Streaming: block ends exactly on a line boundary (no suffix).
    #[test]
    fn json_log_streaming_no_suffix() {
        let domain = JsonLogDomain;
        let data = b"{\"level\":\"info\",\"msg\":\"a\"}\n{\"level\":\"warn\",\"msg\":\"b\"}\n";

        // Build detection-phase fields from extract()
        let detection = domain.extract(data).unwrap();
        let result = domain.extract_with_fields(data, &detection.fields).unwrap();

        // Residual must start with 0x01 marker, suffix_len should be 0
        assert_eq!(result.residual[0], 0x01);
        let suffix_len = u32::from_le_bytes([
            result.residual[1], result.residual[2],
            result.residual[3], result.residual[4],
        ]);
        assert_eq!(suffix_len, 0);

        // Reconstruct and verify byte-exact output
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(reconstructed, data.to_vec());
    }

    /// Streaming: block ends mid-line (suffix present).
    #[test]
    fn json_log_streaming_with_suffix() {
        let domain = JsonLogDomain;
        // Block ends mid-JSON-line after a complete line
        let complete_part = b"{\"level\":\"info\",\"msg\":\"done\"}\n";
        let incomplete_part = b"{\"level\":\"error\",\"msg\":\"par";
        let data: Vec<u8> = [complete_part.as_slice(), incomplete_part.as_slice()].concat();

        let detection = domain.extract(complete_part).unwrap();
        let result = domain.extract_with_fields(&data, &detection.fields).unwrap();

        // Residual starts with 0x01; suffix_len == incomplete_part.len()
        assert_eq!(result.residual[0], 0x01);
        let suffix_len = u32::from_le_bytes([
            result.residual[1], result.residual[2],
            result.residual[3], result.residual[4],
        ]) as usize;
        assert_eq!(suffix_len, incomplete_part.len());

        // Reconstruct: should restore original block byte-for-byte
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(reconstructed, data);
    }

    /// Streaming: block has NO newlines at all (entire block is a partial line).
    #[test]
    fn json_log_streaming_all_suffix() {
        let domain = JsonLogDomain;
        let data = b"{\"level\":\"info\",\"msg\":\"no newline here";

        let detection_fields: HashMap<String, Value> = [("keys".to_string(), Value::Array(vec![]))]
            .into_iter()
            .collect();
        let result = domain.extract_with_fields(data, &detection_fields).unwrap();

        // suffix_len == data.len()
        let suffix_len = u32::from_le_bytes([
            result.residual[1], result.residual[2],
            result.residual[3], result.residual[4],
        ]) as usize;
        assert_eq!(suffix_len, data.len());

        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(reconstructed, data.to_vec());
    }

    /// Simulate two adjacent streaming blocks at a mid-line boundary and verify
    /// that concatenating their reconstructions reproduces the original content.
    #[test]
    fn json_log_streaming_cross_block_roundtrip() {
        let domain = JsonLogDomain;

        let line_a = b"{\"level\":\"info\",\"msg\":\"alpha\"}\n";
        let line_b = b"{\"level\":\"warn\",\"msg\":\"beta\"}\n";
        let line_c = b"{\"level\":\"error\",\"msg\":\"gamma\"}\n";

        // Original (3 complete lines)
        let original: Vec<u8> = [line_a.as_slice(), line_b.as_slice(), line_c.as_slice()].concat();

        // Block 1: first line + half of second line
        let split = line_a.len() + line_b.len() / 2;
        let block1 = &original[..split];
        let block2 = &original[split..];

        // Detection from block1 (complete part only for consistent keys)
        let detection = domain.extract(line_a).unwrap();
        let fields = detection.fields;

        let result1 = domain.extract_with_fields(block1, &fields).unwrap();
        let result2 = domain.extract_with_fields(block2, &fields).unwrap();

        let recon1 = domain.reconstruct(&result1).unwrap();
        let recon2 = domain.reconstruct(&result2).unwrap();

        let mut combined = recon1;
        combined.extend_from_slice(&recon2);

        assert_eq!(combined, original, "cross-block concatenation must equal original");
    }
}
