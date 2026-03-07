// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Protocol Buffers domain handler (stub).
//!
//! Note: Full Protobuf support requires schema definitions. This is a
//! placeholder implementation that provides basic detection and passthrough.

use crate::domain::{Domain, DomainInfo, ExtractionResult};
use cpac_types::CpacResult;
use std::collections::HashMap;

/// Protocol Buffers domain handler.
///
/// Currently a passthrough implementation. Full semantic extraction requires
/// schema information which is not available at compression time.
/// Target compression: Relies on downstream entropy coding.
pub struct ProtobufDomain;

impl Domain for ProtobufDomain {
    fn info(&self) -> DomainInfo {
        DomainInfo {
            id: "binary.protobuf",
            name: "Protocol Buffers",
            extensions: &[".pb", ".protobuf"],
            mime_types: &["application/x-protobuf"],
            magic_bytes: &[],
        }
    }

    fn detect(&self, _data: &[u8], _filename: Option<&str>) -> f64 {
        // Disabled: the heuristic wire-type check matches virtually all text and
        // binary content (any byte with low 3 bits ≤ 5 counts), and the extractor
        // is a passthrough stub, so applying this domain only adds metadata
        // overhead with zero benefit.  Re-enable once a schema-aware extractor
        // is implemented (Phase 4 redesign).
        0.0
    }

    fn extract(&self, data: &[u8]) -> CpacResult<ExtractionResult> {
        // Without schema, we can't extract field names
        // Just store raw data and let entropy coding handle it
        Ok(ExtractionResult {
            fields: HashMap::new(),
            residual: data.to_vec(),
            metadata: HashMap::new(),
            domain_id: "binary.protobuf".to_string(),
        })
    }

    fn reconstruct(&self, result: &ExtractionResult) -> CpacResult<Vec<u8>> {
        Ok(result.residual.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protobuf_domain_passthrough() {
        let domain = ProtobufDomain;
        let data = b"\x08\x96\x01\x12\x04test"; // Simple protobuf-like data

        let result = domain.extract(data).unwrap();
        let reconstructed = domain.reconstruct(&result).unwrap();

        assert_eq!(data.as_slice(), reconstructed.as_slice());
    }
}
