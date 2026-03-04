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
        // Find the first newline — marks the end of the header line.
        let newline_pos = data.iter().position(|&b| b == b'\n')
            .ok_or_else(|| CpacError::CompressFailed("CSV: no newline found (single-line CSV)".into()))?;

        // Determine the separator written after the header (CRLF vs LF).
        // We look only at THIS specific newline, not the whole file, so that
        // mixed-ending files are handled correctly.
        let (header_content_end, header_sep) =
            if newline_pos > 0 && data[newline_pos - 1] == b'\r' {
                (newline_pos - 1, "\r\n") // CRLF: header content ends before the \r
            } else {
                (newline_pos, "\n") // LF-only: header content ends before the \n
            };

        // Parse header column names from the raw header bytes.
        // We do NOT trim so that reconstruct produces the byte-exact original.
        let header_bytes = &data[..header_content_end];
        let header_str = std::str::from_utf8(header_bytes)
            .map_err(|e| CpacError::CompressFailed(format!("CSV header decode: {}", e)))?;
        let headers: Vec<&str> = header_str.split(',').collect();

        // Body = every byte after the first newline, stored verbatim.
        // This preserves all original line endings (including mixed CRLF/LF)
        // so reconstruct can produce a byte-exact copy of the original.
        let body_start = newline_pos + 1;
        let body = data[body_start..].to_vec();

        let mut fields = HashMap::new();
        fields.insert(
            "headers".to_string(),
            serde_json::Value::Array(
                headers
                    .iter()
                    .map(|h| serde_json::Value::String(h.to_string()))
                    .collect(),
            ),
        );
        fields.insert(
            "header_sep".to_string(),
            serde_json::Value::String(header_sep.to_string()),
        );

        Ok(ExtractionResult {
            fields,
            residual: body,
            metadata: HashMap::new(),
            domain_id: "text.csv".to_string(),
        })
    }

    fn extract_with_fields(
        &self,
        data: &[u8],
        fields: &HashMap<String, serde_json::Value>,
    ) -> CpacResult<ExtractionResult> {
        // Header-aware streaming extraction.
        //
        // The first streaming block contains the header row; subsequent blocks
        // contain only data rows.  We detect which case applies by checking
        // whether the block starts with the expected header bytes.
        //
        // - Header block  → normal extract() path (header stripped, residual = body).
        // - Data-only block → passthrough with 0x01 marker prefix in the residual:
        //     [0x01 (1B)] [raw block bytes...]
        //   reconstruct() strips the marker and returns the raw bytes directly.

        // Recover expected header from detection-phase metadata.
        let headers: Vec<String> = match fields.get("headers") {
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => Vec::new(),
        };
        let header_sep = fields
            .get("header_sep")
            .and_then(|v| v.as_str())
            .unwrap_or("\n");

        // Build the expected header prefix: "col1,col2,...<sep>"
        let expected_prefix = format!("{}{}", headers.join(","), header_sep);

        if data.starts_with(expected_prefix.as_bytes()) {
            // First (header-bearing) block — use standard extraction.
            self.extract(data)
        } else {
            // Data-only block — passthrough with streaming marker.
            let mut residual = Vec::with_capacity(1 + data.len());
            residual.push(0x01u8); // streaming data-only marker
            residual.extend_from_slice(data);

            Ok(ExtractionResult {
                fields: fields.clone(),
                residual,
                metadata: HashMap::new(),
                domain_id: "text.csv".to_string(),
            })
        }
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        // Streaming data-only block: strip the 0x01 marker and return raw bytes.
        if result.residual.first() == Some(&0x01u8) {
            return Ok(result.residual[1..].to_vec());
        }
        let headers_value = result.fields.get("headers")
            .ok_or_else(|| CpacError::DecompressFailed("Missing headers".into()))?;

        let headers: Vec<String> = if let serde_json::Value::Array(arr) = headers_value {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid headers format".into()));
        };

        // header_sep is the separator written after the header line during extraction.
        // Defaults to "\n" for frames written before this field was introduced.
        let header_sep = result.fields.get("header_sep")
            .and_then(|v| v.as_str())
            .unwrap_or("\n");

        let header_line = headers.join(",");

        // The residual already contains the exact original bytes that followed the
        // header line (all line endings preserved).  Simply prepend the header.
        let mut output =
            Vec::with_capacity(header_line.len() + header_sep.len() + result.residual.len());
        output.extend_from_slice(header_line.as_bytes());
        output.extend_from_slice(header_sep.as_bytes());
        output.extend_from_slice(&result.residual);

        Ok(output)
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
    fn csv_domain_roundtrip_lf() {
        let domain = CsvDomain;
        let data = b"name,age,city\nAlice,30,NYC\nBob,25,LA";

        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }

    #[test]
    fn csv_domain_roundtrip_lf_trailing_newline() {
        let domain = CsvDomain;
        let data = b"name,age,city\nAlice,30,NYC\nBob,25,LA\n";

        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }

    #[test]
    fn csv_domain_roundtrip_crlf() {
        let domain = CsvDomain;
        let data = b"name,age,city\r\nAlice,30,NYC\r\nBob,25,LA\r\n";

        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }

    /// Reproduces the real-world bug: corpus files that are mostly LF but end
    /// with a single CRLF on the last row.
    #[test]
    fn csv_domain_roundtrip_mixed_endings() {
        let domain = CsvDomain;
        // Header: LF, rows 1-999: LF, last row: CRLF (matches metrics.csv pattern)
        let mut data: Vec<u8> = b"id,value,status\n".to_vec();
        for i in 0..999u32 {
            data.extend_from_slice(format!("{i},{},{i}\n", i * 7 % 1000).as_bytes());
        }
        // Last row ends with CRLF (the single CR in the file)
        data.extend_from_slice(b"999,993,999\r\n");

        let result = domain.extract(&data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        assert_eq!(data, reconstructed, "mixed line ending roundtrip failed");
    }

    #[test]
    fn csv_domain_roundtrip_header_only() {
        let domain = CsvDomain;
        let data = b"col1,col2,col3\n";

        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }

    /// Streaming: first block (contains header) is extracted normally.
    #[test]
    fn csv_streaming_header_block() {
        let domain = CsvDomain;
        let data = b"name,age,city\nAlice,30,NYC\nBob,25,LA\n";

        let detection = domain.extract(data).unwrap();
        let result = domain.extract_with_fields(data, &detection.fields).unwrap();

        // Should NOT start with 0x01 — it went through the normal extract() path.
        assert_ne!(result.residual.first(), Some(&0x01u8));

        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(reconstructed, data.to_vec());
    }

    /// Streaming: subsequent block (data rows only) passes through with marker.
    #[test]
    fn csv_streaming_data_only_block() {
        let domain = CsvDomain;
        let header_block = b"name,age,city\nAlice,30,NYC\n";
        let data_block = b"Charlie,35,SF\nDiana,28,NYC\n";

        let detection = domain.extract(header_block).unwrap();
        let result = domain.extract_with_fields(data_block, &detection.fields).unwrap();

        // Must start with 0x01 marker.
        assert_eq!(result.residual[0], 0x01u8);

        // Reconstruction returns raw block bytes unchanged.
        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(reconstructed, data_block.to_vec());
    }

    /// Two-block streaming roundtrip: header block + data block concatenate
    /// back to the original file.
    #[test]
    fn csv_streaming_two_block_roundtrip() {
        let domain = CsvDomain;
        let block1 = b"id,value\n1,100\n2,200\n";
        let block2 = b"3,300\n4,400\n";
        let original: Vec<u8> = [block1.as_slice(), block2.as_slice()].concat();

        let detection = domain.extract(block1).unwrap();
        let fields = detection.fields;

        let result1 = domain.extract_with_fields(block1, &fields).unwrap();
        let result2 = domain.extract_with_fields(block2, &fields).unwrap();

        let recon1 = domain.reconstruct(&result1).unwrap();
        let recon2 = domain.reconstruct(&result2).unwrap();

        let mut combined = recon1;
        combined.extend_from_slice(&recon2);

        assert_eq!(combined, original, "two-block CSV streaming roundtrip failed");
    }
}
