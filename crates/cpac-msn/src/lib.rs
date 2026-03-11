// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
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

use cpac_types::CpacResult;

/// Maximum data size for single-call MSN extraction.
///
/// Domain extractors use O(N×K) algorithms (String::replace, char-by-char
/// parsing, serde parse-and-serialize) that become prohibitively expensive on
/// large single-block buffers.  Files above this threshold should go through
/// the parallel path where MSN runs per-block (4–32 MB blocks).
pub const MSN_MAX_EXTRACT_SIZE: usize = 16 * 1024 * 1024; // 16 MB

/// Recommended per-domain extraction size limit.
///
/// Individual domain handlers should reject data above this threshold to
/// avoid O(N×K) blowup from String::replace or full-file JSON parsing.
/// Domains with especially expensive algorithms (e.g. XML with 4× replace
/// per tag) should use a lower limit.
pub const MAX_DOMAIN_EXTRACT_SIZE: usize = 8 * 1024 * 1024; // 8 MB

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
///
/// Only `version`, `fields`, and `domain_id` are serialised into the frame.
/// `applied` and `confidence` are runtime-only: they are not written to avoid
/// wasting ~10 bytes per frame. On deserialisation both default to `false`/`0.0`
/// so frames produced by older CPAC versions decode without error.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct MsnMetadata {
    /// MSN format version (currently 1)
    #[serde(default = "default_msn_version")]
    pub version: u8,
    /// Extracted semantic fields
    pub fields: std::collections::HashMap<String, serde_json::Value>,
    /// Whether MSN was actually applied (runtime only — not serialised).
    /// Infer from `domain_id.is_some()` after deserialisation.
    #[serde(skip_serializing, default)]
    pub applied: bool,
    /// Domain ID used (if applied)
    pub domain_id: Option<String>,
    /// Detection confidence (runtime only — not serialised).
    #[serde(skip_serializing, default)]
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

    /// Create a zero-copy "not applied" sentinel.
    ///
    /// Unlike [`passthrough`](Self::passthrough), this does **not** clone the
    /// input data.  The caller is expected to use the original data slice
    /// directly, avoiding the redundant allocation that `passthrough` incurs
    /// when the engine already holds the source buffer.
    #[must_use]
    pub fn not_applied() -> Self {
        Self {
            fields: std::collections::HashMap::new(),
            residual: Vec::new(),
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
    ///
    /// `applied` is derived from whether `domain_id` is set (the engine only
    /// stores metadata when extraction was applied, so `domain_id` is always
    /// `Some` when this struct comes from a real frame).  `confidence` is not
    /// stored in frames and defaults to `0.0` on the decompression path.
    #[must_use]
    #[allow(clippy::needless_pass_by_value)]
    pub fn with_residual(self, residual: Vec<u8>) -> MsnResult {
        let applied = self.domain_id.is_some();
        MsnResult {
            fields: self.fields,
            residual,
            applied,
            domain_id: self.domain_id,
            confidence: self.confidence,
        }
    }
}

/// Extract semantic fields from data using MSN.
///
/// This is the main entry point for MSN extraction.
///
/// `filename` is an optional file path or name that the registry uses for
/// extension-based domain detection (e.g. passing `"events.jsonl"` enables
/// JSONL-specific detection before content probing). Pass `None` for
/// content-only detection.
pub fn extract(data: &[u8], filename: Option<&str>, min_confidence: f64) -> CpacResult<MsnResult> {
    // Large-file guard: skip extraction for buffers above the threshold.
    // Domain extractors use O(N×K) string operations that are prohibitively
    // expensive on large single-block buffers.  The parallel path handles
    // large files by running MSN per-block (4–32 MB blocks).
    if data.len() > MSN_MAX_EXTRACT_SIZE {
        return Ok(MsnResult::not_applied());
    }

    let registry = global_registry();

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
                    // Extraction failed — zero-copy fallback (no data clone).
                    Ok(MsnResult::not_applied())
                }
            }
        }
        None => {
            // No domain detected — zero-copy fallback (no data clone).
            Ok(MsnResult::not_applied())
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
    // `applied` is not stored in frames (skip_serializing); infer from domain_id.
    if metadata.domain_id.is_none() {
        // Metadata was passthrough — no domain was applied.
        return Ok(MsnResult::not_applied());
    }

    // Large-file guard (same rationale as extract()).
    if data.len() > MSN_MAX_EXTRACT_SIZE {
        return Ok(MsnResult::not_applied());
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
            // Extraction failed — zero-copy fallback.
            Ok(MsnResult::not_applied())
        }
    }
}

/// Encode [`MsnMetadata`]
///
/// Uses `MessagePack` (via `rmp-serde`, named/map format) prefixed with a `0x01`
/// discriminator byte.  Named format is used so that `#[serde(skip_serializing)]`
/// fields (`applied`, `confidence`) are cleanly absent from the byte stream while
/// still being deserializable from older frames that do include them.
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
///     // `applied` and `confidence` are runtime-only and are NOT serialised.
///     applied: true,
///     domain_id: Some("text.yaml".to_string()),
///     confidence: 0.9,
/// };
///
/// let compact = encode_metadata_compact(&meta).unwrap();
/// let decoded = decode_metadata_compact(&compact).unwrap();
/// assert_eq!(decoded.domain_id, meta.domain_id);
/// // applied is not serialised — infer from domain_id presence:
/// assert!(decoded.domain_id.is_some());
/// ```
///
/// # Errors
///
/// Returns [`cpac_types::CpacError::CompressFailed`] if `MessagePack` serialization fails.
pub fn encode_metadata_compact(meta: &MsnMetadata) -> CpacResult<Vec<u8>> {
    let mut out = vec![0x01u8]; // discriminator: 0x01 = MessagePack
                                // Use named (map) format so absent `skip_serializing` fields are handled
                                // gracefully by `#[serde(default)]` on the decode side.  The positional
                                // (array) format produced by `to_vec` would mis-align when fields in the
                                // middle of the struct are skipped.
    let msgpack = rmp_serde::to_vec_named(meta)
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
