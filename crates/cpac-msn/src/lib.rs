// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Multi-Scale Normalization (MSN) for CPAC.
//!
//! MSN performs domain-specific semantic extraction on structured data formats.
//! It runs only when SSR indicates Track 1 (structured data worth extracting).

pub mod domain;
pub mod registry;
pub mod domains;

pub use domain::{Domain, DomainInfo, ExtractionResult};
pub use registry::{DomainRegistry, global_registry};

use cpac_types::{CpacResult, DomainHint};

/// Result of MSN extraction.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MsnResult {
    /// Extracted semantic fields
    pub fields: std::collections::HashMap<String, serde_json::Value>,
    /// Residual bytes after extraction
    pub residual: Vec<u8>,
    /// Whether MSN was actually applied
    pub applied: bool,
    /// Domain ID used (if applied)
    pub domain_id: Option<String>,
    /// Detection confidence
    pub confidence: f64,
}

/// Lightweight MSN metadata for frame storage (excludes residual).
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MsnMetadata {
    /// Extracted semantic fields
    pub fields: std::collections::HashMap<String, serde_json::Value>,
    /// Whether MSN was actually applied
    pub applied: bool,
    /// Domain ID used (if applied)
    pub domain_id: Option<String>,
    /// Detection confidence
    pub confidence: f64,
}

impl MsnResult {
    /// Create a passthrough result (MSN not applied).
    pub fn passthrough(data: &[u8]) -> Self {
        Self {
            fields: std::collections::HashMap::new(),
            residual: data.to_vec(),
            applied: false,
            domain_id: None,
            confidence: 0.0,
        }
    }

    /// Extract metadata for frame storage (without residual).
    pub fn metadata(&self) -> MsnMetadata {
        MsnMetadata {
            fields: self.fields.clone(),
            applied: self.applied,
            domain_id: self.domain_id.clone(),
            confidence: self.confidence,
        }
    }
}

impl MsnMetadata {
    /// Convert to MsnResult by adding residual.
    pub fn with_residual(self, residual: Vec<u8>) -> MsnResult {
        MsnResult {
            fields: self.fields,
            residual,
            applied: self.applied,
            domain_id: self.domain_id,
            confidence: self.confidence,
        }
    }
}

/// Extract semantic fields from data using MSN.
///
/// This is the main entry point for MSN extraction.
pub fn extract(
    data: &[u8],
    domain_hint: Option<DomainHint>,
    min_confidence: f64,
) -> CpacResult<MsnResult> {
    let registry = global_registry();
    
    // Auto-detect domain (domain_hint used for filename extension hints)
    let filename = domain_hint.as_ref().map(|h| match h {
        DomainHint::Json => ".json",
        DomainHint::Xml => ".xml",
        DomainHint::Csv => ".csv",
        _ => "",
    });
    
    let detected = registry.auto_detect(data, filename, min_confidence);
    
    match detected {
        Some((domain, confidence)) => {
            match domain.extract(data) {
                Ok(extraction) => Ok(MsnResult {
                    fields: extraction.fields,
                    residual: extraction.residual,
                    applied: true,
                    domain_id: Some(extraction.domain_id),
                    confidence,
                }),
                Err(_) => {
                    // Extraction failed, fall back to passthrough
                    Ok(MsnResult::passthrough(data))
                }
            }
        }
        None => {
            // No domain detected, passthrough
            Ok(MsnResult::passthrough(data))
        }
    }
}

/// Reconstruct original data from MSN result.
pub fn reconstruct(result: &MsnResult) -> CpacResult<Vec<u8>> {
    if !result.applied {
        // Passthrough case
        return Ok(result.residual.clone());
    }
    
    let domain_id = result.domain_id.as_ref().ok_or_else(|| {
        cpac_types::CpacError::DecompressFailed("MSN result missing domain_id".into())
    })?;
    
    let registry = global_registry();
    let domain = registry.get(domain_id).ok_or_else(|| {
        cpac_types::CpacError::DecompressFailed(format!("Domain not found: {}", domain_id))
    })?;
    
    let extraction = ExtractionResult {
        fields: result.fields.clone(),
        residual: result.residual.clone(),
        metadata: std::collections::HashMap::new(),
        domain_id: domain_id.clone(),
    };
    
    domain.reconstruct(&extraction)
}
