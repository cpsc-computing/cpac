// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! XML domain handler — basic element text extraction.
//!
//! Extracts element text content into string columns grouped by tag name.
//! This is a lightweight parser; a full XML DOM is not needed for compression.

use cpac_types::{CpacError, CpacResult, CpacType, DomainHint};
use std::collections::BTreeMap;

use crate::DomainHandler;

/// XML domain handler.
pub struct XmlHandler;

impl DomainHandler for XmlHandler {
    fn name(&self) -> &'static str {
        "xml"
    }
    fn domain_hint(&self) -> DomainHint {
        DomainHint::Xml
    }
    fn can_handle(&self, data: &[u8]) -> bool {
        let text = String::from_utf8_lossy(&data[..data.len().min(200)]);
        text.contains("<?xml") || text.contains("<root") || text.contains("xmlns")
    }
    fn decompose(&self, data: &[u8]) -> CpacResult<CpacType> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::Transform(format!("XML: invalid UTF-8: {e}")))?;
        let columns = extract_element_columns(text);
        let typed_columns: Vec<(String, CpacType)> = columns
            .into_iter()
            .map(|(tag, values)| {
                let total_bytes: usize = values.iter().map(std::string::String::len).sum();
                (
                    tag,
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
    fn reconstruct(&self, _columns: &CpacType) -> CpacResult<Vec<u8>> {
        // XML reconstruction is complex; return error for now
        Err(CpacError::Transform(
            "XML reconstruct: not yet implemented".into(),
        ))
    }
}

/// Extract element text content grouped by tag name.
fn extract_element_columns(text: &str) -> Vec<(String, Vec<String>)> {
    let mut columns: BTreeMap<String, Vec<String>> = BTreeMap::new();
    // Simple regex-free parser: find <tag>content</tag> patterns
    let mut pos = 0;
    let bytes = text.as_bytes();
    while pos < bytes.len() {
        if bytes[pos] == b'<'
            && pos + 1 < bytes.len()
            && bytes[pos + 1] != b'/'
            && bytes[pos + 1] != b'?'
            && bytes[pos + 1] != b'!'
        {
            // Find tag name
            let tag_start = pos + 1;
            let tag_end = text[tag_start..]
                .find(['>', ' ', '/'])
                .map_or(bytes.len(), |i| tag_start + i);
            let tag = &text[tag_start..tag_end];
            if tag.is_empty() {
                pos += 1;
                continue;
            }
            // Find closing angle bracket
            let close_bracket = text[pos..].find('>').map(|i| pos + i);
            if let Some(cb) = close_bracket {
                // Self-closing?
                if bytes[cb - 1] == b'/' {
                    pos = cb + 1;
                    continue;
                }
                let content_start = cb + 1;
                // Find closing tag
                let close_tag = format!("</{tag}>");
                if let Some(ct) = text[content_start..].find(&close_tag) {
                    let content = text[content_start..content_start + ct].trim();
                    if !content.is_empty() && !content.starts_with('<') {
                        columns
                            .entry(tag.to_string())
                            .or_default()
                            .push(content.to_string());
                        pos = content_start + ct + close_tag.len();
                    } else {
                        // Nested elements — continue parsing inside
                        pos = content_start;
                    }
                    continue;
                }
            }
        }
        pos += 1;
    }
    columns.into_iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_elements() {
        let xml = r#"<?xml version="1.0"?>
<root>
  <item><name>Alice</name><age>30</age></item>
  <item><name>Bob</name><age>25</age></item>
</root>"#;
        let cols = extract_element_columns(xml);
        assert!(cols
            .iter()
            .any(|(tag, vals)| tag == "name" && vals == &["Alice", "Bob"]));
        assert!(cols
            .iter()
            .any(|(tag, vals)| tag == "age" && vals == &["30", "25"]));
    }

    #[test]
    fn decompose_xml() {
        let xml = b"<?xml version=\"1.0\"?><root><a>1</a><a>2</a></root>";
        let handler = XmlHandler;
        assert!(handler.can_handle(xml));
        let result = handler.decompose(xml).unwrap();
        match result {
            CpacType::ColumnSet { columns } => {
                assert!(!columns.is_empty());
            }
            _ => panic!("expected ColumnSet"),
        }
    }
}
