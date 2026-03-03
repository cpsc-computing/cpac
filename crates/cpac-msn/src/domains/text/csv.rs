// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! CSV domain handler with header/column extraction.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;

/// CSV domain handler.
///
/// Extracts column headers and structure from CSV data.
/// Target compression: 20-50x on structured CSV.
pub struct CsvDomain;

impl Domain for CsvDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "text.csv",
            name: "CSV",
            extensions: &[".csv", ".tsv"],
            mime_types: &["text/csv", "text/tab-separated-values"],
            magic_bytes: &[],
        }
    }

    fn detect(&self, data: &[u8], filename: Option<&str>) -> f64 {
        if let Some(fname) = filename {
            if fname.ends_with(".csv") {
                return 0.9;
            }
            if fname.ends_with(".tsv") {
                return 0.85;
            }
        }

        // Check if first line looks like CSV header
        let first_line = data.iter().take_while(|&&b| b != b'\n').copied().collect::<Vec<_>>();
        if first_line.is_empty() {
            return 0.0;
        }

        let comma_count = first_line.iter().filter(|&&b| b == b',').count();
        if comma_count >= 1 {
            // Check if subsequent lines exist
            let newline_count = data.iter().filter(|&&b| b == b'\n').count();
            if newline_count >= 1 {
                return 0.7;
            }
        }

        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("CSV decode: {}", e)))?;

        // Detect line ending style
        let line_ending = if text.contains("\r\n") {
            "\r\n"
        } else {
            "\n"
        };

        let mut lines = text.lines();
        let header = lines.next()
            .ok_or_else(|| CpacError::CompressFailed("Empty CSV".into()))?;

        // Extract headers
        let headers: Vec<&str> = header.split(',').map(|h| h.trim()).collect();

        // Store body without header, preserving line ending
        let body = lines.collect::<Vec<_>>().join(line_ending);
        let has_trailing_newline = text.ends_with('\n');

        let mut fields = HashMap::new();
        fields.insert("headers".to_string(), serde_json::Value::Array(
            headers.iter().map(|h| serde_json::Value::String(h.to_string())).collect()
        ));
        fields.insert("trailing_newline".to_string(), serde_json::Value::Bool(has_trailing_newline));
        fields.insert("line_ending".to_string(), serde_json::Value::String(line_ending.to_string()));

        Ok(ExtractionResult {
            fields,
            residual: body.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "text.csv".to_string(),
        })
    }

    fn extract_with_fields(
        &self,
        _data: &[u8],
        _fields: &HashMap<String, serde_json::Value>,
    ) -> CpacResult<ExtractionResult> {
        // For streaming: CSV blocks after the first won't have headers
        // Just pass through the data as-is rather than trying to extract non-existent headers
        // The detection-phase metadata has the headers, and they'll be added during reconstruction
        Err(CpacError::CompressFailed(
            "CSV blocks without headers not supported for streaming".into()
        ))
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let headers_value = result.fields.get("headers")
            .ok_or_else(|| CpacError::DecompressFailed("Missing headers".into()))?;

        let headers: Vec<String> = if let serde_json::Value::Array(arr) = headers_value {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid headers format".into()));
        };

        let has_trailing_newline = result.fields.get("trailing_newline")
            .and_then(|v| v.as_bool())
            .unwrap_or(false);

        let line_ending = result.fields.get("line_ending")
            .and_then(|v| v.as_str())
            .unwrap_or("\n");

        let header_line = headers.join(",");
        let body = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {}", e)))?;

        let reconstructed = if body.is_empty() {
            if has_trailing_newline {
                format!("{}{}", header_line, line_ending)
            } else {
                header_line
            }
        } else {
            if has_trailing_newline {
                format!("{}{}{}{}", header_line, line_ending, body, line_ending)
            } else {
                format!("{}{}{}", header_line, line_ending, body)
            }
        };

        Ok(reconstructed.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn csv_domain_detection() {
        let domain = CsvDomain;
        assert!(domain.detect(b"a,b,c\n1,2,3", None) > 0.6);
        assert!(domain.detect(b"", Some("test.csv")) > 0.8);
    }

    #[test]
    fn csv_domain_roundtrip() {
        let domain = CsvDomain;
        let data = b"name,age,city\nAlice,30,NYC\nBob,25,LA";

        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }
}
