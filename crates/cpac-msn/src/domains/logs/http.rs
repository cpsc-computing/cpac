// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
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

        let text = match std::str::from_utf8(data) {
            Ok(t) => t,
            Err(_) => return 0.0,
        };

        // Check for HTTP request/response patterns
        let first_lines: Vec<&str> = text.lines().take(10).collect();
        let mut http_indicators = 0;

        for line in &first_lines {
            if line.starts_with("GET ") || line.starts_with("POST ") ||
               line.starts_with("PUT ") || line.starts_with("DELETE ") ||
               line.starts_with("HTTP/1") || line.starts_with("HTTP/2") {
                http_indicators += 1;
            }
            if line.contains("Host:") || line.contains("User-Agent:") ||
               line.contains("Content-Type:") || line.contains("Content-Length:") {
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
            .map_err(|e| CpacError::CompressFailed(format!("HTTP decode: {}", e)))?;

        // Extract common HTTP headers
        let mut common_headers = HashMap::new();
        let lines: Vec<&str> = text.lines().collect();
        
        // Count header occurrences
        let mut header_counts: HashMap<String, usize> = HashMap::new();
        for line in &lines {
            if let Some(colon_pos) = line.find(':') {
                let header = line[..colon_pos].trim().to_lowercase();
                *header_counts.entry(header).or_insert(0) += 1;
            }
        }

        // Store headers that appear more than twice
        for (header, count) in header_counts {
            if count > 2 {
                common_headers.insert(header.clone(), serde_json::Value::Number(count.into()));
            }
        }

        let mut fields = HashMap::new();
        if !common_headers.is_empty() {
            fields.insert("common_headers".to_string(), serde_json::Value::Object(
                common_headers.into_iter()
                    .collect()
            ));
        }

        Ok(ExtractionResult {
            fields,
            residual: data.to_vec(),
            metadata: HashMap::new(),
            domain_id: "log.http".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        // Simple passthrough - HTTP structure is complex
        Ok(result.residual.clone())
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
