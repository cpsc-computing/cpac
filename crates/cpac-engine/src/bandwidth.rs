// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Bandwidth-adaptive compression.
//!
//! Dynamically tunes per-block compression level to maintain a target output
//! throughput (bytes/sec) — useful when writing to a network pipe or a
//! bandwidth-limited storage target.
//!
//! The controller uses a simple PID-inspired feedback loop:
//! - Measure actual throughput after each block.
//! - If throughput < target, **lower** compression level (faster, bigger output).
//! - If throughput > target, **raise** level (slower, smaller output).
//! - Clamp to `[min_level, max_level]`.

use cpac_types::{CompressConfig, CompressionLevel, CpacResult};

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

/// Bandwidth-adaptive compression configuration.
#[derive(Clone, Debug)]
pub struct BandwidthConfig {
    /// Target output throughput in bytes/sec.
    pub target_bps: u64,
    /// Minimum compression level.
    pub min_level: i32,
    /// Maximum compression level.
    pub max_level: i32,
    /// Block size for per-block adaptive compression.
    pub block_size: usize,
    /// Gain factor for the proportional controller (0.0–2.0 typical).
    pub gain: f64,
}

impl Default for BandwidthConfig {
    fn default() -> Self {
        Self {
            target_bps: 100 * 1024 * 1024, // 100 MB/s default
            min_level: 1,
            max_level: 19,
            block_size: 1 << 20, // 1 MB
            gain: 1.0,
        }
    }
}

// ---------------------------------------------------------------------------
// Controller state
// ---------------------------------------------------------------------------

/// Bandwidth-adaptive controller.
pub struct BandwidthController {
    config: BandwidthConfig,
    current_level: i32,
    blocks_processed: usize,
    total_input: usize,
    total_output: usize,
    total_elapsed_secs: f64,
}

impl BandwidthController {
    /// Create a new controller.
    pub fn new(config: BandwidthConfig) -> Self {
        let initial = (config.min_level + config.max_level) / 2;
        Self {
            config,
            current_level: initial,
            blocks_processed: 0,
            total_input: 0,
            total_output: 0,
            total_elapsed_secs: 0.0,
        }
    }

    /// Current compression level being used.
    pub fn current_level(&self) -> i32 {
        self.current_level
    }

    /// Adjust level after observing a block's compression time.
    fn adjust(&mut self, output_bytes: usize, elapsed_secs: f64) {
        if elapsed_secs <= 0.0 {
            return;
        }
        let actual_bps = output_bytes as f64 / elapsed_secs;
        let target = self.config.target_bps as f64;

        // Error: positive → we are faster than needed → can raise level
        //        negative → we are slower → must lower level
        let error = (actual_bps - target) / target;

        // Proportional adjustment (clamped to ±3 levels per block)
        let delta = (error * self.config.gain).clamp(-3.0, 3.0) as i32;
        self.current_level =
            (self.current_level + delta).clamp(self.config.min_level, self.config.max_level);
    }

    /// Summary statistics.
    pub fn stats(&self) -> BandwidthStats {
        let avg_bps = if self.total_elapsed_secs > 0.0 {
            self.total_output as f64 / self.total_elapsed_secs
        } else {
            0.0
        };
        BandwidthStats {
            blocks: self.blocks_processed,
            total_input: self.total_input,
            total_output: self.total_output,
            avg_throughput_bps: avg_bps as u64,
            final_level: self.current_level,
        }
    }
}

/// Summary of bandwidth-adaptive compression.
#[derive(Debug, Clone)]
pub struct BandwidthStats {
    pub blocks: usize,
    pub total_input: usize,
    pub total_output: usize,
    pub avg_throughput_bps: u64,
    pub final_level: i32,
}

// ---------------------------------------------------------------------------
// Public API
// ---------------------------------------------------------------------------

/// Compress `data` with bandwidth-adaptive level tuning.
///
/// Returns `(compressed_output, stats)`.
pub fn compress_bandwidth_adaptive(
    data: &[u8],
    base_config: &CompressConfig,
    bw_config: &BandwidthConfig,
) -> CpacResult<(Vec<u8>, BandwidthStats)> {
    let mut ctrl = BandwidthController::new(bw_config.clone());
    let block_sz = bw_config.block_size.max(4096);
    let blocks: Vec<&[u8]> = data.chunks(block_sz).collect();
    let num_blocks = blocks.len();

    // Streaming frame header: same as CPBL but with "BA" magic
    const BA_MAGIC: &[u8; 2] = b"BA";
    const BA_VERSION: u8 = 1;

    let mut compressed_blocks: Vec<Vec<u8>> = Vec::with_capacity(num_blocks);

    for block in &blocks {
        let mut cfg = base_config.clone();
        cfg.level = level_from_i32(ctrl.current_level());
        cfg.disable_parallel = true; // per-block, no recursion

        let t0 = std::time::Instant::now();
        let result = crate::compress(block, &cfg)?;
        let elapsed = t0.elapsed().as_secs_f64();

        ctrl.total_input += block.len();
        ctrl.total_output += result.data.len();
        ctrl.total_elapsed_secs += elapsed;
        ctrl.blocks_processed += 1;

        ctrl.adjust(result.data.len(), elapsed);
        compressed_blocks.push(result.data);
    }

    // Build frame: BA(2) + ver(1) + num_blocks(4) + orig_size(8) + block_size(4)
    //              + [per block: compressed_len(4) + data]
    let header_size = 2 + 1 + 4 + 8 + 4;
    let payload_size: usize = compressed_blocks.iter().map(|b| 4 + b.len()).sum();
    let mut frame = Vec::with_capacity(header_size + payload_size);

    frame.extend_from_slice(BA_MAGIC);
    frame.push(BA_VERSION);
    frame.extend_from_slice(&(num_blocks as u32).to_le_bytes());
    frame.extend_from_slice(&(data.len() as u64).to_le_bytes());
    frame.extend_from_slice(&(block_sz as u32).to_le_bytes());

    for block in &compressed_blocks {
        frame.extend_from_slice(&(block.len() as u32).to_le_bytes());
        frame.extend_from_slice(block);
    }

    Ok((frame, ctrl.stats()))
}

/// Decompress a bandwidth-adaptive frame (BA magic).
pub fn decompress_bandwidth_adaptive(data: &[u8]) -> CpacResult<Vec<u8>> {
    if data.len() < 19 || &data[0..2] != b"BA" || data[2] != 1 {
        return Err(cpac_types::CpacError::InvalidFrame(
            "not a bandwidth-adaptive frame".into(),
        ));
    }

    let num_blocks = u32::from_le_bytes([data[3], data[4], data[5], data[6]]) as usize;
    let original_size = u64::from_le_bytes([
        data[7], data[8], data[9], data[10], data[11], data[12], data[13], data[14],
    ]) as usize;
    let _block_size = u32::from_le_bytes([data[15], data[16], data[17], data[18]]) as usize;

    let mut offset = 19;
    let mut result = Vec::with_capacity(original_size);

    for _ in 0..num_blocks {
        if offset + 4 > data.len() {
            return Err(cpac_types::CpacError::InvalidFrame(
                "truncated block header".into(),
            ));
        }
        let clen = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;
        if offset + clen > data.len() {
            return Err(cpac_types::CpacError::InvalidFrame(
                "truncated block data".into(),
            ));
        }
        let decompressed = crate::decompress(&data[offset..offset + clen])?;
        result.extend_from_slice(&decompressed.data);
        offset += clen;
    }

    if result.len() != original_size {
        return Err(cpac_types::CpacError::DecompressFailed(format!(
            "size mismatch: expected {original_size}, got {}",
            result.len()
        )));
    }

    Ok(result)
}

/// Map integer level to CompressionLevel enum.
fn level_from_i32(level: i32) -> CompressionLevel {
    match level {
        0..=1 => CompressionLevel::UltraFast,
        2..=3 => CompressionLevel::Fast,
        4..=9 => CompressionLevel::Default,
        10..=15 => CompressionLevel::High,
        _ => CompressionLevel::Best,
    }
}

/// Check if data is a bandwidth-adaptive frame.
#[must_use]
pub fn is_bandwidth_frame(data: &[u8]) -> bool {
    data.len() >= 19 && &data[0..2] == b"BA" && data[2] == 1
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bandwidth_roundtrip() {
        let data: Vec<u8> = b"Hello, bandwidth-adaptive compression! ".repeat(1000);
        let cc = CompressConfig::default();
        let bw = BandwidthConfig {
            target_bps: 50 * 1024 * 1024, // 50 MB/s
            min_level: 1,
            max_level: 9,
            block_size: 4096,
            gain: 1.0,
        };
        let (frame, stats) = compress_bandwidth_adaptive(&data, &cc, &bw).unwrap();
        assert!(is_bandwidth_frame(&frame));
        assert!(stats.blocks > 0);
        let restored = decompress_bandwidth_adaptive(&frame).unwrap();
        assert_eq!(restored, data);
    }

    #[test]
    fn bandwidth_controller_level_stays_in_bounds() {
        let bw = BandwidthConfig {
            min_level: 1,
            max_level: 5,
            ..Default::default()
        };
        let mut ctrl = BandwidthController::new(bw);
        // Simulate very fast block → should raise level
        ctrl.adjust(10_000_000, 0.001);
        assert!(ctrl.current_level() <= 5);
        // Simulate very slow block → should lower level
        ctrl.adjust(100, 10.0);
        assert!(ctrl.current_level() >= 1);
    }

    #[test]
    fn not_bandwidth_frame() {
        assert!(!is_bandwidth_frame(b"XX"));
        assert!(!is_bandwidth_frame(b"CS")); // streaming
    }
}
