// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Passthrough domain: no-op extraction for Track 2 data.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::CpacResult;
use std::collections::HashMap;

/// Passthrough domain that performs no extraction.
///
/// Used for Track 2 data where MSN is not beneficial.
/// Simply passes through the original data as residual.
pub struct PassthroughDomain;

impl Domain for PassthroughDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "passthrough",
            name: "Passthrough (No Extraction)",
            extensions: &[],
            mime_types: &[],
            magic_bytes: &[],
        }
    }

    fn detect(&self, _data: &[u8], _filename: Option<&str>) -> f64 {
        // Passthrough never auto-detects (confidence 0)
        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        Ok(ExtractionResult {
            fields: HashMap::new(),
            residual: data.to_vec(),
            metadata: HashMap::new(),
            domain_id: "passthrough".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        // Simply return the residual
        Ok(result.residual.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_roundtrip() {
        let domain = PassthroughDomain;
        let data = b"test data that should pass through unchanged";

        let result = domain.extract(data).unwrap();
        assert_eq!(result.residual, data);
        assert!(result.fields.is_empty());

        let reconstructed = domain.reconstruct(&result).unwrap();
        assert_eq!(reconstructed, data);
    }

    #[test]
    fn passthrough_never_detects() {
        let domain = PassthroughDomain;
        assert_eq!(domain.detect(b"any data", None), 0.0);
        assert_eq!(domain.detect(b"", Some("file.txt")), 0.0);
    }
}
