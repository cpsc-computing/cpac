// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! JSON domain handler with semantic field extraction.
//!
//! Supports both single-document JSON and JSONL (newline-delimited JSON).

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
    /// Build a (field_map, field_names) pair from field-count data.
    fn build_field_index(field_counts: &HashMap<String, u32>) -> (HashMap<String, u32>, Vec<String>) {
        let mut repeated: Vec<String> = field_counts
            .iter()
            .filter(|(_, &c)| c >= 2)
            .map(|(n, _)| n.clone())
            .collect();
        repeated.sort();
        let mut map = HashMap::new();
        for (idx, name) in repeated.iter().enumerate() {
            map.insert(name.clone(), idx as u32);
        }
        (map, repeated)
    }

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
                        format!("${idx}")
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
                    let new_key = key.strip_prefix('$')
                        .and_then(|s| s.parse::<usize>().ok())
                        .and_then(|idx| field_names.get(idx).cloned())
                        .unwrap_or_else(|| key.clone());
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

    /// Internal: extract a single-document JSON block.
    fn extract_single(&self, data: &[u8], value: &Value) -> CpacResult<ExtractionResult> {
        let mut all_field_names = Vec::new();
        Self::extract_field_names(value, &mut all_field_names);
        let mut field_counts: HashMap<String, u32> = HashMap::new();
        for name in &all_field_names {
            *field_counts.entry(name.clone()).or_insert(0) += 1;
        }
        let (field_map, repeated_fields) = Self::build_field_index(&field_counts);
        let compacted = Self::compact_json(value, &field_map);
        let residual = serde_json::to_vec(&compacted)
            .map_err(|e| CpacError::CompressFailed(format!("text.json serialize: {e}")))?;
        let mut fields = HashMap::new();
        fields.insert(
            "field_names".to_string(),
            Value::Array(repeated_fields.into_iter().map(Value::String).collect()),
        );
        fields.insert("original_size".to_string(), Value::Number(data.len().into()));
        Ok(ExtractionResult { fields, residual, metadata: HashMap::new(), domain_id: "text.json".to_string() })
    }

    /// Internal: extract JSONL (newline-delimited JSON) blocks.
    ///
    /// Strict: any non-empty line that fails JSON parsing causes this to return
    /// an error, which callers interpret as "not JSONL, use passthrough".
    fn extract_jsonl(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        // Parse every non-empty line; fail on the first invalid line.
        let lines: Vec<Value> = data
            .split(|&b| b == b'\n')
            .enumerate()
            .filter_map(|(i, raw)| {
                let l = raw.strip_suffix(b"\r").unwrap_or(raw);
                if l.is_empty() {
                    None
                } else {
                    Some(serde_json::from_slice::<Value>(l).map_err(|e| {
                        CpacError::CompressFailed(format!("text.json: JSONL line {i} invalid: {e}"))
                    }))
                }
            })
            .collect::<CpacResult<Vec<Value>>>()?;
        if lines.is_empty() {
            return Err(CpacError::CompressFailed(
                "text.json: data is not valid JSON or JSONL".into(),
            ));
        }
        // Collect field counts across all lines.
        let mut field_counts: HashMap<String, u32> = HashMap::new();
        for val in &lines {
            let mut names = Vec::new();
            Self::extract_field_names(val, &mut names);
            for n in names { *field_counts.entry(n).or_insert(0) += 1; }
        }
        let (field_map, repeated_fields) = Self::build_field_index(&field_counts);
        // Compact each line.
        let mut compacted_lines: Vec<u8> = Vec::with_capacity(data.len());
        for val in &lines {
            let compacted = Self::compact_json(val, &field_map);
            let bytes = serde_json::to_vec(&compacted)
                .map_err(|e| CpacError::CompressFailed(format!("text.json JSONL serialize: {e}")))?;
            compacted_lines.extend_from_slice(&bytes);
            compacted_lines.push(b'\n');
        }
        let mut fields = HashMap::new();
        fields.insert(
            "field_names".to_string(),
            Value::Array(repeated_fields.into_iter().map(Value::String).collect()),
        );
        fields.insert("format".to_string(), Value::String("jsonl".to_string()));
        fields.insert("original_size".to_string(), Value::Number(data.len().into()));
        // Track whether original data ended with a newline for byte-exact roundtrip.
        let trailing_newline = data.last() == Some(&b'\n');
        fields.insert("trailing_newline".to_string(), Value::Bool(trailing_newline));
        Ok(ExtractionResult {
            fields,
            residual: compacted_lines,
            metadata: HashMap::new(),
            domain_id: "text.json".to_string(),
        })
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
        if let Some(fname) = filename {
            if fname.ends_with(".json") { return 0.9; }
            if fname.ends_with(".jsonl") { return 0.95; }
        }

        // Use memchr2 to find the first structural JSON byte ({  or [),
        // skipping any leading ASCII whitespace without a per-byte branch loop.
        let start = data.iter().position(|b| !b.is_ascii_whitespace()).unwrap_or(data.len());
        if start >= data.len() {
            return 0.0;
        }
        let first_byte = data[start];
        if first_byte != b'{' && first_byte != b'[' {
            return 0.0;
        }

        // Try full single-document parse first.
        if serde_json::from_slice::<Value>(data).is_ok() {
            return 0.95;
        }

        // Check if it's JSONL: first line must be a valid JSON object.
        let nl = memchr::memchr(b'\n', data).unwrap_or(data.len());
        let first_line = data[start..nl].strip_suffix(b"\r").unwrap_or(&data[start..nl]);
        if !first_line.is_empty() && serde_json::from_slice::<Value>(first_line).is_ok() {
            return 0.85; // Likely JSONL
        }

        0.6 // Magic bytes match but couldn't fully parse
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        // Try single-document JSON first.
        if let Ok(value) = serde_json::from_slice::<Value>(data) {
            return self.extract_single(data, &value);
        }

        // Fall back to JSONL (newline-delimited JSON objects).
        self.extract_jsonl(data)
    }

    fn extract_with_fields(
        &self,
        data: &[u8],
        fields: &HashMap<String, serde_json::Value>,
    ) -> CpacResult<ExtractionResult> {
        let field_names_value = fields.get("field_names")
            .ok_or_else(|| CpacError::CompressFailed("text.json: missing field_names in metadata".into()))?;

        let field_names: Vec<String> = if let Value::Array(arr) = field_names_value {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else {
            return Err(CpacError::CompressFailed("text.json: invalid field_names format".into()));
        };

        let is_jsonl = fields.get("format").and_then(Value::as_str) == Some("jsonl");

        if is_jsonl {
            // Build consistent field map and compact each line.
            let mut field_map = HashMap::new();
            for (idx, name) in field_names.iter().enumerate() {
                field_map.insert(name.clone(), idx as u32);
            }
            let mut compacted_lines: Vec<u8> = Vec::new();
            for raw_line in data.split(|&b| b == b'\n') {
                let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
                if line.is_empty() { continue; }
                let val: Value = serde_json::from_slice(line)
                    .map_err(|e| CpacError::CompressFailed(format!("text.json JSONL line parse: {e}")))?;
                let compacted = Self::compact_json(&val, &field_map);
                let bytes = serde_json::to_vec(&compacted)
                    .map_err(|e| CpacError::CompressFailed(format!("text.json JSONL serialize: {e}")))?;
                compacted_lines.extend_from_slice(&bytes);
                compacted_lines.push(b'\n');
            }
            let mut result_fields = fields.clone();
            result_fields.insert("original_size".to_string(), Value::Number(data.len().into()));
            return Ok(ExtractionResult {
                fields: result_fields,
                residual: compacted_lines,
                metadata: HashMap::new(),
                domain_id: "text.json".to_string(),
            });
        }

        // Single-document JSON path.
        let value: Value = serde_json::from_slice(data)
            .map_err(|e| CpacError::CompressFailed(format!("text.json parse error: {e}")))?;
        let mut field_map = HashMap::new();
        for (idx, name) in field_names.iter().enumerate() {
            field_map.insert(name.clone(), idx as u32);
        }
        let compacted = Self::compact_json(&value, &field_map);
        let residual = serde_json::to_vec(&compacted)
            .map_err(|e| CpacError::CompressFailed(format!("text.json serialize error: {e}")))?;
        let mut result_fields = HashMap::new();
        result_fields.insert("field_names".to_string(), field_names_value.clone());
        result_fields.insert("original_size".to_string(), Value::Number(data.len().into()));
        Ok(ExtractionResult {
            fields: result_fields,
            residual,
            metadata: HashMap::new(),
            domain_id: "text.json".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let field_names_value = result.fields.get("field_names")
            .ok_or_else(|| CpacError::DecompressFailed("text.json: missing field_names".into()))?;
        let field_names: Vec<String> = if let Value::Array(arr) = field_names_value {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else {
            return Err(CpacError::DecompressFailed("text.json: invalid field_names format".into()));
        };

        let is_jsonl = result.fields.get("format").and_then(Value::as_str) == Some("jsonl");
        if is_jsonl {
            let trailing_newline = result.fields.get("trailing_newline")
                .and_then(Value::as_bool)
                .unwrap_or(true); // default true for backward compat
            let mut output: Vec<u8> = Vec::with_capacity(result.residual.len());
            for raw_line in result.residual.split(|&b| b == b'\n') {
                let line = raw_line.strip_suffix(b"\r").unwrap_or(raw_line);
                if line.is_empty() { continue; }
                let compacted: Value = serde_json::from_slice(line)
                    .map_err(|e| CpacError::DecompressFailed(format!("text.json JSONL parse: {e}")))?;
                let expanded = Self::expand_json(&compacted, &field_names);
                let bytes = serde_json::to_vec(&expanded)
                    .map_err(|e| CpacError::DecompressFailed(format!("text.json JSONL serialize: {e}")))?;
                output.extend_from_slice(&bytes);
                output.push(b'\n');
            }
            // Restore exact trailing-newline behaviour of the original input.
            if !trailing_newline && output.last() == Some(&b'\n') {
                output.pop();
            }
            return Ok(output);
        }

        // Single-document JSON path.
        let compacted: Value = serde_json::from_slice(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("text.json parse error: {e}")))?;
        let expanded = Self::expand_json(&compacted, &field_names);
        serde_json::to_vec(&expanded)
            .map_err(|e| CpacError::DecompressFailed(format!("text.json serialize error: {e}")))
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
    fn json_domain_jsonl_detection() {
        let domain = JsonDomain;
        // Filename-based JSONL detection
        assert!(domain.detect(b"", Some("events.jsonl")) > 0.9);
        // Content-based JSONL detection (first line is valid JSON object, whole is not)
        let jsonl = b"{\"a\":1}\n{\"a\":2}\n{\"a\":3}\n";
        let confidence = domain.detect(jsonl, None);
        assert!(confidence > 0.7, "expected > 0.7, got {confidence}");
    }

    #[test]
    fn json_domain_jsonl_roundtrip() {
        let domain = JsonDomain;
        let data = b"{\"user\":\"alice\",\"score\":100}\n{\"user\":\"bob\",\"score\":200}\n{\"user\":\"charlie\",\"score\":150}\n";

        let result = domain.extract(data).unwrap();

        // Should detect JSONL format
        assert_eq!(result.fields.get("format").and_then(serde_json::Value::as_str), Some("jsonl"));
        // Residual should be smaller (field names extracted)
        assert!(result.residual.len() < data.len(), "residual {} >= original {}", result.residual.len(), data.len());

        let reconstructed = domain.reconstruct(&result).unwrap();
        // Compare parsed objects line by line
        let orig_lines: Vec<serde_json::Value> = data.split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).unwrap())
            .collect();
        let recon_lines: Vec<serde_json::Value> = reconstructed.split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).unwrap())
            .collect();
        assert_eq!(orig_lines, recon_lines);
    }

    #[test]
    fn json_domain_jsonl_extract_with_fields() {
        let domain = JsonDomain;
        // Two-record JSONL blocks so extract() takes the JSONL path (not single-doc).
        let block1 = b"{\"ts\":\"2026-01-01\",\"level\":\"INFO\",\"msg\":\"start\"}\n\
{\"ts\":\"2026-01-01\",\"level\":\"DEBUG\",\"msg\":\"init\"}\n";
        let block2 = b"{\"ts\":\"2026-01-02\",\"level\":\"WARN\",\"msg\":\"slow\"}\n\
{\"ts\":\"2026-01-02\",\"level\":\"ERROR\",\"msg\":\"fail\"}\n";

        let detection = domain.extract(block1).unwrap();
        assert_eq!(
            detection.fields.get("format").and_then(serde_json::Value::as_str),
            Some("jsonl"),
            "expected JSONL format to be detected"
        );

        // Apply same field map to block2
        let result2 = domain.extract_with_fields(block2, &detection.fields).unwrap();
        let recon2 = domain.reconstruct(&result2).unwrap();

        // Compare line-by-line as parsed JSON
        let orig_lines: Vec<serde_json::Value> = block2
            .split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).unwrap())
            .collect();
        let recon_lines: Vec<serde_json::Value> = recon2
            .split(|&b| b == b'\n')
            .filter(|l| !l.is_empty())
            .map(|l| serde_json::from_slice(l).unwrap())
            .collect();
        assert_eq!(orig_lines, recon_lines);
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
