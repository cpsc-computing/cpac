// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Domain trait and core types for MSN.

use cpac_types::CpacResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Metadata about a domain handler.
#[derive(Clone, Debug)]
pub struct DomainInfo {
    /// Domain identifier (e.g., "text.json", "text.csv")
    pub id: &'static str,
    /// Human-readable name
    pub name: &'static str,
    /// File extensions this domain handles
    pub extensions: &'static [&'static str],
    /// MIME types this domain handles
    pub mime_types: &'static [&'static str],
    /// Magic bytes for detection (multiple alternatives)
    pub magic_bytes: &'static [&'static [u8]],
}

/// Result of semantic extraction from a domain handler.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExtractionResult {
    /// Extracted semantic fields (domain-specific)
    pub fields: HashMap<String, serde_json::Value>,
    /// Residual bytes not captured by semantic extraction
    pub residual: Vec<u8>,
    /// Domain-specific metadata for reconstruction
    pub metadata: HashMap<String, String>,
    /// Domain ID that performed the extraction
    pub domain_id: String,
}

/// Trait for domain-specific semantic extraction handlers.
///
/// Each domain handler implements format-specific logic to extract
/// high-redundancy semantic fields and isolate residual bytes.
pub trait Domain: Send + Sync {
    /// Return metadata about this domain handler.
    fn info(&self) -> DomainInfo;

    /// Detect if this domain can handle the given data.
    ///
    /// Returns a confidence score from 0.0 to 1.0, where:
    /// - 0.0 = definitely not this domain
    /// - 0.5 = minimum viable confidence
    /// - 1.0 = certain this is the correct domain
    fn detect(&self, data: &[u8], filename: Option<&str>) -> f64;

    /// Extract semantic fields from data.
    ///
    /// Returns extracted fields and residual bytes.
    /// Must be losslessly reversible via `reconstruct()`.
    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult>;

    /// Extract semantic fields using pre-computed field mappings.
    ///
    /// This allows consistent field-to-index mappings across multiple
    /// extractions (e.g., for streaming per-block compression).
    /// The fields parameter contains domain-specific field mappings
    /// from a previous extraction.
    ///
    /// Default implementation falls back to normal `extract()`.
    fn extract_with_fields(
        &self,
        data: &[u8],
        _fields: &HashMap<String, serde_json::Value>,
    ) -> CpacResult<ExtractionResult> {
        // Default: ignore provided fields and do normal extraction
        self.extract(data)
    }

    /// Reconstruct original bytes from extraction result.
    ///
    /// Must produce byte-identical output to original input.
    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extraction_result_serialization() {
        let mut fields = HashMap::new();
        fields.insert("test".to_string(), serde_json::json!({"key": "value"}));
        
        let result = ExtractionResult {
            fields,
            residual: vec![1, 2, 3],
            metadata: HashMap::new(),
            domain_id: "test.domain".to_string(),
        };

        // Should serialize/deserialize without errors
        let serialized = serde_json::to_string(&result).unwrap();
        let deserialized: ExtractionResult = serde_json::from_str(&serialized).unwrap();
        
        assert_eq!(result.domain_id, deserialized.domain_id);
        assert_eq!(result.residual, deserialized.residual);
    }
}
