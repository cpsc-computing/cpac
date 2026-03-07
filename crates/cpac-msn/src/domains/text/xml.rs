// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
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

    fn detect(&self, _data: &[u8], _filename: Option<&str>) -> f64 {
        // Disabled: tag-name token substitution is net-negative against zstd on
        // real XML/HTML corpora (-117KB aggregate on the benchmark set).  zstd's
        // LZ77 back-references already exploit tag repetition at 6-8x compression;
        // our replacement disrupts those back-references and adds metadata overhead.
        // Re-enable once a structure-aware transform that preserves backend
        // compressibility is implemented (Phase 4 redesign).
        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("XML decode: {e}")))?;

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
        #[allow(clippy::cast_possible_truncation)]
        for (idx, (tag, _)) in repeated_tags.iter().enumerate() {
            tag_map.insert(tag.clone(), idx as u32);
        }

        // Compact XML by replacing tag names
        // IMPORTANT: Replace in reverse order by tag length to avoid partial matches
        let mut tag_vec: Vec<(&String, &u32)> = tag_map.iter().collect();
        tag_vec.sort_by(|a, b| b.0.len().cmp(&a.0.len())); // Longest tags first

        let mut compacted = text.to_string();
        for (tag, idx) in tag_vec {
            let placeholder = format!("@T{idx}");
            // Use word boundaries to avoid partial replacements
            compacted = compacted.replace(&format!("<{tag}"), &format!("<{placeholder}"));
            compacted = compacted.replace(&format!("</{tag}"), &format!("</{placeholder}"));
            compacted = compacted.replace(&format!("<{tag} "), &format!("<{placeholder} "));
            compacted = compacted.replace(&format!("<{tag}>"), &format!("<{placeholder}>"));
        }

        // Savings gate: only proceed if tag extraction made the residual smaller.
        // For files with short tag names (e.g. <id>, <dt>) the placeholder strings
        // (<@T0>, <@T1>, ...) can be LONGER than the originals, inflating the
        // residual and degrading the final compression ratio (e.g. silesia/xml).
        // Returning original data as residual causes the engine safety check to
        // reject this MSN result (residual + metadata >= original → passthrough).
        if compacted.len() >= data.len() {
            return Ok(ExtractionResult {
                fields: HashMap::new(),
                residual: data.to_vec(),
                metadata: HashMap::new(),
                domain_id: "text.xml".to_string(),
            });
        }

        let mut fields = HashMap::new();
        fields.insert(
            "tags".to_string(),
            serde_json::Value::Array(
                repeated_tags
                    .iter()
                    .map(|(t, _)| serde_json::Value::String(t.clone()))
                    .collect(),
            ),
        );

        Ok(ExtractionResult {
            fields,
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "text.xml".to_string(),
        })
    }

    fn extract_with_fields(
        &self,
        data: &[u8],
        fields: &HashMap<String, serde_json::Value>,
    ) -> CpacResult<ExtractionResult> {
        // Apply XML tag compaction using the detection-phase tag list so that
        // every streaming block uses the same @T{idx} ↔ tag mapping.
        let text = std::str::from_utf8(data)
            .map_err(|e| CpacError::CompressFailed(format!("XML decode: {e}")))?;

        let tags: Vec<String> = match fields.get("tags") {
            Some(serde_json::Value::Array(arr)) => arr
                .iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect(),
            _ => Vec::new(),
        };

        // Build tag_map from detection-phase list (same stable indices).
        #[allow(clippy::cast_possible_truncation)]
        let tag_map: HashMap<String, u32> = tags
            .iter()
            .enumerate()
            .map(|(i, t)| (t.clone(), i as u32))
            .collect();

        // Replace longest tags first to avoid partial matches.
        let mut tag_vec: Vec<(&String, &u32)> = tag_map.iter().collect();
        tag_vec.sort_by(|a, b| b.0.len().cmp(&a.0.len()));

        let mut compacted = text.to_string();
        for (tag, idx) in &tag_vec {
            let placeholder = format!("@T{idx}");
            compacted = compacted.replace(&format!("<{tag} "), &format!("<{placeholder} "));
            compacted = compacted.replace(&format!("<{tag}>"), &format!("<{placeholder}>"));
            compacted = compacted.replace(&format!("</{tag}"), &format!("</{placeholder}"));
            compacted = compacted.replace(&format!("<{tag}"), &format!("<{placeholder}"));
        }

        Ok(ExtractionResult {
            fields: fields.clone(),
            residual: compacted.into_bytes(),
            metadata: HashMap::new(),
            domain_id: "text.xml".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        let tags_value = result
            .fields
            .get("tags")
            .ok_or_else(|| CpacError::DecompressFailed("Missing tags".into()))?;

        let tags: Vec<String> = if let serde_json::Value::Array(arr) = tags_value {
            arr.iter()
                .filter_map(|v| v.as_str().map(String::from))
                .collect()
        } else {
            return Err(CpacError::DecompressFailed("Invalid tags format".into()));
        };

        let mut reconstructed = std::str::from_utf8(&result.residual)
            .map_err(|e| CpacError::DecompressFailed(format!("UTF-8 decode: {e}")))?
            .to_string();

        // Expand placeholders back to original tags
        // CRITICAL: Replace in REVERSE order (highest index first) to avoid placeholder interference
        // E.g., if we have @T1 and @T10, replacing @T1 first would corrupt @T10 → @Ttag0
        for (idx, tag) in tags.iter().enumerate().rev() {
            let placeholder = format!("@T{idx}");
            // Replace all forms of the tag
            reconstructed = reconstructed.replace(&format!("<{placeholder} "), &format!("<{tag} "));
            reconstructed = reconstructed.replace(&format!("<{placeholder}>"), &format!("<{tag}>"));
            reconstructed = reconstructed.replace(&format!("<{placeholder}"), &format!("<{tag}"));
            reconstructed = reconstructed.replace(&format!("</{placeholder}"), &format!("</{tag}"));
        }

        Ok(reconstructed.into_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn xml_domain_detection() {
        // Detection is disabled (returns 0.0 for all inputs) pending redesign.
        let domain = XmlDomain;
        assert_eq!(domain.detect(b"<?xml version=\"1.0\"?>", None), 0.0);
        assert_eq!(domain.detect(b"<html><body></body></html>", None), 0.0);
        assert_eq!(domain.detect(b"", Some("test.xml")), 0.0);
    }

    #[test]
    fn xml_domain_roundtrip() {
        let domain = XmlDomain;
        let data = b"<root><item>A</item><item>B</item></root>";

        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }

    /// extract_with_fields() uses detection-phase indices (stable across blocks).
    #[test]
    fn xml_streaming_consistent_indices() {
        let domain = XmlDomain;
        let block1 = b"<person><name>Alice</name><age>30</age></person>\n<person><name>Bob</name><age>25</age></person>\n";
        let block2 = b"<person><name>Charlie</name><age>35</age></person>\n";

        // Simulate detection from block1
        let detection = domain.extract(block1).unwrap();

        // Compress block1 with detection fields
        let r1 = domain
            .extract_with_fields(block1, &detection.fields)
            .unwrap();
        // Compress block2 with SAME detection fields
        let r2 = domain
            .extract_with_fields(block2, &detection.fields)
            .unwrap();

        // Both use detection-phase fields for reconstruction
        let recon1 = domain.reconstruct(&r1).unwrap();
        let recon2 = domain.reconstruct(&r2).unwrap();

        assert_eq!(recon1, block1.to_vec());
        assert_eq!(recon2, block2.to_vec());
    }

    /// Two-block streaming produces the same output as the original concatenation.
    #[test]
    fn xml_streaming_two_block_roundtrip() {
        let domain = XmlDomain;
        let block1 = b"<?xml version=\"1.0\"?>\n<records>\n<item><id>1</id><name>A</name></item>\n";
        let block2 = b"<item><id>2</id><name>B</name></item>\n<item><id>3</id><name>C</name></item>\n</records>\n";
        let original: Vec<u8> = [block1.as_slice(), block2.as_slice()].concat();

        let detection = domain.extract(block1).unwrap();
        let fields = detection.fields;

        let r1 = domain.extract_with_fields(block1, &fields).unwrap();
        let r2 = domain.extract_with_fields(block2, &fields).unwrap();

        let mut combined = domain.reconstruct(&r1).unwrap();
        combined.extend_from_slice(&domain.reconstruct(&r2).unwrap());

        assert_eq!(
            combined, original,
            "XML two-block streaming roundtrip failed"
        );
    }
}
