// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! CSV domain handler.

use cpac_types::{CpacError, CpacResult, CpacType, DomainHint};

use crate::DomainHandler;

/// Detect if data looks like CSV.
#[must_use] 
pub fn detect_csv(data: &[u8]) -> bool {
    let sample = &data[..data.len().min(4096)];
    let text = String::from_utf8_lossy(sample);
    let lines: Vec<&str> = text.lines().take(10).collect();
    if lines.len() < 2 {
        return false;
    }
    // Check for consistent delimiter across lines
    for delim in [',', '\t', '|'] {
        let counts: Vec<usize> = lines.iter().map(|l| l.matches(delim).count()).collect();
        if counts[0] > 0 && counts.iter().all(|&c| c == counts[0]) {
            return true;
        }
    }
    false
}

/// Parse CSV text into column vectors.
pub fn parse_csv(data: &[u8]) -> CpacResult<(Vec<String>, Vec<Vec<String>>)> {
    let text = std::str::from_utf8(data)
        .map_err(|e| CpacError::Transform(format!("CSV: invalid UTF-8: {e}")))?;

    let delim = detect_delimiter(text);
    let mut lines = text.lines();
    let header_line = lines
        .next()
        .ok_or_else(|| CpacError::Transform("CSV: empty data".into()))?;
    let headers: Vec<String> = header_line
        .split(delim)
        .map(|s| s.trim().to_string())
        .collect();
    let num_cols = headers.len();

    let mut columns: Vec<Vec<String>> = vec![Vec::new(); num_cols];
    for line in lines {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split(delim).collect();
        for (i, col) in columns.iter_mut().enumerate() {
            col.push(fields.get(i).unwrap_or(&"").trim().to_string());
        }
    }

    Ok((headers, columns))
}

fn detect_delimiter(text: &str) -> char {
    let first_line = text.lines().next().unwrap_or("");
    let comma_count = first_line.matches(',').count();
    let tab_count = first_line.matches('\t').count();
    let pipe_count = first_line.matches('|').count();
    if tab_count > comma_count && tab_count > pipe_count {
        '\t'
    } else if pipe_count > comma_count {
        '|'
    } else {
        ','
    }
}

/// CSV domain handler.
pub struct CsvHandler;

impl DomainHandler for CsvHandler {
    fn name(&self) -> &'static str {
        "csv"
    }
    fn domain_hint(&self) -> DomainHint {
        DomainHint::Csv
    }
    fn can_handle(&self, data: &[u8]) -> bool {
        detect_csv(data)
    }
    fn decompose(&self, data: &[u8]) -> CpacResult<CpacType> {
        let (headers, columns) = parse_csv(data)?;
        let typed_columns: Vec<(String, CpacType)> = headers
            .into_iter()
            .zip(columns)
            .map(|(name, values)| {
                let total_bytes: usize = values.iter().map(std::string::String::len).sum();
                (
                    name,
                    CpacType::StringColumn {
                        values,
                        total_bytes,
                    },
                )
            })
            .collect();
        Ok(CpacType::ColumnSet {
            columns: typed_columns,
        })
    }
    fn reconstruct(&self, columns: &CpacType) -> CpacResult<Vec<u8>> {
        match columns {
            CpacType::ColumnSet { columns } => {
                let headers: Vec<&str> = columns.iter().map(|(name, _)| name.as_str()).collect();
                let num_rows = match &columns[0].1 {
                    CpacType::StringColumn { values, .. } => values.len(),
                    _ => 0,
                };
                let mut out = String::new();
                out.push_str(&headers.join(","));
                out.push('\n');
                for row in 0..num_rows {
                    let fields: Vec<String> = columns
                        .iter()
                        .map(|(_, col)| match col {
                            CpacType::StringColumn { values, .. } => {
                                values.get(row).cloned().unwrap_or_default()
                            }
                            _ => String::new(),
                        })
                        .collect();
                    out.push_str(&fields.join(","));
                    out.push('\n');
                }
                Ok(out.into_bytes())
            }
            _ => Err(CpacError::Transform(
                "CSV reconstruct: expected ColumnSet".into(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_simple_csv() {
        let data = b"name,age,city\nAlice,30,NYC\nBob,25,LA\n";
        let (headers, cols) = parse_csv(data).unwrap();
        assert_eq!(headers, vec!["name", "age", "city"]);
        assert_eq!(cols[0], vec!["Alice", "Bob"]);
        assert_eq!(cols[1], vec!["30", "25"]);
        assert_eq!(cols[2], vec!["NYC", "LA"]);
    }

    #[test]
    fn decompose_reconstruct() {
        let data = b"a,b\n1,2\n3,4\n";
        let handler = CsvHandler;
        let columns = handler.decompose(data).unwrap();
        let restored = handler.reconstruct(&columns).unwrap();
        let restored_str = String::from_utf8(restored).unwrap();
        assert!(restored_str.contains("a,b"));
        assert!(restored_str.contains("1,2"));
        assert!(restored_str.contains("3,4"));
    }
}
