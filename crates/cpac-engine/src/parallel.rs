// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Block-parallel compression and decompression using rayon.
//!
//! Wire format (CPBL):
//! ```text
//! "CPBL" (4B) | version (1B) | block_count (4B LE) | original_size (8B LE)
//! | [compressed_block_size: u32 LE] × block_count
//! | [block_data] × block_count
//! ```
//!
//! Each block is independently compressed using the normal CPAC pipeline,
//! allowing full parallel decompression.

use cpac_types::{CompressConfig, CompressResult, CpacError, CpacResult, DecompressResult};
use rayon::prelude::*;

/// CPBL magic bytes (block-parallel format).
pub const CPBL_MAGIC: &[u8; 4] = b"CPBL";

/// Current CPBL format version.
pub const CPBL_VERSION: u8 = 1;

/// Default block size: 4 MiB.
///
/// Larger blocks give zstd more cross-block context, reducing the ratio
/// penalty of block-parallel compression vs single-stream.  Benchmarks
/// showed 1 MiB blocks lost 15-22% ratio on medium-compressible files
/// (mr, mozilla, ooffice) vs standalone zstd-3 on the full file.
pub const DEFAULT_BLOCK_SIZE: usize = 4 << 20;

/// Minimum input size to trigger parallel compression (4 MiB).
///
/// Files below this size are compressed single-threaded with full-file
/// context.  The previous 256 KiB threshold was too aggressive — block
/// overhead dominated for files under ~10 MB.
pub const PARALLEL_THRESHOLD: usize = 4 * 1024 * 1024;

/// P3: Higher threshold for text-heavy data.  Structured text (JSON, logs,
/// YAML, config) benefits far more from full-file LZ77 context than from
/// parallelism.  Files below 16 MiB that are mostly ASCII stay on the
/// single-stream path for better ratios and lower preprocessing overhead.
pub const PARALLEL_THRESHOLD_TEXT: usize = 16 * 1024 * 1024;

/// Small block size: 4 MiB.  Used for high-entropy or small files.
pub const BLOCK_SIZE_SMALL: usize = 4 << 20;
/// Medium block size: 16 MiB.  Used for medium-entropy data.
pub const BLOCK_SIZE_MEDIUM: usize = 16 << 20;
/// Large block size: 32 MiB.  Used for low-entropy, highly-compressible data.
pub const BLOCK_SIZE_LARGE: usize = 32 << 20;

/// Phase 4C: Choose block size adaptively based on entropy and file size.
///
/// Heuristic:
/// - Low entropy (< 4.0 bits/byte): large blocks (32 MB) — zstd benefits from
///   more LZ77 context on highly-redundant data (e.g. logs, YAML).
/// - Medium entropy (4.0–6.5): medium blocks (16 MB) — balanced.
/// - High entropy (> 6.5): small blocks (4 MB) — little back-reference benefit,
///   prefer more parallelism.
///
/// Additionally, file size matters: if the file is < 64 MB, large blocks would
/// produce too few blocks for effective parallelism.
#[must_use]
pub fn adaptive_block_size(entropy_estimate: f64, file_size: usize) -> usize {
    // For smaller files, cap block size so we get at least 2 blocks
    let max_block = file_size / 2;

    let ideal = if entropy_estimate < 4.0 {
        BLOCK_SIZE_LARGE
    } else if entropy_estimate < 6.5 {
        BLOCK_SIZE_MEDIUM
    } else {
        BLOCK_SIZE_SMALL
    };

    ideal.min(max_block).max(BLOCK_SIZE_SMALL)
}

/// CPBL header: magic(4) + version(1) + `block_count(4)` + `original_size(8)` = 17 bytes.
const CPBL_HEADER_SIZE: usize = 4 + 1 + 4 + 8;

/// Check whether the given data starts with the CPBL magic.
#[must_use]
pub fn is_cpbl(data: &[u8]) -> bool {
    data.len() >= 4 && &data[..4] == CPBL_MAGIC
}

/// Compress data using block-parallel pipeline.
///
/// Splits `data` into blocks of `block_size` bytes, compresses each block
/// independently using the standard CPAC pipeline (in parallel via rayon),
/// and encodes them into the CPBL wire format.
pub fn compress_parallel(
    data: &[u8],
    config: &CompressConfig,
    block_size: usize,
    num_threads: usize,
) -> CpacResult<CompressResult> {
    let original_size = data.len();
    let bs = if block_size == 0 {
        DEFAULT_BLOCK_SIZE
    } else {
        block_size
    };

    // Split into blocks
    let blocks: Vec<&[u8]> = data.chunks(bs).collect();
    let block_count = blocks.len();
    cpac_trace!("[TRACE] compress_parallel: size={}B block_size={}B blocks={} threads={}",
        original_size, bs, block_count, num_threads);

    // Use shared global thread pool (Phase 4B) instead of creating a new one per call.
    let pool = crate::pool::get_or_init_thread_pool(num_threads);

    // Compress blocks in parallel with disable_parallel flag to prevent recursion
    let mut block_config = config.clone();
    block_config.disable_parallel = true;

    // P1: skip expensive transforms (BWT) on parallel sub-blocks — BWT on
    // multi-MB blocks is expensive and block-parallel framing already
    // destroys cross-block context that BWT relies on.
    block_config.skip_expensive_transforms = true;

    // Phase 4A: MSN field-map caching — probe the first block to discover
    // the domain and build the field map, then reuse it across all blocks.
    // This avoids O(N_blocks × N_domains) detection overhead for large
    // homogeneous files (e.g. 100 MB YAML split into 25 × 4 MB blocks).
    if config.enable_msn && block_config.cached_msn_metadata.is_none() && !blocks.is_empty() {
        let probe_filename = config.filename.as_deref();
        if let Ok(probe_result) =
            cpac_msn::extract(blocks[0], probe_filename, config.msn_confidence)
        {
            cpac_trace!("[TRACE] parallel MSN probe: applied={} domain={:?} conf={:.3} fields={}",
                probe_result.applied, probe_result.domain_id, probe_result.confidence, probe_result.fields.len());
            if probe_result.applied {
                if let Ok(encoded) = cpac_msn::encode_metadata_compact(&probe_result.metadata()) {
                    cpac_trace!("[TRACE] parallel MSN probe: cached metadata={}B", encoded.len());
                    block_config.cached_msn_metadata = Some(encoded);
                }
            }
        } else {
            cpac_trace!("[TRACE] parallel MSN probe: extract failed");
        }
    }

    // P2+P9+P10: Cache transform recommendations AND SSR from the first block
    // and reuse across all blocks.  For homogeneous files (JSON, logs), every
    // block has the same structure, so re-running analyze_structure/SSR is waste.
    if block_config.cached_transform_recs.is_none() && !blocks.is_empty() {
        let probe_ssr = cpac_ssr::analyze(blocks[0]);

        // P9: cache SSR so per-block compress() calls skip cpac_ssr::analyze().
        block_config.cached_ssr =
            Some(cpac_types::CachedSsr::from(&probe_ssr));

        let probe_profile = crate::analyzer::analyze_structure_fast(
            &probe_ssr,
            config.filename.as_deref(),
            block_config.skip_expensive_transforms,
        );
        let rec_names: Vec<String> = probe_profile
            .recommended_chain
            .iter()
            .filter(|r| r.confidence >= crate::SMART_MIN_CONFIDENCE)
            .map(|r| r.name.clone())
            .collect();
        cpac_trace!("[TRACE] parallel probe SSR: entropy={:.3} ascii={:.3} track={:?}",
            probe_ssr.entropy_estimate, probe_ssr.ascii_ratio, probe_ssr.track);
        cpac_trace!("[TRACE] parallel probe transforms: {:?} (all: {:?})",
            rec_names, probe_profile.recommended_chain.iter().map(|r| format!("{}:{:.2}", r.name, r.confidence)).collect::<Vec<_>>());
        if !rec_names.is_empty() {
            block_config.cached_transform_recs = Some(rec_names);
        } else {
            // P10: no transforms recommended — disable smart_preprocess for
            // all blocks to avoid the per-block trial overhead entirely.
            cpac_trace!("[TRACE] parallel: P10 disabling smart_transforms (no recs)");
            block_config.enable_smart_transforms = false;
        }
    }

    let compressed_blocks: Vec<CpacResult<Vec<u8>>> = pool.install(|| {
        blocks
            .par_iter()
            .map(|block| {
                let result = crate::compress(block, &block_config)?;
                Ok(result.data)
            })
            .collect()
    });

    // Collect results, propagating any errors
    let mut block_data: Vec<Vec<u8>> = Vec::with_capacity(block_count);
    for result in compressed_blocks {
        block_data.push(result?);
    }

    // Encode CPBL wire format
    let block_sizes: Vec<u32> = block_data.iter().map(|b| b.len() as u32).collect();
    let payload_size: usize = block_data.iter().map(std::vec::Vec::len).sum();
    let total = CPBL_HEADER_SIZE + block_count * 4 + payload_size;
    let mut out = Vec::with_capacity(total);

    // Header
    out.extend_from_slice(CPBL_MAGIC);
    out.push(CPBL_VERSION);
    out.extend_from_slice(&(block_count as u32).to_le_bytes());
    out.extend_from_slice(&(original_size as u64).to_le_bytes());

    // Block size table
    for &sz in &block_sizes {
        out.extend_from_slice(&sz.to_le_bytes());
    }

    // Block payloads
    for block in &block_data {
        out.extend_from_slice(block);
    }

    let compressed_size = out.len();

    Ok(CompressResult {
        data: out,
        original_size,
        compressed_size,
        track: cpac_types::Track::Track2,
        backend: config.backend.unwrap_or(cpac_types::Backend::Zstd),
    })
}

/// Decompress CPBL block-parallel data.
///
/// Reads the CPBL header, locates block boundaries, decompresses each
/// block in parallel, and concatenates the results.
pub fn decompress_parallel(data: &[u8], num_threads: usize) -> CpacResult<DecompressResult> {
    if data.len() < CPBL_HEADER_SIZE {
        return Err(CpacError::DecompressFailed("CPBL data too short".into()));
    }

    // Parse header
    if &data[..4] != CPBL_MAGIC {
        return Err(CpacError::DecompressFailed("not a CPBL frame".into()));
    }

    let version = data[4];
    if version != CPBL_VERSION {
        return Err(CpacError::DecompressFailed(format!(
            "unsupported CPBL version: {version}"
        )));
    }

    let block_count = u32::from_le_bytes([data[5], data[6], data[7], data[8]]) as usize;
    let original_size = u64::from_le_bytes([
        data[9], data[10], data[11], data[12], data[13], data[14], data[15], data[16],
    ]) as usize;

    // Read block size table
    let table_start = CPBL_HEADER_SIZE;
    let table_end = table_start + block_count * 4;
    if data.len() < table_end {
        return Err(CpacError::DecompressFailed(
            "CPBL truncated block size table".into(),
        ));
    }

    let mut block_sizes = Vec::with_capacity(block_count);
    for i in 0..block_count {
        let off = table_start + i * 4;
        let sz =
            u32::from_le_bytes([data[off], data[off + 1], data[off + 2], data[off + 3]]) as usize;
        block_sizes.push(sz);
    }

    // Locate block boundaries
    let mut blocks: Vec<&[u8]> = Vec::with_capacity(block_count);
    let mut cursor = table_end;
    for &sz in &block_sizes {
        if cursor + sz > data.len() {
            return Err(CpacError::DecompressFailed(
                "CPBL truncated block payload".into(),
            ));
        }
        blocks.push(&data[cursor..cursor + sz]);
        cursor += sz;
    }

    // Use shared global thread pool (Phase 4B) instead of creating a new one per call.
    let pool = crate::pool::get_or_init_thread_pool(num_threads);

    let decompressed_blocks: Vec<CpacResult<Vec<u8>>> = pool.install(|| {
        blocks
            .par_iter()
            .map(|block| {
                let result = crate::decompress(block)?;
                Ok(result.data)
            })
            .collect()
    });

    // Concatenate results
    let mut output = Vec::with_capacity(original_size);
    for result in decompressed_blocks {
        output.extend_from_slice(&result?);
    }

    // Verify size
    if output.len() != original_size {
        return Err(CpacError::DecompressFailed(format!(
            "CPBL size mismatch: expected {original_size}, got {}",
            output.len()
        )));
    }

    Ok(DecompressResult {
        data: output,
        success: true,
        error: None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpbl_roundtrip_basic() {
        let data: Vec<u8> = b"Hello parallel CPAC world! ".repeat(100);
        let config = CompressConfig::default();
        let compressed = compress_parallel(&data, &config, 512, 2).unwrap();
        assert!(is_cpbl(&compressed.data));
        let decompressed = decompress_parallel(&compressed.data, 2).unwrap();
        assert_eq!(decompressed.data, data);
    }

    #[test]
    fn cpbl_roundtrip_large() {
        let data: Vec<u8> = (0u8..=255)
            .cycle()
            .take(DEFAULT_BLOCK_SIZE * 3 + 100)
            .collect();
        let config = CompressConfig::default();
        let compressed = compress_parallel(&data, &config, DEFAULT_BLOCK_SIZE, 4).unwrap();
        assert!(is_cpbl(&compressed.data));
        let decompressed = decompress_parallel(&compressed.data, 4).unwrap();
        assert_eq!(decompressed.data, data);
    }

    #[test]
    fn cpbl_roundtrip_single_block() {
        let data = b"small data fits in one block";
        let config = CompressConfig::default();
        let compressed = compress_parallel(data, &config, DEFAULT_BLOCK_SIZE, 1).unwrap();
        assert!(is_cpbl(&compressed.data));
        let decompressed = decompress_parallel(&compressed.data, 1).unwrap();
        assert_eq!(decompressed.data, data);
    }

    #[test]
    fn is_cpbl_detection() {
        assert!(is_cpbl(b"CPBLsomething"));
        assert!(!is_cpbl(b"CPsomething")); // regular CPAC frame
        assert!(!is_cpbl(b"XX"));
    }

    #[test]
    fn cpbl_bad_magic() {
        let result = decompress_parallel(b"XXBLjunkdata1234567890", 1);
        assert!(result.is_err());
    }

    #[test]
    fn cpbl_empty_data() {
        let data = b"";
        let config = CompressConfig::default();
        let compressed = compress_parallel(data, &config, DEFAULT_BLOCK_SIZE, 1).unwrap();
        let decompressed = decompress_parallel(&compressed.data, 1).unwrap();
        assert_eq!(decompressed.data, data);
    }

    #[test]
    fn adaptive_block_size_low_entropy() {
        // Low entropy → large blocks (32 MB) when file is large enough
        let bs = adaptive_block_size(2.5, 256 << 20);
        assert_eq!(bs, BLOCK_SIZE_LARGE);
    }

    #[test]
    fn adaptive_block_size_medium_entropy() {
        // Medium entropy → medium blocks (16 MB) when file is large enough
        let bs = adaptive_block_size(5.0, 256 << 20);
        assert_eq!(bs, BLOCK_SIZE_MEDIUM);
    }

    #[test]
    fn adaptive_block_size_high_entropy() {
        // High entropy → small blocks (4 MB)
        let bs = adaptive_block_size(7.5, 256 << 20);
        assert_eq!(bs, BLOCK_SIZE_SMALL);
    }

    #[test]
    fn adaptive_block_size_small_file_clamp() {
        // Even with low entropy, small file → clamp to BLOCK_SIZE_SMALL
        let bs = adaptive_block_size(2.5, 8 << 20);
        assert_eq!(bs, BLOCK_SIZE_SMALL);
    }
}
