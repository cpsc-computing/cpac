// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Multi-Scale Normalization (MSN) for CPAC.
//!
//! MSN performs domain-specific semantic extraction on structured data formats.
//! It runs only when SSR indicates Track 1 (structured data worth extracting).

#![allow(
    clippy::missing_errors_doc,
    clippy::missing_panics_doc,
    clippy::unnecessary_wraps,
    clippy::needless_pass_by_value
)]

pub mod domain;
pub mod domains;
pub mod registry;

pub use domain::{Domain, DomainInfo, ExtractionResult};
pub use registry::{global_registry, DomainRegistry};

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
    /// MSN format version (currently 1)
    #[serde(default = "default_msn_version")]
    pub version: u8,
    /// Extracted semantic fields
    pub fields: std::collections::HashMap<String, serde_json::Value>,
    /// Whether MSN was actually applied
    pub applied: bool,
    /// Domain ID used (if applied)
    pub domain_id: Option<String>,
    /// Detection confidence
    pub confidence: f64,
}

fn default_msn_version() -> u8 {
    1
}

impl MsnResult {
    /// Create a passthrough result (MSN not applied).
    #[must_use]
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
    #[must_use]
    pub fn metadata(&self) -> MsnMetadata {
        MsnMetadata {
            version: 1,
            fields: self.fields.clone(),
            applied: self.applied,
            domain_id: self.domain_id.clone(),
            confidence: self.confidence,
        }
    }
}

impl MsnMetadata {
    /// Convert to `MsnResult` by adding residual.
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
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
        DomainHint::Log => ".log",
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

/// Extract semantic fields using existing metadata for consistent field mappings.
///
/// This applies the field mappings from `metadata` to new `data`, ensuring
/// consistent indices across multiple extractions (e.g., streaming per-block).
///
/// If metadata was not applied or domain not found, falls back to passthrough.
///
/// # Examples
///
/// Compressing a stream of YAML blocks with consistent key indices:
/// ```
/// use cpac_msn::{extract, extract_with_metadata};
///
/// let block1 = b"host: srv1\nport: 8080\nhost: srv2\nport: 9090\n";
/// let block2 = b"host: srv3\nport: 7070\n";
///
/// // Detection phase: extract from first block to build field map.
/// let result1 = extract(block1, None, 0.7).unwrap();
///
/// // Subsequent blocks use the same field map for consistent indices.
/// if result1.applied {
///     let meta = result1.metadata();
///     let result2 = extract_with_metadata(block2, &meta).unwrap();
///     // Both residuals can be independently decompressed with the same metadata.
///     assert!(result2.residual.len() <= block2.len());
/// }
/// ```
///
/// # Errors
///
/// Returns [`cpac_types::CpacError::CompressFailed`] if the domain extraction fails.
pub fn extract_with_metadata(data: &[u8], metadata: &MsnMetadata) -> CpacResult<MsnResult> {
    if !metadata.applied {
        // Metadata was passthrough, so just passthrough this data too
        return Ok(MsnResult::passthrough(data));
    }

    let domain_id = metadata.domain_id.as_ref().ok_or_else(|| {
        cpac_types::CpacError::CompressFailed("MSN metadata missing domain_id".into())
    })?;

    let registry = global_registry();
    let domain = registry.get(domain_id).ok_or_else(|| {
        cpac_types::CpacError::CompressFailed(format!("Domain not found: {domain_id}"))
    })?;

    // Use extract_with_fields to apply consistent field mappings
    match domain.extract_with_fields(data, &metadata.fields) {
        Ok(extraction) => Ok(MsnResult {
            fields: extraction.fields,
            residual: extraction.residual,
            applied: true,
            domain_id: Some(extraction.domain_id),
            confidence: metadata.confidence,
        }),
        Err(_) => {
            // Extraction failed, fall back to passthrough
            Ok(MsnResult::passthrough(data))
        }
    }
}

/// Encode [`MsnMetadata`] to a compact binary representation.
///
/// Uses `MessagePack` (via `rmp-serde`) prefixed with a `0x01` discriminator byte.
/// This is ~30-40% smaller than JSON for typical metadata payloads and avoids
/// UTF-8 parsing overhead on the hot decompression path.
///
/// The `decode_metadata_compact` function is forward-compatible: if the first
/// byte is `{` (0x7B) it falls back to JSON for frames compressed by older
/// versions of CPAC.
///
/// # Examples
///
/// ```
/// use cpac_msn::{encode_metadata_compact, decode_metadata_compact, MsnMetadata};
///
/// let meta = MsnMetadata {
///     version: 1,
///     fields: std::collections::HashMap::new(),
///     applied: true,
///     domain_id: Some("text.yaml".to_string()),
///     confidence: 0.9,
/// };
///
/// let compact = encode_metadata_compact(&meta).unwrap();
/// let decoded = decode_metadata_compact(&compact).unwrap();
/// assert_eq!(decoded.domain_id, meta.domain_id);
/// assert_eq!(decoded.applied, true);
/// ```
///
/// # Errors
///
/// Returns [`cpac_types::CpacError::CompressFailed`] if `MessagePack` serialization fails.
pub fn encode_metadata_compact(meta: &MsnMetadata) -> CpacResult<Vec<u8>> {
    let mut out = vec![0x01u8]; // discriminator: 0x01 = MessagePack
    let msgpack = rmp_serde::to_vec(meta)
        .map_err(|e| cpac_types::CpacError::CompressFailed(format!("MSN metadata encode: {e}")))?;
    out.extend_from_slice(&msgpack);
    Ok(out)
}

/// Decode [`MsnMetadata`] from the compact representation produced by
/// [`encode_metadata_compact`], or from legacy JSON.
///
/// Auto-detects format from the first byte:
/// - `0x01` → `MessagePack` (new format)
/// - `{` (0x7B) → JSON (legacy)
pub fn decode_metadata_compact(bytes: &[u8]) -> CpacResult<MsnMetadata> {
    if bytes.is_empty() {
        return Err(cpac_types::CpacError::DecompressFailed(
            "empty MSN metadata".into(),
        ));
    }
    if bytes[0] == 0x01 {
        rmp_serde::from_slice(&bytes[1..]).map_err(|e| {
            cpac_types::CpacError::DecompressFailed(format!("MSN metadata msgpack: {e}"))
        })
    } else {
        // Legacy JSON path (first byte is '{' = 0x7B or whitespace)
        serde_json::from_slice(bytes)
            .map_err(|e| cpac_types::CpacError::DecompressFailed(format!("MSN metadata json: {e}")))
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
        cpac_types::CpacError::DecompressFailed(format!("Domain not found: {domain_id}"))
    })?;

    let extraction = ExtractionResult {
        fields: result.fields.clone(),
        residual: result.residual.clone(),
        metadata: std::collections::HashMap::new(),
        domain_id: domain_id.clone(),
    };

    domain.reconstruct(&extraction)
}
