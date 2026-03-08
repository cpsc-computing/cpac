// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! CSV domain handler with header/column extraction.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;
// memchr provides SIMD-accelerated byte search on x86/aarch64.

// ---------------------------------------------------------------------------
// Columnar extraction helper
// ---------------------------------------------------------------------------

struct ColumnarData {
    /// Per-column type: "int", "float", or "str".
    col_types: Vec<String>,
    /// Delta-encoded integer columns keyed by column index.
    int_columns: serde_json::Map<String, serde_json::Value>,
    /// Residual: non-integer column values, row line endings.
    residual: Vec<u8>,
}

/// Attempt columnar extraction on CSV body rows.
///
/// Returns `Some(ColumnarData)` if ≥50 rows and ≥1 integer column found.
fn try_columnar_extraction(body: &[u8], _headers: &[&str]) -> Option<ColumnarData> {
    if body.is_empty() {
        return None;
    }

    // Parse rows (quick scan, no full CSV parser needed for simple CSVs)
    let body_str = std::str::from_utf8(body).ok()?;
    let mut rows: Vec<Vec<&str>> = Vec::new();
    let mut line_endings: Vec<&str> = Vec::new();

    for line in body_str.split_inclusive('\n') {
        let (content, ending) = if let Some(stripped) = line.strip_suffix("\r\n") {
            (stripped, "\r\n")
        } else if let Some(stripped) = line.strip_suffix('\n') {
            (stripped, "\n")
        } else {
            (line, "")
        };
        if content.is_empty() && ending.is_empty() {
            continue;
        }
        let cols: Vec<&str> = content.split(',').collect();
        rows.push(cols);
        line_endings.push(ending);
    }

    if rows.len() < 50 {
        return None;
    }

    let num_cols = rows.first().map(|r| r.len()).unwrap_or(0);
    if num_cols == 0 {
        return None;
    }

    // Classify columns
    let mut col_types = vec!["str".to_string(); num_cols];
    let mut int_cols: Vec<Option<Vec<i64>>> = vec![None; num_cols];

    for ci in 0..num_cols {
        // Check if all values in this column are parseable as i64
        let mut all_int = true;
        let mut vals = Vec::with_capacity(rows.len());
        for row in &rows {
            if ci >= row.len() {
                all_int = false;
                break;
            }
            match row[ci].parse::<i64>() {
                Ok(v) => vals.push(v),
                Err(_) => {
                    all_int = false;
                    break;
                }
            }
        }
        if all_int && !vals.is_empty() {
            col_types[ci] = "int".to_string();
            int_cols[ci] = Some(vals);
        }
    }

    // Need at least 1 integer column to justify columnar mode
    if int_cols.iter().all(|c| c.is_none()) {
        return None;
    }

    // Build int_columns map with delta encoding
    let mut int_columns = serde_json::Map::new();
    for (ci, opt_vals) in int_cols.iter().enumerate() {
        if let Some(vals) = opt_vals {
            // Delta encode
            let mut deltas = Vec::with_capacity(vals.len());
            deltas.push(serde_json::Value::Number(vals[0].into()));
            for i in 1..vals.len() {
                let d = vals[i] - vals[i - 1];
                deltas.push(serde_json::Value::Number(d.into()));
            }
            int_columns.insert(ci.to_string(), serde_json::Value::Array(deltas));
        }
    }

    // Build residual: for integer columns, replace values with placeholder "@"
    let mut residual = Vec::new();
    for (ri, row) in rows.iter().enumerate() {
        for (ci, &val) in row.iter().enumerate() {
            if ci > 0 {
                residual.push(b',');
            }
            if int_cols[ci].is_some() {
                residual.push(b'@');
            } else {
                residual.extend_from_slice(val.as_bytes());
            }
        }
        residual.extend_from_slice(line_endings[ri].as_bytes());
    }

    Some(ColumnarData {
        col_types,
        int_columns,
        residual,
    })
}

/// CSV domain handler.
///
/// Extracts column headers and structure from CSV data.
/// Target compression: 20-50x on structured CSV.
pub struct CsvDomain;

impl CsvDomain {
    /// Reconstruct from columnar mode: replace '@' placeholders with delta-decoded integers.
    fn reconstruct_columnar(
        &self,
        result: &ExtractionResult,
        header_line: &str,
        header_sep: &str,
    ) -> CpacResult<Vec<u8>> {
        // Decode delta-encoded integer columns
        let int_columns = result
            .fields
            .get("int_columns")
            .and_then(|v| v.as_object())
            .ok_or_else(|| CpacError::DecompressFailed("CSV: missing int_columns".into()))?;

        // Build per-column iterators: delta-decode and convert to string
        let mut col_iters: HashMap<usize, Vec<String>> = HashMap::new();
        for (key, arr_val) in int_columns {
            let ci: usize = key
                .parse()
                .map_err(|_| CpacError::DecompressFailed("CSV: invalid column index".into()))?;
            let deltas: Vec<i64> = arr_val
                .as_array()
                .ok_or_else(|| CpacError::DecompressFailed("CSV: int_columns not array".into()))?
                .iter()
                .filter_map(|v| v.as_i64())
                .collect();
            // Un-delta
            let mut values = Vec::with_capacity(deltas.len());
            let mut acc = 0i64;
            for (i, &d) in deltas.iter().enumerate() {
                if i == 0 {
                    acc = d;
                } else {
                    acc += d;
                }
                values.push(acc.to_string());
            }
            col_iters.insert(ci, values);
        }

        // Walk the residual, replacing '@' placeholders with decoded values
        let residual_str = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("CSV: residual UTF-8: {e}")))?;

        let mut output =
            Vec::with_capacity(header_line.len() + header_sep.len() + result.residual.len() * 2);
        output.extend_from_slice(header_line.as_bytes());
        output.extend_from_slice(header_sep.as_bytes());

        // Track row index per column for value lookup
        let mut col_row_idx: HashMap<usize, usize> = HashMap::new();

        for line in residual_str.split_inclusive('\n') {
            let (content, ending) = if let Some(stripped) = line.strip_suffix("\r\n") {
                (stripped, "\r\n")
            } else if let Some(stripped) = line.strip_suffix('\n') {
                (stripped, "\n")
            } else {
                (line, "")
            };

            if content.is_empty() && ending.is_empty() {
                continue;
            }

            let cols: Vec<&str> = content.split(',').collect();
            for (ci, &val) in cols.iter().enumerate() {
                if ci > 0 {
                    output.push(b',');
                }
                if val == "@" {
                    if let Some(values) = col_iters.get(&ci) {
                        let ri = col_row_idx.entry(ci).or_insert(0);
                        if *ri < values.len() {
                            output.extend_from_slice(values[*ri].as_bytes());
                            *ri += 1;
                        }
                    }
                } else {
                    output.extend_from_slice(val.as_bytes());
                }
            }
            output.extend_from_slice(ending.as_bytes());
        }

        Ok(output)
    }
}

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
            if std::path::Path::new(fname)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("csv"))
            {
                return 0.9;
            }
            if std::path::Path::new(fname)
                .extension()
                .is_some_and(|e| e.eq_ignore_ascii_case("tsv"))
            {
                return 0.85;
            }
        }

        // Content-based CSV detection: require ≥3 columns AND ≥10 data rows
        // before firing.  The previous threshold (≥1 comma in first line) was
        // too loose — it matched conf files, bash scripts, and YAML lists that
        // have incidental commas, accumulating -40KB overhead with no benefit.
        let first_nl = memchr::memchr(b'\n', data).unwrap_or(data.len());
        if first_nl == 0 || first_nl >= data.len() {
            return 0.0;
        }
        let first_line = &data[..first_nl];
        let comma_count = memchr::memchr_iter(b',', first_line).count();
        if comma_count < 2 {
            // Need at least 3 columns to justify columnar extraction overhead.
            return 0.0;
        }
        // Count newlines in the rest of the file to estimate row count.
        let row_count = memchr::memchr_iter(b'\n', &data[first_nl + 1..]).count();
        if row_count >= 10 {
            return 0.7;
        }

        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        // Find the first newline — marks the end of the header line.
        let newline_pos = data.iter().position(|&b| b == b'\n').ok_or_else(|| {
            CpacError::CompressFailed("CSV: no newline found (single-line CSV)".into())
        })?;

        // Determine the separator written after the header (CRLF vs LF).
        let (header_content_end, header_sep) = if newline_pos > 0 && data[newline_pos - 1] == b'\r'
        {
            (newline_pos - 1, "\r\n")
        } else {
            (newline_pos, "\n")
        };

        let header_bytes = &data[..header_content_end];
        let header_str = std::str::from_utf8(header_bytes)
            .map_err(|e| CpacError::CompressFailed(format!("CSV header decode: {e}")))?;
        let headers: Vec<&str> = header_str.split(',').collect();

        let body_start = newline_pos + 1;
        let body = data[body_start..].to_vec();

        // Try columnar extraction on data rows (≥50 rows, ≥1 int column)
        let columnar = try_columnar_extraction(&body, &headers);

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

        if let Some(col_data) = columnar {
            // Store delta-encoded integer columns in fields
            fields.insert(
                "int_columns".to_string(),
                serde_json::Value::Object(col_data.int_columns),
            );
            fields.insert(
                "col_types".to_string(),
                serde_json::Value::Array(
                    col_data
                        .col_types
                        .iter()
                        .map(|t| serde_json::Value::String(t.clone()))
                        .collect(),
                ),
            );
            fields.insert(
                "mode".to_string(),
                serde_json::Value::String("columnar".to_string()),
            );

            Ok(ExtractionResult {
                fields,
                residual: col_data.residual,
                metadata: HashMap::new(),
                domain_id: "text.csv".to_string(),
            })
        } else {
            fields.insert(
                "mode".to_string(),
                serde_json::Value::String("header_only".to_string()),
            );

            Ok(ExtractionResult {
                fields,
                residual: body,
                metadata: HashMap::new(),
                domain_id: "text.csv".to_string(),
            })
        }
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
        let headers_value = result
            .fields
            .get("headers")
            .ok_or_else(|| CpacError::DecompressFailed("Missing headers".into()))?;

        let headers: Vec<String> = if let serde_json::Value::Array(arr) = headers_value {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid headers format".into()));
        };

        let header_sep = result
            .fields
            .get("header_sep")
            .and_then(|v| v.as_str())
            .unwrap_or("\n");

        let header_line = headers.join(",");

        let mode = result
            .fields
            .get("mode")
            .and_then(|v| v.as_str())
            .unwrap_or("header_only");

        if mode == "columnar" {
            return self.reconstruct_columnar(result, &header_line, header_sep);
        }

        // header_only mode: prepend header to residual body
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
        // Extension-based detection still fires.
        assert!(domain.detect(b"", Some("test.csv")) > 0.8);
        assert!(domain.detect(b"", Some("data.tsv")) > 0.8);
        // Content detection: requires >=3 columns AND >=10 data rows.
        let few_rows = b"a,b,c\n1,2,3";
        assert_eq!(
            domain.detect(few_rows, None),
            0.0,
            "too few rows should not fire"
        );
        let many_rows = {
            let mut v: Vec<u8> = b"id,value,status\n".to_vec();
            for i in 0..10u32 {
                v.extend_from_slice(format!("{i},{},{i}\n", i * 2).as_bytes());
            }
            v
        };
        assert!(
            domain.detect(&many_rows, None) > 0.6,
            ">=10 rows with 3 cols should fire"
        );
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
        let result = domain
            .extract_with_fields(data_block, &detection.fields)
            .unwrap();

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

        assert_eq!(
            combined, original,
            "two-block CSV streaming roundtrip failed"
        );
    }
}
