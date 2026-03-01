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
/// Pipeline: SSR → preprocess → entropy → frame.
/// Uses profile-driven DAG when `config.profile` is set, otherwise
/// falls back to the SSR-guided TP preprocess orchestrator.
#[must_use = "compression result is returned"]
pub fn compress(data: &[u8], config: &CompressConfig) -> CpacResult<CompressResult> {
    let original_size = data.len();

    // 1. SSR analysis
    let ssr = cpac_ssr::analyze(data);

    // 2. Select backend
    let backend = config
        .backend
        .unwrap_or_else(|| cpac_entropy::auto_select_backend(ssr.entropy_estimate));

    // 3. Preprocess (transforms)
    let transform_ctx = cpac_transforms::TransformContext {
        entropy_estimate: ssr.entropy_estimate,
        ascii_ratio: ssr.ascii_ratio,
        data_size: ssr.data_size,
    };
    // Use SSR-guided TP preprocess (generic profile / default)
    let (preprocessed, _transform_meta) = cpac_transforms::preprocess(data, &transform_ctx);

    // 4. Entropy coding
    let compressed_payload = cpac_entropy::compress(&preprocessed, backend)?;

    // 5. Frame encoding (empty DAG descriptor — preprocess metadata embedded in TP frame)
    let frame = cpac_frame::encode_frame(&compressed_payload, backend, original_size, &[]);

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
/// Pipeline: decode frame → entropy decompress → unpreprocess.
/// Supports both TP-frame preprocess and DAG-based decompression.
#[must_use = "decompression result is returned"]
pub fn decompress(data: &[u8]) -> CpacResult<DecompressResult> {
    // 1. Decode frame
    let (header, payload) = cpac_frame::decode_frame(data)?;

    // 2. Entropy decompress
    let decompressed_payload = cpac_entropy::decompress(payload, header.backend)?;

    // 3. Reverse transforms
    let result = if !header.dag_descriptor.is_empty() {
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

    // Verify size if available
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
}
