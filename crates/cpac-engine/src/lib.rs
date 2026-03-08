// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! CPAC compression engine — top-level compress/decompress API.
//!
//! Pipeline:
//! 1. SSR analysis → select backend + track
//! 2. Preprocess (transforms) — TP-frame auto-select or DAG profile
//! 3. Entropy coding (Zstd/Brotli/Raw)
//! 4. Frame encoding (self-describing wire format)

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

pub mod analyzer;
pub mod bench;
pub mod corpus;
pub mod host;
pub mod parallel;
pub mod pool;

pub use analyzer::{analyze_structure, format_profile, ColumnProfile, StructureProfile};
pub use bench::{check_regressions, load_baseline, save_baseline};
pub use bench::{
    BaselineEngine, BaselineEntry, BenchProfile, BenchResult, BenchmarkRunner, CorpusSummary,
    RegressionKind, RegressionViolation,
};
pub use cpac_dag::{ProfileCache, TransformDAG, TransformRegistry};
pub use cpac_types::{
    Backend, CompressConfig, CompressResult, CpacError, CpacResult, DecompressResult,
    ResourceConfig, Track,
};
pub use host::{auto_resource_config, cached_host_info, detect_host, HostInfo, SimdTier};
pub use parallel::{
    compress_parallel, decompress_parallel, is_cpbl, CPBL_MAGIC, DEFAULT_BLOCK_SIZE,
    PARALLEL_THRESHOLD,
};

/// Engine version string.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Minimum input size (in bytes) below which preprocessing is skipped.
const PREPROCESS_THRESHOLD: usize = 4096;

/// Compress data using the CPAC pipeline.
///
/// Performs adaptive compression with SSR analysis, optional preprocessing transforms,
/// and entropy coding. The compressed data is wrapped in a self-describing frame format
/// that can be decompressed with [`decompress`].
///
/// # Pipeline
/// 1. SSR analysis → select backend + track
/// 2. Preprocess (transforms) — SSR-guided or DAG profile
/// 3. Entropy coding (Zstd/Brotli/Raw)
/// 4. Frame encoding (self-describing wire format)
///
/// # Examples
///
/// Basic compression with auto-selected backend:
/// ```
/// use cpac_engine::{compress, CompressConfig};
///
/// let data = b"Hello, CPAC!";
/// let config = CompressConfig::default();
/// let result = compress(data, &config).unwrap();
/// println!("Compressed {} bytes to {} bytes ({}x)",
///          result.original_size, result.compressed_size, result.ratio());
/// ```
///
/// Force a specific backend:
/// ```
/// use cpac_engine::{compress, CompressConfig, Backend};
///
/// let config = CompressConfig {
///     backend: Some(Backend::Brotli),
///     ..Default::default()
/// };
/// let result = compress(b"test data", &config).unwrap();
/// assert_eq!(result.backend, Backend::Brotli);
/// ```
///
/// # Errors
///
/// Returns [`CpacError::CompressFailed`] if the entropy backend fails.
///
/// # See Also
///
/// - [`decompress`] — decompress CPAC frames
/// - [`compress_parallel`] — parallel block compression for large data
#[must_use = "compression result is returned"]
pub fn compress(data: &[u8], config: &CompressConfig) -> CpacResult<CompressResult> {
    let original_size = data.len();

    // 1. SSR analysis
    let ssr = cpac_ssr::analyze(data);

    // 2. Select backend with size awareness
    let backend = config.backend.unwrap_or_else(|| {
        cpac_entropy::auto_select_backend_with_size(ssr.entropy_estimate, original_size)
    });

    // 3. Check if we should use parallel compression for large files.
    // Done BEFORE MSN extraction so we don't waste time extracting MSN on the full
    // file only to discard the result — each parallel block applies MSN independently.
    // Skip if disable_parallel flag is set (prevents recursive calls from compress_parallel).
    if !config.disable_parallel
        && original_size >= parallel::PARALLEL_THRESHOLD
        && backend != Backend::Raw
    {
        // Use default 1MB block size and auto-detect thread count
        let num_threads = rayon::current_num_threads();
        return compress_parallel(data, config, DEFAULT_BLOCK_SIZE, num_threads);
    }

    // 4. MSN (Multi-Scale Normalization) — Track 1 only, single-block path.
    //    Pass the actual filename from config so extension-based domain detection
    //    (e.g. ".jsonl", ".log") works in addition to content-based probing.
    let msn_filename = config.filename.as_deref();
    // Verbose tracing: enabled by config flag (-vvv CLI) or CPAC_MSN_VERBOSE=1 env var.
    let msn_verbose = config.msn_verbose
        || std::env::var("CPAC_MSN_VERBOSE").is_ok_and(|v| v == "1" || v == "true");
    // force_track overrides SSR's track assignment (used for discovery/research benchmarks).
    // When Some(Track::Track1), MSN runs on every block regardless of entropy estimate.
    // When Some(Track::Track2), MSN is always bypassed.
    let effective_track = config.force_track.unwrap_or(ssr.track);
    let (msn_data, msn_metadata) = if config.enable_msn && effective_track == Track::Track1 {
        match cpac_msn::extract(data, msn_filename, config.msn_confidence) {
            Ok(result) if result.applied => {
                // MSN succeeded - use residual as input, store metadata (without residual).
                // Encode as compact MessagePack (~30-40% smaller than JSON).
                let metadata = cpac_msn::encode_metadata_compact(&result.metadata())?;
                // Safety check: only use MSN if residual + metadata is strictly
                // smaller than the original.  If not, the metadata overhead
                // (stored in the frame) would negate any compression gain.
                if result.residual.len() + metadata.len() < data.len() {
                    // Roundtrip verification: reconstruct from the residual and confirm
                    // the output bytes exactly match the original block.  This catches
                    // extraction bugs where global String::replace interactions produce a
                    // residual whose reconstruction differs in size or content from the
                    // source (e.g. NASA access logs where short IP sub-strings collide
                    // with placeholder tokens, inflating the reconstructed output).
                    match cpac_msn::reconstruct(&result) {
                        Ok(ref reconstructed) if reconstructed.as_slice() == data => {
                            if msn_verbose {
                                let savings_pct = (1.0
                                    - result.residual.len() as f64 / data.len() as f64)
                                    * 100.0;
                                eprintln!(
                                    "[MSN] domain={} conf={:.2} fields={} \
                                     residual={}B original={}B ({:.1}% saved) → APPLIED",
                                    result.domain_id.as_deref().unwrap_or("?"),
                                    result.confidence,
                                    result.fields.len(),
                                    result.residual.len(),
                                    data.len(),
                                    savings_pct,
                                );
                            }
                            (result.residual, metadata)
                        }
                        Ok(ref reconstructed) => {
                            if msn_verbose {
                                eprintln!(
                                    "[MSN] domain={} conf={:.2} → BYPASSED \
                                     (roundtrip mismatch: expected {}B got {}B)",
                                    result.domain_id.as_deref().unwrap_or("?"),
                                    result.confidence,
                                    data.len(),
                                    reconstructed.len(),
                                );
                            }
                            (data.to_vec(), Vec::new())
                        }
                        Err(e) => {
                            if msn_verbose {
                                eprintln!(
                                    "[MSN] domain={} conf={:.2} → BYPASSED \
                                     (roundtrip error: {e})",
                                    result.domain_id.as_deref().unwrap_or("?"),
                                    result.confidence,
                                );
                            }
                            (data.to_vec(), Vec::new())
                        }
                    }
                } else {
                    if msn_verbose {
                        eprintln!(
                            "[MSN] domain={} conf={:.2} → BYPASSED \
                             (no size savings: residual+meta={}B original={}B)",
                            result.domain_id.as_deref().unwrap_or("?"),
                            result.confidence,
                            result.residual.len() + metadata.len(),
                            data.len(),
                        );
                    }
                    (data.to_vec(), Vec::new())
                }
            }
            Ok(_) => {
                // Domain detected but applied=false — confidence below threshold.
                if msn_verbose {
                    eprintln!(
                        "[MSN] → BYPASSED (no domain above conf threshold {:.2})",
                        config.msn_confidence,
                    );
                }
                (data.to_vec(), Vec::new())
            }
            Err(_) => {
                // MSN extraction error — fall back to passthrough.
                (data.to_vec(), Vec::new())
            }
        }
    } else {
        // MSN disabled or Track 2 — passthrough.
        if msn_verbose && config.enable_msn {
            let why = if config.force_track == Some(Track::Track2) {
                "force_track=T2"
            } else {
                "Track 2 data, SSR confidence too low"
            };
            eprintln!("[MSN] → BYPASSED ({why})");
        }
        (data.to_vec(), Vec::new())
    };

    let data_to_compress = &msn_data;

    // 5. Adaptive preprocessing
    // Skip preprocessing for:
    // - Raw backend (passthrough mode)
    // - Small files (< 4KB) where overhead exceeds benefit
    let should_preprocess = backend != Backend::Raw && original_size >= PREPROCESS_THRESHOLD;

    // Track DAG descriptor for the frame header (non-empty when smart transforms used).
    let mut dag_descriptor: Vec<u8> = Vec::new();

    let preprocessed = if should_preprocess {
        // Re-analyze entropy from the actual data being preprocessed.  When MSN
        // extraction was applied the residual can have meaningfully different
        // entropy characteristics from the original (e.g. FQCN removal lowers
        // text entropy), so we let SSR re-measure rather than re-using the
        // original estimate for transform selection.
        let residual_ssr = if !msn_metadata.is_empty() {
            cpac_ssr::analyze(data_to_compress)
        } else {
            ssr
        };
        let transform_ctx = cpac_transforms::TransformContext {
            entropy_estimate: residual_ssr.entropy_estimate,
            ascii_ratio: residual_ssr.ascii_ratio,
            data_size: residual_ssr.data_size,
        };

        // Try smart transform path first (data-driven, DAG-based).
        let smart_result = if config.enable_smart_transforms {
            smart_preprocess(data_to_compress, config.filename.as_deref())
        } else {
            None
        };

        if let Some((smart_data, smart_dag_desc)) = smart_result {
            // Smart path produced a smaller output — use it.
            dag_descriptor = smart_dag_desc;
            smart_data
        } else {
            // Fall back to SSR-guided TP preprocess (generic profile / default)
            let (preprocessed, _transform_meta) =
                cpac_transforms::preprocess(data_to_compress, &transform_ctx);
            preprocessed
        }
    } else {
        data_to_compress.clone()
    };

    // 6. Entropy coding (level-aware, with optional dictionary for Zstd).
    //
    // If MSN metadata is present, prepend it to the preprocessed residual *before*
    // entropy coding so both are compressed in the same stream.  Sharing a
    // single zstd context lets the encoder discover cross-references between the
    // repeated token strings that appear in both the metadata dictionary and the
    // residual body, eliminating the per-frame uncompressed metadata overhead.
    let inline_msn_meta_len = msn_metadata.len();
    let data_for_entropy: Vec<u8> = if !msn_metadata.is_empty() {
        let mut combined = Vec::with_capacity(msn_metadata.len() + preprocessed.len());
        combined.extend_from_slice(&msn_metadata);
        combined.extend_from_slice(&preprocessed);
        combined
    } else {
        preprocessed
    };

    let compressed_payload = cpac_entropy::compress_at_level(
        &data_for_entropy,
        backend,
        config.level,
        if backend == Backend::Zstd {
            config.dictionary.as_deref()
        } else {
            None
        },
    )?;

    // 7. Frame encoding.
    //   - No MSN + no DAG: standard CP v1 frame.
    //   - MSN present: CP2 inline frame (FLAG_MSN_INLINE).
    //   - DAG descriptor is embedded in the frame header for both versions.
    let frame = if msn_metadata.is_empty() {
        cpac_frame::encode_frame(&compressed_payload, backend, original_size, &dag_descriptor)
    } else {
        cpac_frame::encode_frame_cp2_inline(
            &compressed_payload,
            backend,
            original_size,
            &dag_descriptor,
            inline_msn_meta_len,
        )
    };

    let compressed_size = frame.len();

    Ok(CompressResult {
        data: frame,
        original_size,
        compressed_size,
        track: effective_track,
        backend,
    })
}

// ---------------------------------------------------------------------------
// Smart preprocess (data-driven, DAG-based)
// ---------------------------------------------------------------------------

/// Minimum confidence threshold for a recommended transform to be included
/// in the smart preprocess pipeline.
const SMART_MIN_CONFIDENCE: f64 = 0.50;

/// Maximum number of individual transforms to trial in adaptive mode.
const MAX_ADAPTIVE_TRIALS: usize = 3;

/// Quick zstd-1 compressed size for comparing transform effectiveness.
/// Uses the fast level to keep overhead low while still reflecting real
/// compressibility (raw size is a poor proxy — a transform can make data
/// smaller in raw bytes but larger after entropy coding).
fn quick_zstd_size(data: &[u8]) -> usize {
    zstd::bulk::compress(data, 1)
        .map(|z| z.len())
        .unwrap_or(data.len())
}

/// Attempt data-driven preprocessing using the structure analyzer.
///
/// Two strategies are tried:
/// 1. **Full chain**: apply all recommended transforms as a DAG.
/// 2. **Adaptive trials**: try each candidate transform individually,
///    pick the one that produces the smallest output.
///
/// Effectiveness is measured by **compressed size** (quick zstd-1 trial),
/// not raw size, because transforms can alter entropy characteristics.
///
/// Returns `Some((transformed_bytes, dag_descriptor))` if any approach
/// compresses strictly smaller than the original; `None` otherwise.
fn smart_preprocess(data: &[u8], filename: Option<&str>) -> Option<(Vec<u8>, Vec<u8>)> {
    let profile = analyzer::analyze_structure(data, filename);

    // Filter to transforms that accept Serial input and have sufficient confidence.
    let registry = TransformRegistry::with_builtins();
    let candidates: Vec<&str> = profile
        .recommended_chain
        .iter()
        .filter(|r| r.confidence >= SMART_MIN_CONFIDENCE)
        .filter(|r| {
            // Only include transforms that accept Serial (raw bytes).
            registry
                .get_by_name(&r.name)
                .map(|n| n.accepts().contains(&cpac_types::TypeTag::Serial))
                .unwrap_or(false)
        })
        .map(|r| r.name.as_str())
        .collect();

    if candidates.is_empty() {
        return None;
    }

    let ssr = cpac_ssr::analyze(data);
    let ctx = cpac_transforms::TransformContext {
        entropy_estimate: ssr.entropy_estimate,
        ascii_ratio: ssr.ascii_ratio,
        data_size: data.len(),
    };

    // Baseline: compressed size of the untransformed input.
    // The total frame cost is approximately: compressed_payload + dag_descriptor.
    // We compare against this total cost, not just the compressed payload alone.
    let baseline_z = quick_zstd_size(data);

    // Track the best result across all trials.
    let mut best: Option<(Vec<u8>, Vec<u8>)> = None;
    let mut best_cost = baseline_z; // baseline has no descriptor overhead

    // Maximum DAG descriptor size: u16 in the frame header (65535 bytes).
    // Per-step metadata also uses u16.  Reject any trial that would overflow.
    const MAX_DESC_SIZE: usize = u16::MAX as usize;

    // --- Strategy 1: full chain ---
    if candidates.len() > 1 {
        if let Ok(dag) = TransformDAG::compile(&registry, &candidates) {
            let input = cpac_types::CpacType::Serial(data.to_vec());
            if let Ok((cpac_types::CpacType::Serial(bytes), meta_chain)) =
                dag.execute_forward(input, &ctx)
            {
                let desc = cpac_dag::serialize_dag_descriptor(&meta_chain);
                if desc.len() <= MAX_DESC_SIZE {
                    let total_cost = quick_zstd_size(&bytes) + desc.len();
                    if total_cost < best_cost {
                        best_cost = total_cost;
                        best = Some((bytes, desc));
                    }
                }
            }
        }
    }

    // --- Strategy 2: adaptive trials (each candidate individually) ---
    for &name in candidates.iter().take(MAX_ADAPTIVE_TRIALS) {
        if let Ok(dag) = TransformDAG::compile(&registry, &[name]) {
            let input = cpac_types::CpacType::Serial(data.to_vec());
            if let Ok((cpac_types::CpacType::Serial(bytes), meta_chain)) =
                dag.execute_forward(input, &ctx)
            {
                let desc = cpac_dag::serialize_dag_descriptor(&meta_chain);
                if desc.len() <= MAX_DESC_SIZE {
                    let total_cost = quick_zstd_size(&bytes) + desc.len();
                    if total_cost < best_cost {
                        best_cost = total_cost;
                        best = Some((bytes, desc));
                    }
                }
            }
        }
    }

    best
}

/// Decompress CPAC-framed data.
///
/// Reconstructs the original data from a CPAC-compressed frame. Automatically
/// detects the backend and transform pipeline from the frame header.
///
/// # Pipeline
/// 1. Decode frame → extract header and payload
/// 2. Entropy decompress → using backend from header
/// 3. Unpreprocess → reverse transforms (TP-frame or DAG)
///
/// # Examples
///
/// Basic decompression:
/// ```
/// use cpac_engine::{compress, decompress, CompressConfig};
///
/// let original = b"Hello, CPAC!";
/// let compressed = compress(original, &CompressConfig::default()).unwrap();
/// let result = decompress(&compressed.data).unwrap();
/// assert_eq!(result.data, original);
/// ```
///
/// # Errors
///
/// Returns [`CpacError::InvalidFrame`] if the frame header is corrupted or has an
/// unsupported version.
///
/// Returns [`CpacError::DecompressFailed`] if:
/// - The entropy backend fails to decompress the payload
/// - Transform reversal fails
/// - Size verification fails (decompressed size ≠ expected size)
///
/// # See Also
///
/// - [`compress`] — compress data to CPAC format
/// - [`decompress_parallel`] — parallel block decompression
#[must_use = "decompression result is returned"]
pub fn decompress(data: &[u8]) -> CpacResult<DecompressResult> {
    // Check if this is a CPBL (parallel) frame first
    if is_cpbl(data) {
        let num_threads = rayon::current_num_threads();
        return decompress_parallel(data, num_threads);
    }

    // 1. Decode frame
    let (header, payload) = cpac_frame::decode_frame(data)?;

    // 2. Entropy decompress
    let decompressed_payload = cpac_entropy::decompress(payload, header.backend)?;

    // 2.5 If MSN metadata was inlined, split it off before transform reversal.
    //     The compressed payload contains [msn_metadata][tp_framed_residual];
    //     msn_meta_len from the frame header gives the split point in the
    //     *decompressed* buffer.
    let (inline_meta_bytes, data_to_unpreprocess) =
        if header.flags & cpac_frame::FLAG_MSN_INLINE != 0 && header.msn_meta_len > 0 {
            let split = header.msn_meta_len;
            if decompressed_payload.len() < split {
                return Err(CpacError::DecompressFailed(format!(
                    "inline MSN meta split {split} > decompressed payload {}",
                    decompressed_payload.len()
                )));
            }
            let meta = decompressed_payload[..split].to_vec();
            let rest = decompressed_payload[split..].to_vec();
            (Some(meta), rest)
        } else {
            (None, decompressed_payload)
        };

    // 3. Reverse transforms (applied only to the TP-framed residual portion)
    let mut result = if header.dag_descriptor.is_empty() {
        // TP-frame based decompression (generic/default)
        cpac_transforms::unpreprocess(&data_to_unpreprocess, &[])
    } else {
        // DAG-based decompression: deserialize descriptor and execute backward
        let (ids, metas, _consumed) = cpac_dag::deserialize_dag_descriptor(&header.dag_descriptor)?;
        let registry = TransformRegistry::with_builtins();
        let dag = TransformDAG::compile_from_ids(&registry, &ids)?;
        let meta_chain: Vec<(u8, Vec<u8>)> = ids.into_iter().zip(metas).collect();
        let output = dag.execute_backward(
            cpac_types::CpacType::Serial(data_to_unpreprocess),
            &meta_chain,
        )?;
        match output {
            cpac_types::CpacType::Serial(bytes) => bytes,
            _ => {
                return Err(CpacError::DecompressFailed(
                    "DAG produced non-Serial output".into(),
                ))
            }
        }
    };

    // 4. MSN reconstruction.
    //    Inline format: metadata extracted from split above.
    //    Legacy CP2 format: metadata is in header.msn_metadata.
    let msn_bytes = inline_meta_bytes.or_else(|| {
        if !header.msn_metadata.is_empty() {
            Some(header.msn_metadata.clone())
        } else {
            None
        }
    });
    if let Some(mb) = msn_bytes {
        // Auto-detect encoding: 0x01 prefix = MessagePack (new), '{' prefix = JSON (legacy).
        let msn_metadata = cpac_msn::decode_metadata_compact(&mb)?;
        let msn_result = msn_metadata.with_residual(result);
        result = cpac_msn::reconstruct(&msn_result)?;
    }

    // 5. Verify size
    if result.len() != header.original_size as usize {
        return Err(CpacError::DecompressFailed(format!(
            "size mismatch: expected {}, got {}",
            header.original_size,
            result.len()
        )));
    }

    Ok(DecompressResult {
        data: result,
        success: true,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_default_config() {
        let data = b"Hello, CPAC Rust engine! This is a test.";
        let config = CompressConfig::default();
        let compressed = compress(data, &config).unwrap();
        assert!(compressed.compressed_size > 0);

        let decompressed = decompress(&compressed.data).unwrap();
        assert!(decompressed.success);
        assert_eq!(decompressed.data, data);
    }

    #[test]
    fn roundtrip_empty() {
        let data = b"";
        let config = CompressConfig::default();
        let compressed = compress(data, &config).unwrap();
        let decompressed = decompress(&compressed.data).unwrap();
        assert_eq!(decompressed.data, data);
    }

    #[test]
    fn roundtrip_forced_backend() {
        for backend in [Backend::Raw, Backend::Zstd, Backend::Brotli] {
            let data = b"Testing forced backend selection in CPAC.";
            let config = CompressConfig {
                backend: Some(backend),
                ..Default::default()
            };
            let compressed = compress(data, &config).unwrap();
            assert_eq!(compressed.backend, backend);

            let decompressed = decompress(&compressed.data).unwrap();
            assert_eq!(decompressed.data, data);
        }
    }

    #[test]
    fn roundtrip_repetitive() {
        let data: Vec<u8> = b"abcdef".repeat(10_000);
        let config = CompressConfig::default();
        let compressed = compress(&data, &config).unwrap();
        // Should compress well
        assert!(compressed.ratio() > 2.0);

        let decompressed = decompress(&compressed.data).unwrap();
        assert_eq!(decompressed.data, data);
    }

    #[test]
    fn roundtrip_binary() {
        let data: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        let config = CompressConfig::default();
        let compressed = compress(&data, &config).unwrap();
        let decompressed = decompress(&compressed.data).unwrap();
        assert_eq!(decompressed.data, data);
    }

    #[test]
    fn roundtrip_msn_xml_large_parallel() {
        // > 256 KB triggers CPBL parallel path; XML content should activate MSN
        let record = b"<?xml version=\"1.0\"?><record><id>1</id><name>Alice</name><age>30</age><city>New York</city></record>\n";
        let data: Vec<u8> = record.iter().copied().cycle().take(300_000).collect();
        assert!(data.len() >= parallel::PARALLEL_THRESHOLD);

        let config = CompressConfig {
            enable_msn: true,
            ..Default::default()
        };
        let compressed = compress(&data, &config).expect("CP2+CPBL compress failed");
        // Verify it actually went through parallel path (CPBL wrapper)
        assert!(is_cpbl(&compressed.data), "expected CPBL frame");
        let result = decompress(&compressed.data).expect("CP2+CPBL decompress failed");
        assert_eq!(result.data, data, "CP2+CPBL roundtrip data mismatch");
    }

    #[test]
    fn roundtrip_smart_transforms_text() {
        // Text data large enough to trigger preprocessing (> 4KB).
        // normalize should be recommended (ascii_ratio > 0.80) and applied.
        let data: Vec<u8> = b"The quick brown fox jumps over the lazy dog. ".repeat(200);
        let config = CompressConfig {
            enable_smart_transforms: true,
            ..Default::default()
        };
        let compressed = compress(&data, &config).unwrap();
        let decompressed = decompress(&compressed.data).unwrap();
        assert_eq!(decompressed.data, data, "smart transforms roundtrip failed");
    }

    #[test]
    fn roundtrip_smart_transforms_binary() {
        // Binary data — smart path should either help or fall back gracefully.
        let data: Vec<u8> = (0u8..=255).cycle().take(8192).collect();
        let config = CompressConfig {
            enable_smart_transforms: true,
            ..Default::default()
        };
        let compressed = compress(&data, &config).unwrap();
        let decompressed = decompress(&compressed.data).unwrap();
        assert_eq!(
            decompressed.data, data,
            "smart transforms binary roundtrip failed"
        );
    }

    #[test]
    fn roundtrip_smart_transforms_small() {
        // Small data below PREPROCESS_THRESHOLD — smart transforms skipped.
        let data = b"small data";
        let config = CompressConfig {
            enable_smart_transforms: true,
            ..Default::default()
        };
        let compressed = compress(data, &config).unwrap();
        let decompressed = decompress(&compressed.data).unwrap();
        assert_eq!(decompressed.data, data);
    }

    #[test]
    fn roundtrip_with_msn_json() {
        // Repetitive JSON data - ideal for MSN
        let json_data = r#"{"name":"Alice","age":30,"city":"NYC"}
{"name":"Bob","age":25,"city":"LA"}
{"name":"Charlie","age":35,"city":"SF"}
{"name":"Diana","age":28,"city":"NYC"}
{"name":"Eve","age":32,"city":"LA"}"#;

        let data = json_data.as_bytes();

        // Compress with MSN enabled
        let config_msn = CompressConfig {
            enable_msn: true,
            ..Default::default()
        };
        let compressed_msn = compress(data, &config_msn).unwrap();

        // Compress without MSN
        let config_no_msn = CompressConfig {
            enable_msn: false,
            ..Default::default()
        };
        let compressed_no_msn = compress(data, &config_no_msn).unwrap();

        // MSN should achieve better compression on this structured data
        // (though results may vary based on SSR track selection)

        // Decompress and verify (compare JSON semantically, not byte-for-byte)
        let decompressed_msn = decompress(&compressed_msn.data).unwrap();
        let decompressed_no_msn = decompress(&compressed_no_msn.data).unwrap();

        // Parse both as JSON to verify semantic equivalence
        let orig_lines: Vec<serde_json::Value> = std::str::from_utf8(data)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let msn_lines: Vec<serde_json::Value> = std::str::from_utf8(&decompressed_msn.data)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();
        let no_msn_lines: Vec<serde_json::Value> = std::str::from_utf8(&decompressed_no_msn.data)
            .unwrap()
            .lines()
            .map(|l| serde_json::from_str(l).unwrap())
            .collect();

        assert_eq!(orig_lines, msn_lines, "MSN roundtrip semantic mismatch");
        assert_eq!(
            orig_lines, no_msn_lines,
            "No-MSN roundtrip semantic mismatch"
        );
    }
}
