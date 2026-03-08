// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! HTTP domain handler for HTTP request/response logs.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;

/// HTTP domain handler.
///
/// Extracts repeated headers and structure from HTTP logs.
/// Target compression: 15-30x on HTTP logs.
pub struct HttpDomain;

impl Domain for HttpDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "log.http",
            name: "HTTP Log",
            extensions: &[".http", ".httplog"],
            mime_types: &["text/plain"],
            magic_bytes: &[],
        }
    }

    fn detect(&self, data: &[u8], filename: Option<&str>) -> f64 {
        if let Some(fname) = filename {
            if fname.contains("http") || fname.ends_with(".httplog") {
                return 0.7;
            }
        }

        let Ok(text) = std::str::from_utf8(data) else {
            return 0.0;
        };

        // Check for HTTP request/response patterns
        let first_lines: Vec<&str> = text.lines().take(10).collect();
        let mut http_indicators = 0;

        for line in &first_lines {
            if line.starts_with("GET ")
                || line.starts_with("POST ")
                || line.starts_with("PUT ")
                || line.starts_with("DELETE ")
                || line.starts_with("HTTP/1")
                || line.starts_with("HTTP/2")
            {
                http_indicators += 1;
            }
            if line.contains("Host:")
                || line.contains("User-Agent:")
                || line.contains("Content-Type:")
                || line.contains("Content-Length:")
            {
                http_indicators += 1;
            }
        }

        if http_indicators >= 3 {
            return 0.75;
        } else if http_indicators >= 1 {
            return 0.4;
        }

        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("HTTP decode: {e}")))?;

        // Count occurrences of each "Header-Name:" token.
        let mut header_freq: HashMap<String, usize> = HashMap::new();
        for line in text.lines() {
            if let Some(colon_pos) = line.find(':') {
                let name = &line[..colon_pos];
                // Valid HTTP header names: letters, digits, hyphens only (no spaces).
                if !name.is_empty() && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '-') {
                    *header_freq.entry(name.to_string()).or_insert(0) += 1;
                }
            }
        }

        // Keep headers that appear ≥ 2 times; sort longest-first to avoid partial matches.
        let mut repeated: Vec<(String, usize)> = header_freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(b.1.cmp(&a.1)));

        // Build replacement map: "Header-Name:" → "@H{n}:"
        let mut header_map: HashMap<String, String> = HashMap::new();
        for (idx, (name, _)) in repeated.iter().enumerate() {
            header_map.insert(format!("{name}:"), format!("@H{idx}:"));
        }

        let mut compacted = text.to_string();
        for (orig, replacement) in &header_map {
            compacted = compacted.replace(orig.as_str(), replacement.as_str());
        }

        let mut fields = HashMap::new();
        fields.insert(
            "headers".to_string(),
            serde_json::Value::Array(
                repeated
                    .iter()
                    .map(|(n, _)| serde_json::Value::String(n.clone()))
                    .collect(),
            ),
        );

        Ok(ExtractionResult {
            fields,
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "log.http".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let headers: Vec<String> = result
            .fields
            .get("headers")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();

        if headers.is_empty() {
            return Ok(result.residual.clone());
        }

        let raw = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("HTTP decode: {e}")))?;
        let mut s = raw.to_string();
        // Expand placeholders in reverse index order so @H10 expands before @H1.
        for (idx, name) in headers.iter().enumerate().rev() {
            s = s.replace(&format!("@H{idx}:"), &format!("{name}:"));
        }
        Ok(s.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn http_domain_detection() {
        let domain = HttpDomain;
        let http_log = b"GET /index.html HTTP/1.1\nHost: example.com\nUser-Agent: Mozilla/5.0";
        assert!(domain.detect(http_log, None) > 0.6);
    }

    #[test]
    fn http_domain_roundtrip() {
        let domain = HttpDomain;
        let data = b"POST /api HTTP/1.1\nHost: api.example.com\nContent-Type: application/json\n\n{\"test\":1}";

        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }
}
