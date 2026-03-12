// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Block-based streaming I/O and parallel compression for CPAC.
//!
//! Splits large data into fixed-size blocks, compresses each independently
//! (optionally in parallel via rayon), and reassembles on decompression.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_sign_loss,
    clippy::missing_errors_doc,
    clippy::missing_panics_doc
)]

pub mod cloud;
pub mod cpce;
pub mod mmap;
pub mod stream;

use cpac_types::{CompressConfig, CompressResult, CpacError, CpacResult, DecompressResult};
use rayon::prelude::*;

/// Default block size: 1 MB.
pub const DEFAULT_BLOCK_SIZE: usize = 1 << 20;

/// Magic bytes for a streaming frame.
const STREAM_MAGIC: &[u8; 2] = b"CS";

/// Streaming frame version.
const STREAM_VERSION: u8 = 1;

/// Flags bit 0: MSN metadata present (1) or absent (0)
const FLAG_MSN_ENABLED: u16 = 1 << 0;

/// Default MSN detection buffer size: 64 KB.
const DEFAULT_MSN_DETECTION_BUFFER: usize = 64 * 1024;

/// MSN configuration for streaming compression.
#[derive(Clone, Debug)]
pub struct MsnConfig {
    /// Enable MSN for streaming
    pub enable: bool,
    /// Minimum confidence threshold (0.0-1.0)
    pub confidence_threshold: f64,
    /// Buffer size for initial domain detection
    pub detection_buffer_size: usize,
}

impl Default for MsnConfig {
    fn default() -> Self {
        Self {
            enable: true,
            confidence_threshold: 0.7,
            detection_buffer_size: DEFAULT_MSN_DETECTION_BUFFER,
        }
    }
}

impl MsnConfig {
    /// Create disabled MSN config.
    #[must_use]
    pub fn disabled() -> Self {
        Self {
            enable: false,
            confidence_threshold: 0.0,
            detection_buffer_size: 0,
        }
    }
}

/// Compress data in blocks, optionally in parallel.
///
/// Returns a streaming frame containing all compressed blocks.
pub fn compress_streaming(
    data: &[u8],
    config: &CompressConfig,
    block_size: usize,
    parallel: bool,
) -> CpacResult<CompressResult> {
    let block_sz = if block_size == 0 {
        DEFAULT_BLOCK_SIZE
    } else {
        block_size
    };
    let blocks: Vec<&[u8]> = data.chunks(block_sz).collect();
    let num_blocks = blocks.len();

    let compressed_blocks: Vec<CpacResult<Vec<u8>>> = if parallel {
        blocks
            .par_iter()
            .map(|block| cpac_engine::compress(block, config).map(|r| r.data))
            .collect()
    } else {
        blocks
            .iter()
            .map(|block| cpac_engine::compress(block, config).map(|r| r.data))
            .collect()
    };

    // Check for errors
    let mut frame_blocks = Vec::with_capacity(num_blocks);
    for result in compressed_blocks {
        frame_blocks.push(result?);
    }

    // Build streaming frame:
    // [CS][version][flags:2 LE][num_blocks:4 LE][original_size:8 LE][block_size:4 LE]
    // [msn_len:2 LE][msn_metadata][per block: compressed_len:4 LE + data]
    let total_payload: usize = frame_blocks.iter().map(|b| 4 + b.len()).sum();
    let flags: u16 = 0; // No MSN for now
    let msn_metadata: Vec<u8> = Vec::new();
    let header_size = 2 + 1 + 2 + 4 + 8 + 4 + 2 + msn_metadata.len();
    let mut frame = Vec::with_capacity(header_size + total_payload);
    frame.extend_from_slice(STREAM_MAGIC);
    frame.push(STREAM_VERSION);
    frame.extend_from_slice(&flags.to_le_bytes());
    frame.extend_from_slice(&(num_blocks as u32).to_le_bytes());
    frame.extend_from_slice(&(data.len() as u64).to_le_bytes());
    frame.extend_from_slice(&(block_sz as u32).to_le_bytes());
    frame.extend_from_slice(&(msn_metadata.len() as u16).to_le_bytes());
    frame.extend_from_slice(&msn_metadata);
    for block in &frame_blocks {
        frame.extend_from_slice(&(block.len() as u32).to_le_bytes());
        frame.extend_from_slice(block);
    }

    let compressed_size = frame.len();
    Ok(CompressResult {
        data: frame,
        original_size: data.len(),
        compressed_size,
        track: cpac_types::Track::Track2,
        backend: config.backend.unwrap_or(cpac_types::Backend::Zstd),
        msn_applied: false,
    })
}

/// Decompress a streaming frame, optionally in parallel.
pub fn decompress_streaming(data: &[u8], parallel: bool) -> CpacResult<DecompressResult> {
    // Min header: CS(2) + version(1) + flags(2) + num_blocks(4) + orig_size(8) + block_size(4) + msn_len(2) = 23
    if data.len() < 23 || &data[0..2] != STREAM_MAGIC {
        return Err(CpacError::InvalidFrame("not a streaming frame".into()));
    }
    if data[2] != STREAM_VERSION {
        return Err(CpacError::InvalidFrame(
            "unsupported streaming version".into(),
        ));
    }

    let flags = u16::from_le_bytes([data[3], data[4]]);
    let num_blocks = u32::from_le_bytes([data[5], data[6], data[7], data[8]]) as usize;
    let original_size = u64::from_le_bytes([
        data[9], data[10], data[11], data[12], data[13], data[14], data[15], data[16],
    ]) as usize;
    let _block_size = u32::from_le_bytes([data[17], data[18], data[19], data[20]]) as usize;
    let msn_len = u16::from_le_bytes([data[21], data[22]]) as usize;

    // Parse optional MSN metadata
    let _msn_metadata: Option<cpac_msn::MsnMetadata> = if flags & FLAG_MSN_ENABLED != 0 {
        if data.len() < 23 + msn_len {
            return Err(CpacError::InvalidFrame("truncated MSN metadata".into()));
        }
        let msn_bytes = &data[23..23 + msn_len];
        Some(
            serde_json::from_slice(msn_bytes)
                .map_err(|e| CpacError::InvalidFrame(format!("MSN deserialize: {e}")))?,
        )
    } else {
        None
    };

    // Parse block offsets
    let mut offset = 23 + msn_len;
    let mut block_data: Vec<&[u8]> = Vec::with_capacity(num_blocks);
    for _ in 0..num_blocks {
        if offset + 4 > data.len() {
            return Err(CpacError::InvalidFrame("truncated block header".into()));
        }
        let block_len = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;
        if offset + block_len > data.len() {
            return Err(CpacError::InvalidFrame("truncated block data".into()));
        }
        block_data.push(&data[offset..offset + block_len]);
        offset += block_len;
    }

    let decompressed_blocks: Vec<CpacResult<Vec<u8>>> = if parallel {
        block_data
            .par_iter()
            .map(|block| cpac_engine::decompress(block).map(|r| r.data))
            .collect()
    } else {
        block_data
            .iter()
            .map(|block| cpac_engine::decompress(block).map(|r| r.data))
            .collect()
    };

    let mut result = Vec::with_capacity(original_size);
    for block_result in decompressed_blocks {
        result.extend_from_slice(&block_result?);
    }

    if result.len() != original_size {
        return Err(CpacError::DecompressFailed(format!(
            "size mismatch: expected {original_size}, got {}",
            result.len()
        )));
    }

    Ok(DecompressResult {
        data: result,
        success: true,
        error: None,
    })
}

/// Check if data is a streaming frame.
#[must_use]
pub fn is_streaming_frame(data: &[u8]) -> bool {
    data.len() >= 23 && &data[0..2] == STREAM_MAGIC && data[2] == STREAM_VERSION
}

// ---------------------------------------------------------------------------
// Progress tracking
// ---------------------------------------------------------------------------

/// Progress information for streaming operations.
#[derive(Clone, Debug)]
pub struct ProgressInfo {
    pub bytes_processed: usize,
    pub total_bytes: usize,
    pub blocks_done: usize,
    pub blocks_total: usize,
    pub elapsed_secs: f64,
}

impl ProgressInfo {
    /// Throughput in MB/s.
    #[must_use]
    pub fn throughput_mbs(&self) -> f64 {
        if self.elapsed_secs > 0.0 {
            self.bytes_processed as f64 / 1_048_576.0 / self.elapsed_secs
        } else {
            0.0
        }
    }
    /// Estimated time remaining in seconds.
    #[must_use]
    pub fn eta_seconds(&self) -> f64 {
        if self.bytes_processed > 0 && self.total_bytes > self.bytes_processed {
            let rate = self.bytes_processed as f64 / self.elapsed_secs;
            (self.total_bytes - self.bytes_processed) as f64 / rate
        } else {
            0.0
        }
    }
    /// Completion fraction 0.0..1.0.
    #[must_use]
    pub fn fraction(&self) -> f64 {
        if self.total_bytes > 0 {
            self.bytes_processed as f64 / self.total_bytes as f64
        } else {
            1.0
        }
    }
}

/// Compress streaming with progress callback.
pub fn compress_streaming_with_progress(
    data: &[u8],
    config: &CompressConfig,
    block_size: usize,
    callback: &dyn Fn(&ProgressInfo),
) -> CpacResult<CompressResult> {
    let block_sz = if block_size == 0 {
        DEFAULT_BLOCK_SIZE
    } else {
        block_size
    };
    let blocks: Vec<&[u8]> = data.chunks(block_sz).collect();
    let num_blocks = blocks.len();
    let start = std::time::Instant::now();

    let mut frame_blocks = Vec::with_capacity(num_blocks);
    for (i, block) in blocks.iter().enumerate() {
        let result = cpac_engine::compress(block, config)?;
        frame_blocks.push(result.data);
        callback(&ProgressInfo {
            bytes_processed: (i + 1) * block_sz,
            total_bytes: data.len(),
            blocks_done: i + 1,
            blocks_total: num_blocks,
            elapsed_secs: start.elapsed().as_secs_f64(),
        });
    }

    // Build frame (same format as compress_streaming)
    let total_payload: usize = frame_blocks.iter().map(|b| 4 + b.len()).sum();
    let flags: u16 = 0; // No MSN for now
    let msn_metadata: Vec<u8> = Vec::new();
    let header_size = 2 + 1 + 2 + 4 + 8 + 4 + 2 + msn_metadata.len();
    let mut frame = Vec::with_capacity(header_size + total_payload);
    frame.extend_from_slice(STREAM_MAGIC);
    frame.push(STREAM_VERSION);
    frame.extend_from_slice(&flags.to_le_bytes());
    frame.extend_from_slice(&(num_blocks as u32).to_le_bytes());
    frame.extend_from_slice(&(data.len() as u64).to_le_bytes());
    frame.extend_from_slice(&(block_sz as u32).to_le_bytes());
    frame.extend_from_slice(&(msn_metadata.len() as u16).to_le_bytes());
    frame.extend_from_slice(&msn_metadata);
    for block in &frame_blocks {
        frame.extend_from_slice(&(block.len() as u32).to_le_bytes());
        frame.extend_from_slice(block);
    }

    let compressed_size = frame.len();
    Ok(CompressResult {
        data: frame,
        original_size: data.len(),
        compressed_size,
        track: cpac_types::Track::Track2,
        backend: config.backend.unwrap_or(cpac_types::Backend::Zstd),
        msn_applied: false,
    })
}

// ---------------------------------------------------------------------------
// Adaptive block sizing
// ---------------------------------------------------------------------------

/// Configuration for adaptive block sizing.
#[derive(Clone, Debug)]
pub struct AdaptiveBlockConfig {
    pub min_block: usize,
    pub max_block: usize,
}

impl Default for AdaptiveBlockConfig {
    fn default() -> Self {
        Self {
            min_block: 64 * 1024,       // 64 KB
            max_block: 4 * 1024 * 1024, // 4 MB
        }
    }
}

impl AdaptiveBlockConfig {
    /// Choose block size based on data entropy estimate.
    /// High entropy → larger blocks (less overhead), low entropy → smaller (faster).
    #[must_use]
    pub fn select_block_size(&self, entropy: f64, data_size: usize) -> usize {
        let ratio = (entropy / 8.0).clamp(0.0, 1.0);
        let size = self.min_block + ((self.max_block - self.min_block) as f64 * ratio) as usize;
        size.min(data_size).max(self.min_block)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_roundtrip_sequential() {
        let data: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        let config = CompressConfig::default();
        let compressed = compress_streaming(&data, &config, 1024, false).unwrap();
        assert!(is_streaming_frame(&compressed.data));
        let decompressed = decompress_streaming(&compressed.data, false).unwrap();
        assert_eq!(decompressed.data, data);
    }

    #[test]
    fn streaming_roundtrip_parallel() {
        let data: Vec<u8> = b"The quick brown fox ".repeat(500);
        let config = CompressConfig::default();
        let compressed = compress_streaming(&data, &config, 2048, true).unwrap();
        let decompressed = decompress_streaming(&compressed.data, true).unwrap();
        assert_eq!(decompressed.data, data);
    }

    #[test]
    fn streaming_small_data() {
        let data = b"small";
        let config = CompressConfig::default();
        let compressed = compress_streaming(data, &config, DEFAULT_BLOCK_SIZE, false).unwrap();
        let decompressed = decompress_streaming(&compressed.data, false).unwrap();
        assert_eq!(decompressed.data, data);
    }

    #[test]
    fn not_streaming_frame() {
        assert!(!is_streaming_frame(b"CP"));
        assert!(!is_streaming_frame(b"XX"));
    }

    #[test]
    fn streaming_with_progress_roundtrip() {
        use std::cell::Cell;
        let data: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        let config = CompressConfig::default();
        let calls = Cell::new(0usize);
        let compressed = compress_streaming_with_progress(&data, &config, 1024, &|info| {
            calls.set(calls.get() + 1);
            assert!(info.fraction() <= 1.0);
            assert!(info.blocks_done <= info.blocks_total);
        })
        .unwrap();
        assert!(calls.get() > 0);
        // Should produce valid streaming frame decompressible with the normal path
        let decompressed = decompress_streaming(&compressed.data, false).unwrap();
        assert_eq!(decompressed.data, data);
    }

    #[test]
    fn progress_info_calculations() {
        let info = ProgressInfo {
            bytes_processed: 1_048_576,
            total_bytes: 2_097_152,
            blocks_done: 1,
            blocks_total: 2,
            elapsed_secs: 1.0,
        };
        assert!((info.throughput_mbs() - 1.0).abs() < 0.01);
        assert!((info.eta_seconds() - 1.0).abs() < 0.01);
        assert!((info.fraction() - 0.5).abs() < 0.01);
    }

    #[test]
    fn adaptive_block_config_entropy() {
        let cfg = AdaptiveBlockConfig::default();
        // Low entropy → near min_block
        let low = cfg.select_block_size(0.5, 10_000_000);
        // High entropy → near max_block
        let high = cfg.select_block_size(7.5, 10_000_000);
        assert!(low < high);
        assert!(low >= cfg.min_block);
        assert!(high <= cfg.max_block);
    }
}
