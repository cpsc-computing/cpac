// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! W3C Extended Log Format domain handler.
//!
//! Handles IIS and other W3C extended log files that begin with directive lines:
//!   `#Version: 1.0`
//!   `#Date: YYYY-MM-DD HH:MM:SS`
//!   `#Fields: date time c-ip cs-method cs-uri-stem sc-status ...`
//!
//! Data lines are space-separated, columns defined by the `#Fields:` directive.
//! Extraction: repeated tokens per column (cs-method, sc-status, c-ip) are
//! replaced with `@W{col}_{idx}` placeholders.
//! The `#Fields:` header and `#Version:`/`#Date:` directives are stored verbatim
//! in the fields metadata and stripped from the residual to save space.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;

const MIN_FREQUENCY: usize = 2;
const MIN_TOKEN_LEN: usize = 4;
const MIN_USEFUL_SIZE: usize = 16_384; // 16 KB

pub struct W3cLogDomain;

impl Domain for W3cLogDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "log.w3c",
            name: "W3C Extended Log",
            extensions: &[".log"],
            mime_types: &["text/plain"],
            magic_bytes: &[],
        }
    }

    fn detect(&self, data: &[u8], _filename: Option<&str>) -> f64 {
        let text = std::str::from_utf8(data).unwrap_or("");

        // W3C logs have #Version: and/or #Fields: directives.
        let has_version = text.lines().take(20).any(|l| l.starts_with("#Version:"));
        let has_fields = text.lines().take(20).any(|l| l.starts_with("#Fields:"));

        if has_fields {
            if data.len() >= MIN_USEFUL_SIZE {
                if has_version { 0.90 } else { 0.80 }
            } else {
                0.35
            }
        } else {
            0.0
        }
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("W3C log decode: {e}")))?;
        extract_w3c(text)
    }

    fn extract_with_fields(
        &self,
        data: &[u8],
        fields: &HashMap<String, serde_json::Value>,
    ) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("W3C log decode: {e}")))?;

        // Re-use column token tables from detection phase.
        let col_names = get_str_vec(fields, "col_names").unwrap_or_default();
        if col_names.is_empty() {
            return extract_w3c(text);
        }

        let compacted = compact_w3c(text, &col_names, fields);
        Ok(ExtractionResult {
            fields: fields.clone(),
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.w3c".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let col_names = get_str_vec(&result.fields, "col_names").unwrap_or_default();
        let directives = get_str_vec(&result.fields, "directives").unwrap_or_default();

        let mut reconstructed = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {e}")))?
            .to_string();

        // Expand column-value placeholders in reverse order.
        for (col_idx, _col_name) in col_names.iter().enumerate() {
            let key = format!("col_{col_idx}");
            let vals = get_str_vec(&result.fields, &key).unwrap_or_default();
            for (val_idx, val) in vals.iter().enumerate().rev() {
                reconstructed = reconstructed.replace(
                    &format!("@W{col_idx}_{val_idx}"),
                    val,
                );
            }
        }

        // Prepend directives that were stripped.
        if !directives.is_empty() {
            let dir_block: String = directives
                .iter()
                .map(|d| format!("{d}\n"))
                .collect();
            reconstructed = dir_block + &reconstructed;
        }

        Ok(reconstructed.into_bytes())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn extract_w3c(text: &str) -> CpacResult<ExtractionResult> {
    // Collect directive lines (#Version, #Date, #Fields, #Software).
    let mut directives: Vec<String> = Vec::new();
    let mut col_names: Vec<String> = Vec::new();
    let mut data_lines: Vec<&str> = Vec::new();

    for line in text.lines() {
        if line.starts_with('#') {
            directives.push(line.to_string());
            if let Some(field_list) = line.strip_prefix("#Fields:") {
                col_names = field_list
                    .split_whitespace()
                    .map(String::from)
                    .collect();
            }
        } else if !line.trim().is_empty() {
            data_lines.push(line);
        }
    }

    if col_names.is_empty() || data_lines.is_empty() {
        // No column definition — passthrough.
        return Ok(ExtractionResult {
            fields: HashMap::new(),
            residual: text.as_bytes().to_vec(),
            metadata: HashMap::new(),
            domain_id: "log.w3c".to_string(),
        });
    }

    // Build per-column frequency tables.
    let num_cols = col_names.len();
    let mut col_freq: Vec<HashMap<String, usize>> = vec![HashMap::new(); num_cols];

    for line in &data_lines {
        let tokens: Vec<&str> = line.split(' ').collect();
        for (ci, tok) in tokens.iter().enumerate().take(num_cols) {
            if tok.len() >= MIN_TOKEN_LEN && *tok != "-" {
                *col_freq[ci].entry(tok.to_string()).or_insert(0) += 1;
            }
        }
    }

    // Keep only repeated values per column; build replacement maps.
    let mut col_vals: Vec<Vec<String>> = Vec::with_capacity(num_cols);
    let mut col_maps: Vec<HashMap<String, String>> = Vec::with_capacity(num_cols);

    for (ci, freq) in col_freq.into_iter().enumerate() {
        let mut repeated: Vec<(String, usize)> = freq
            .into_iter()
            .filter(|(v, count)| *count >= MIN_FREQUENCY && v.len() >= MIN_TOKEN_LEN)
            .collect();
        repeated.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));

        let mut map = HashMap::new();
        for (vi, (val, _)) in repeated.iter().enumerate() {
            map.insert(val.clone(), format!("@W{ci}_{vi}"));
        }
        col_vals.push(repeated.into_iter().map(|(v, _)| v).collect());
        col_maps.push(map);
    }

    // Compact data lines using per-column maps.
    let mut compacted_lines: Vec<String> = Vec::with_capacity(data_lines.len());
    for line in &data_lines {
        let tokens: Vec<&str> = line.split(' ').collect();
        let new_tokens: Vec<String> = tokens
            .iter()
            .enumerate()
            .map(|(ci, tok)| {
                if ci < num_cols {
                    col_maps[ci]
                        .get(*tok)
                        .cloned()
                        .unwrap_or_else(|| tok.to_string())
                } else {
                    tok.to_string()
                }
            })
            .collect();
        compacted_lines.push(new_tokens.join(" "));
    }

    let has_trailing_newline = text.ends_with('\n');
    let mut residual = compacted_lines.join("\n");
    if has_trailing_newline {
        residual.push('\n');
    }

    let mut fields = HashMap::new();
    fields.insert(
        "directives".to_string(),
        serde_json::Value::Array(
            directives
                .iter()
                .map(|d| serde_json::Value::String(d.clone()))
                .collect(),
        ),
    );
    fields.insert(
        "col_names".to_string(),
        serde_json::Value::Array(
            col_names
                .iter()
                .map(|c| serde_json::Value::String(c.clone()))
                .collect(),
        ),
    );
    for (ci, vals) in col_vals.iter().enumerate() {
        fields.insert(
            format!("col_{ci}"),
            serde_json::Value::Array(
                vals.iter()
                    .map(|v| serde_json::Value::String(v.clone()))
                    .collect(),
            ),
        );
    }

    Ok(ExtractionResult {
        fields,
        residual: residual.into_bytes(),
        metadata: HashMap::new(),
        domain_id: "log.w3c".to_string(),
    })
}

fn compact_w3c(
    text: &str,
    col_names: &[String],
    fields: &HashMap<String, serde_json::Value>,
) -> String {
    let num_cols = col_names.len();

    // Rebuild per-column maps from stored fields.
    let mut col_maps: Vec<HashMap<String, String>> = Vec::with_capacity(num_cols);
    for ci in 0..num_cols {
        let key = format!("col_{ci}");
        let vals = get_str_vec(fields, &key).unwrap_or_default();
        let map: HashMap<String, String> = vals
            .iter()
            .enumerate()
            .map(|(vi, v)| (v.clone(), format!("@W{ci}_{vi}")))
            .collect();
        col_maps.push(map);
    }

    let mut out_lines: Vec<String> = Vec::new();
    for line in text.lines() {
        if line.starts_with('#') || line.trim().is_empty() {
            continue; // directives already stored separately
        }
        let tokens: Vec<&str> = line.split(' ').collect();
        let new_tokens: Vec<String> = tokens
            .iter()
            .enumerate()
            .map(|(ci, tok)| {
                if ci < num_cols {
                    col_maps[ci]
                        .get(*tok)
                        .cloned()
                        .unwrap_or_else(|| tok.to_string())
                } else {
                    tok.to_string()
                }
            })
            .collect();
        out_lines.push(new_tokens.join(" "));
    }

    let has_trailing = text.ends_with('\n');
    let mut result = out_lines.join("\n");
    if has_trailing {
        result.push('\n');
    }
    result
}

fn get_str_vec(
    fields: &HashMap<String, serde_json::Value>,
    key: &str,
) -> Option<Vec<String>> {
    fields.get(key).and_then(|v| v.as_array()).map(|arr| {
        arr.iter()
            .filter_map(|v| v.as_str().map(String::from))
            .collect()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &[u8] = b"#Version: 1.0\n\
#Date: 1998-11-19 22:48:39\n\
#Fields: date time c-ip cs-method cs-uri-stem sc-status sc-bytes\n\
1998-11-19 22:48:39 192.168.1.1 GET /index.html 200 2943\n\
1998-11-19 22:48:40 192.168.1.2 GET /about.html 200 1234\n\
1998-11-19 22:48:41 192.168.1.1 POST /submit 200 512\n\
1998-11-19 22:48:42 192.168.1.1 GET /index.html 200 2943\n";

    #[test]
    fn w3c_detect() {
        let domain = W3cLogDomain;
        let data: Vec<u8> = SAMPLE.iter().copied().cycle().take(20_000).collect();
        let conf = domain.detect(&data, None);
        assert!(conf >= 0.8, "W3C confidence={conf}");
    }

    #[test]
    fn w3c_roundtrip() {
        let domain = W3cLogDomain;
        let result = domain.extract(SAMPLE).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(SAMPLE, reconstructed.as_slice());
    }
}
