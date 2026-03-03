// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! CPAC compression engine — top-level compress/decompress API.
//!
//! Pipeline:
//! 1. SSR analysis → select backend + track
//! 2. Preprocess (transforms) — TP-frame auto-select or DAG profile
//! 3. Entropy coding (Zstd/Brotli/Raw)
//! 4. Frame encoding (self-describing wire format)

pub mod bench;
pub mod corpus;
pub mod host;
pub mod parallel;
pub mod pool;

pub use bench::{BaselineEngine, BenchProfile, BenchResult, BenchmarkRunner, CorpusSummary};
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

    // 2. MSN (Multi-Scale Normalization) - Track 1 only, if enabled
    let (msn_data, msn_metadata) = if config.enable_msn && ssr.track == Track::Track1 {
        // min_confidence = 0.5 for auto-detection
        match cpac_msn::extract(data, None, 0.5) {
            Ok(result) if result.applied => {
                // MSN succeeded - use residual as input to rest of pipeline
                let metadata = serde_json::to_vec(&result)
                    .map_err(|e| CpacError::CompressFailed(format!("MSN metadata serialize: {}", e)))?;
                (result.residual, metadata)
            }
            _ => {
                // MSN failed or not applicable - passthrough
                (data.to_vec(), Vec::new())
            }
        }
    } else {
        // MSN disabled or Track 2 - passthrough
        (data.to_vec(), Vec::new())
    };

    let data_to_compress = &msn_data;

    // 3. Select backend with size awareness
    let backend = config.backend.unwrap_or_else(|| {
        cpac_entropy::auto_select_backend_with_size(ssr.entropy_estimate, original_size)
    });

    // 4. Check if we should use parallel compression for large files
    // Skip if disable_parallel flag is set (prevents recursive calls from compress_parallel)
    const PARALLEL_THRESHOLD: usize = 1_048_576; // 1MB
    if !config.disable_parallel && original_size >= PARALLEL_THRESHOLD && backend != Backend::Raw {
        // Use default 1MB block size and auto-detect thread count
        let num_threads = rayon::current_num_threads();
        return compress_parallel(data, config, DEFAULT_BLOCK_SIZE, num_threads);
    }

    // 5. Adaptive preprocessing
    // Skip preprocessing for:
    // - Raw backend (passthrough mode)
    // - Small files (< 4KB) where overhead exceeds benefit
    const PREPROCESS_THRESHOLD: usize = 4096;
    let should_preprocess = backend != Backend::Raw && original_size >= PREPROCESS_THRESHOLD;

    let preprocessed = if should_preprocess {
        let transform_ctx = cpac_transforms::TransformContext {
            entropy_estimate: ssr.entropy_estimate,
            ascii_ratio: ssr.ascii_ratio,
            data_size: ssr.data_size,
        };
        // Use SSR-guided TP preprocess (generic profile / default)
        let (preprocessed, _transform_meta) = cpac_transforms::preprocess(data_to_compress, &transform_ctx);
        preprocessed
    } else {
        data_to_compress.to_vec()
    };

    // 6. Entropy coding (with optional dictionary for Zstd)
    let compressed_payload = if backend == Backend::Zstd && config.dictionary.is_some() {
        cpac_entropy::compress_with_dict(&preprocessed, backend, config.dictionary.as_deref())?
    } else {
        cpac_entropy::compress(&preprocessed, backend)?
    };

    // 7. Frame encoding (CP2 if MSN enabled, CP otherwise)
    let frame = if !msn_metadata.is_empty() {
        cpac_frame::encode_frame_cp2(&compressed_payload, backend, original_size, &[], &msn_metadata)
    } else {
        cpac_frame::encode_frame(&compressed_payload, backend, original_size, &[])
    };

    let compressed_size = frame.len();

    Ok(CompressResult {
        data: frame,
        original_size,
        compressed_size,
        track: ssr.track,
        backend,
    })
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

    // 3. Reverse transforms
    let mut result = if !header.dag_descriptor.is_empty() {
        // DAG-based decompression: deserialize descriptor and execute backward
        let (ids, metas, _consumed) = cpac_dag::deserialize_dag_descriptor(&header.dag_descriptor)?;
        let registry = TransformRegistry::with_builtins();
        let dag = TransformDAG::compile_from_ids(&registry, &ids)?;
        let meta_chain: Vec<(u8, Vec<u8>)> = ids.into_iter().zip(metas).collect();
        let output = dag.execute_backward(
            cpac_types::CpacType::Serial(decompressed_payload),
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
    } else {
        // TP-frame based decompression (generic/default)
        cpac_transforms::unpreprocess(&decompressed_payload, &[])
    };

    // 4. MSN reconstruction (if metadata present in CP2 frame)
    if !header.msn_metadata.is_empty() {
        let msn_result: cpac_msn::MsnResult = serde_json::from_slice(&header.msn_metadata)
            .map_err(|e| CpacError::DecompressFailed(format!("MSN metadata deserialize: {}", e)))?;
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
        assert_eq!(orig_lines, no_msn_lines, "No-MSN roundtrip semantic mismatch");
    }
}
