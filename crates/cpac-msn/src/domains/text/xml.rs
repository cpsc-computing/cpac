// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! XML domain handler with tag/attribute extraction.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::{CpacError, CpacResult};
use std::collections::HashMap;

/// XML domain handler.
///
/// Extracts repeated tag names and attribute keys.
/// Target compression: 15-30x on structured XML/HTML.
pub struct XmlDomain;

impl Domain for XmlDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "text.xml",
            name: "XML",
            extensions: &[".xml", ".html", ".svg", ".xhtml"],
            mime_types: &["text/xml", "application/xml", "text/html"],
            magic_bytes: &[],
        }
    }

    fn detect(&self, data: &[u8], filename: Option<&str>) -> f64 {
        if let Some(fname) = filename {
            if fname.ends_with(".xml") || fname.ends_with(".svg") {
                return 0.9;
            }
            if fname.ends_with(".html") || fname.ends_with(".xhtml") {
                return 0.85;
            }
        }

        // Check for XML declaration or common tags
        if data.starts_with(b"<?xml") {
            return 0.95;
        }
        if data.starts_with(b"<!DOCTYPE html") || data.starts_with(b"<html") {
            return 0.9;
        }
        // Check for closing tags
        if data.windows(2).any(|w| w == b"</") && data.contains(&b'>') {
            return 0.6;
        }

        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("XML decode: {}", e)))?;

        // Extract all tag names (simple parser)
        let mut tag_freq: HashMap<String, usize> = HashMap::new();
        let mut in_tag = false;
        let mut tag_name = String::new();
        let mut chars = text.chars().peekable();

        while let Some(ch) = chars.next() {
            match ch {
                '<' => {
                    in_tag = true;
                    tag_name.clear();
                    // Skip '/' for closing tags
                    if chars.peek() == Some(&'/') {
                        chars.next();
                    }
                    // Skip '!' for comments/declarations
                    if chars.peek() == Some(&'!') {
                        in_tag = false;
                    }
                    // Skip '?' for processing instructions
                    if chars.peek() == Some(&'?') {
                        in_tag = false;
                    }
                }
                '>' | ' ' | '/' | '\t' | '\n' if in_tag => {
                    if !tag_name.is_empty() {
                        *tag_freq.entry(tag_name.clone()).or_insert(0) += 1;
                        tag_name.clear();
                    }
                    in_tag = false;
                }
                _ if in_tag => {
                    tag_name.push(ch);
                }
                _ => {}
            }
        }

        // Extract tags with frequency >= 2
        let mut repeated_tags: Vec<(String, usize)> = tag_freq
            .into_iter()
            .filter(|(_, count)| *count >= 2)
            .collect();
        repeated_tags.sort_by(|a, b| b.1.cmp(&a.1));

        // Build tag map
        let mut tag_map: HashMap<String, u32> = HashMap::new();
        for (idx, (tag, _)) in repeated_tags.iter().enumerate() {
            tag_map.insert(tag.clone(), idx as u32);
        }

        // Compact XML by replacing tag names
        let mut compacted = text.to_string();
        for (tag, idx) in &tag_map {
            let placeholder = format!("@T{}", idx);
            compacted = compacted.replace(&format!("<{}", tag), &format!("<{}", placeholder));
            compacted = compacted.replace(&format!("</{}", tag), &format!("</{}", placeholder));
            compacted = compacted.replace(&format!("<{} ", tag), &format!("<{} ", placeholder));
        }

        let mut fields = HashMap::new();
        fields.insert("tags".to_string(), serde_json::Value::Array(
            repeated_tags.iter().map(|(t, _)| serde_json::Value::String(t.clone())).collect()
        ));

        Ok(ExtractionResult {
            fields,
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "text.xml".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let tags_value = result.fields.get("tags")
            .ok_or_else(|| CpacError::DecompressFailed("Missing tags".into()))?;

        let tags: Vec<String> = if let serde_json::Value::Array(arr) = tags_value {
            arr.iter().filter_map(|v| v.as_str().map(String::from)).collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid tags format".into()));
        };

        let mut reconstructed = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {}", e)))?
            .to_string();

        // Expand placeholders back to original tags
        for (idx, tag) in tags.iter().enumerate() {
            let placeholder = format!("@T{}", idx);
            reconstructed = reconstructed.replace(&format!("<{}", placeholder), &format!("<{}", tag));
            reconstructed = reconstructed.replace(&format!("</{}", placeholder), &format!("</{}", tag));
        }

        Ok(reconstructed.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_domain_detection() {
        let domain = XmlDomain;
        assert!(domain.detect(b"<?xml version=\"1.0\"?>", None) > 0.9);
        assert!(domain.detect(b"<html><body></body></html>", None) > 0.5);
        assert!(domain.detect(b"", Some("test.xml")) > 0.8);
    }

    #[test]
    fn xml_domain_roundtrip() {
        let domain = XmlDomain;
        let data = b"<root><item>A</item><item>B</item></root>";

        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }
}
