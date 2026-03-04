// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
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

/// Default block size: 1 MiB.
pub const DEFAULT_BLOCK_SIZE: usize = 1 << 20;

/// Minimum input size to trigger parallel compression (256 KiB).
pub const PARALLEL_THRESHOLD: usize = 256 * 1024;

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

    // Configure rayon thread pool
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads.max(1))
        .build()
        .map_err(|e| CpacError::CompressFailed(format!("rayon pool: {e}")))?;

    // Compress blocks in parallel with disable_parallel flag to prevent recursion
    let mut block_config = config.clone();
    block_config.disable_parallel = true;

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

    // Decompress blocks in parallel
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(num_threads.max(1))
        .build()
        .map_err(|e| CpacError::DecompressFailed(format!("rayon pool: {e}")))?;

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
}
