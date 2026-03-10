// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Content-defined chunking (CDC) and dedup-aware compression.
//!
//! Implements a Gear-hash based CDC splitter, a chunk-dedup index backed by
//! BLAKE3 fingerprints, and a wire format (`CPDD`) that stores unique chunks
//! once and references duplicates by index.
//!
//! # Wire Format — CPDD v1
//!
//! ```text
//! Magic("DD", 2B) | Version(1B) | Flags(2B LE) |
//! NumUniqueChunks(4B LE) | NumTotalChunks(4B LE) | OriginalSize(8B LE) |
//! [for each unique chunk: CompressedLen(4B LE) + Data] |
//! [for each total chunk: UniqueIndex(4B LE)]
//! ```
//!
//! The index map at the end lets the decoder reconstruct the original byte
//! stream by emitting decompressed unique chunks in order.

use cpac_types::{CompressConfig, CpacError, CpacResult};
use std::collections::HashMap;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// CPDD magic bytes.
const DEDUP_MAGIC: &[u8; 2] = b"DD";
/// CPDD wire version.
const DEDUP_VERSION: u8 = 1;

/// Default CDC parameters.
const DEFAULT_MIN_CHUNK: usize = 4 * 1024;       // 4 KB
const DEFAULT_AVG_CHUNK: usize = 64 * 1024;      // 64 KB
const DEFAULT_MAX_CHUNK: usize = 1024 * 1024;    // 1 MB

// Gear-hash parameters
const _GEAR_MASK: u64 = (1u64 << 16) - 1; // ~64 KB average (reserved)

// ---------------------------------------------------------------------------
// Gear hash table (fixed random lookup)
// ---------------------------------------------------------------------------

/// Pre-computed random byte→u64 table for Gear hash.
/// Generated deterministically from BLAKE3("cpac-gear-table").
fn gear_table() -> &'static [u64; 256] {
    use std::sync::OnceLock;
    static TABLE: OnceLock<[u64; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let seed = blake3::hash(b"cpac-gear-table");
        let seed_bytes = seed.as_bytes();
        let mut table = [0u64; 256];
        // Expand seed into 256 u64s via repeated BLAKE3 keyed derivation
        for i in 0..32 {
            let block_seed = blake3::hash(&[seed_bytes.as_slice(), &[i as u8]].concat());
            let b = block_seed.as_bytes();
            for j in 0..4 {
                let idx = i * 8 + j * 2;
                if idx < 256 {
                    table[idx] = u64::from_le_bytes([
                        b[j * 8],
                        b[j * 8 + 1],
                        b[j * 8 + 2],
                        b[j * 8 + 3],
                        b[j * 8 + 4],
                        b[j * 8 + 5],
                        b[j * 8 + 6],
                        b[j * 8 + 7],
                    ]);
                }
                if idx + 1 < 256 {
                    // Second half of the block
                    table[idx + 1] = table[idx].wrapping_mul(0x517cc1b727220a95).wrapping_add(i as u64);
                }
            }
        }
        table
    })
}

// ---------------------------------------------------------------------------
// CDC chunker
// ---------------------------------------------------------------------------

/// Configuration for CDC-based dedup compression.
#[derive(Clone, Debug)]
pub struct DedupConfig {
    /// Minimum chunk size (bytes).
    pub min_chunk: usize,
    /// Target average chunk size (bytes).  Gear mask is derived from this.
    pub avg_chunk: usize,
    /// Maximum chunk size (bytes).
    pub max_chunk: usize,
}

impl Default for DedupConfig {
    fn default() -> Self {
        Self {
            min_chunk: DEFAULT_MIN_CHUNK,
            avg_chunk: DEFAULT_AVG_CHUNK,
            max_chunk: DEFAULT_MAX_CHUNK,
        }
    }
}

/// Split `data` into content-defined chunks using Gear hashing.
///
/// Returns a `Vec` of `(offset, length)` pairs.
pub fn cdc_split(data: &[u8], cfg: &DedupConfig) -> Vec<(usize, usize)> {
    if data.is_empty() {
        return Vec::new();
    }

    let gt = gear_table();
    // Compute mask from avg_chunk: round down to power of two
    let bits = (cfg.avg_chunk as f64).log2().floor() as u32;
    let mask: u64 = (1u64 << bits) - 1;

    let mut chunks = Vec::new();
    let mut start = 0;

    while start < data.len() {
        let end = data.len().min(start + cfg.max_chunk);
        let min_end = data.len().min(start + cfg.min_chunk);

        // Skip min_chunk bytes before looking for boundary
        let mut hash: u64 = 0;
        let mut pos = min_end;
        while pos < end {
            hash = hash.wrapping_shl(1).wrapping_add(gt[data[pos] as usize]);
            if hash & mask == 0 {
                pos += 1; // include the boundary byte
                break;
            }
            pos += 1;
        }
        let chunk_end = pos.min(end);
        chunks.push((start, chunk_end - start));
        start = chunk_end;
    }

    chunks
}

// ---------------------------------------------------------------------------
// Fingerprinting + dedup index
// ---------------------------------------------------------------------------

/// BLAKE3 fingerprint of a chunk (32 bytes, truncated display).
pub type ChunkFingerprint = [u8; 32];

/// Compute BLAKE3 fingerprint of a data slice.
#[inline]
pub fn fingerprint(data: &[u8]) -> ChunkFingerprint {
    *blake3::hash(data).as_bytes()
}

/// Dedup index: maps fingerprint → unique-chunk-id.
#[derive(Default)]
pub struct DedupIndex {
    map: HashMap<ChunkFingerprint, u32>,
    unique_chunks: Vec<Vec<u8>>,
}

impl DedupIndex {
    /// Insert a chunk.  Returns `(unique_id, is_new)`.
    pub fn insert(&mut self, data: &[u8]) -> (u32, bool) {
        let fp = fingerprint(data);
        if let Some(&id) = self.map.get(&fp) {
            (id, false)
        } else {
            let id = self.unique_chunks.len() as u32;
            self.map.insert(fp, id);
            self.unique_chunks.push(data.to_vec());
            (id, true)
        }
    }

    /// Number of unique chunks stored.
    pub fn unique_count(&self) -> usize {
        self.unique_chunks.len()
    }

    /// Access unique chunk data by id.
    pub fn get(&self, id: u32) -> Option<&[u8]> {
        self.unique_chunks.get(id as usize).map(|v| v.as_slice())
    }
}

// ---------------------------------------------------------------------------
// Dedup-aware compress
// ---------------------------------------------------------------------------

/// Result of dedup compression.
#[derive(Debug)]
pub struct DedupResult {
    /// CPDD wire-format frame.
    pub data: Vec<u8>,
    /// Original input size.
    pub original_size: usize,
    /// Compressed size (frame length).
    pub compressed_size: usize,
    /// Number of CDC chunks (total).
    pub total_chunks: usize,
    /// Number of unique chunks stored.
    pub unique_chunks: usize,
    /// Dedup savings ratio (0.0 = no dedup, 1.0 = all duplicate).
    pub dedup_ratio: f64,
}

/// Compress with CDC dedup.
///
/// 1. Splits input via Gear-hash CDC.
/// 2. Fingerprints each chunk with BLAKE3.
/// 3. Compresses only unique chunks.
/// 4. Emits CPDD wire frame.
pub fn compress_dedup(
    data: &[u8],
    compress_config: &CompressConfig,
    dedup_config: &DedupConfig,
) -> CpacResult<DedupResult> {
    let chunks = cdc_split(data, dedup_config);
    let total_chunks = chunks.len();

    let mut index = DedupIndex::default();
    let mut chunk_ids: Vec<u32> = Vec::with_capacity(total_chunks);

    for &(offset, len) in &chunks {
        let chunk_data = &data[offset..offset + len];
        let (id, _is_new) = index.insert(chunk_data);
        chunk_ids.push(id);
    }

    let unique_count = index.unique_count();
    let dedup_ratio = if total_chunks > 0 {
        1.0 - (unique_count as f64 / total_chunks as f64)
    } else {
        0.0
    };

    // Compress each unique chunk
    let mut compressed_chunks: Vec<Vec<u8>> = Vec::with_capacity(unique_count);
    for i in 0..unique_count {
        let chunk = index.get(i as u32).unwrap();
        let result = crate::compress(chunk, compress_config)?;
        compressed_chunks.push(result.data);
    }

    // Build CPDD frame
    let payload_size: usize = compressed_chunks.iter().map(|c| 4 + c.len()).sum();
    let index_size = total_chunks * 4;
    let header_size = 2 + 1 + 2 + 4 + 4 + 8; // magic + ver + flags + n_unique + n_total + orig_size
    let total_size = header_size + payload_size + index_size;

    let mut frame = Vec::with_capacity(total_size);

    // Header
    frame.extend_from_slice(DEDUP_MAGIC);
    frame.push(DEDUP_VERSION);
    frame.extend_from_slice(&0u16.to_le_bytes()); // flags (reserved)
    frame.extend_from_slice(&(unique_count as u32).to_le_bytes());
    frame.extend_from_slice(&(total_chunks as u32).to_le_bytes());
    frame.extend_from_slice(&(data.len() as u64).to_le_bytes());

    // Unique compressed chunks
    for chunk in &compressed_chunks {
        frame.extend_from_slice(&(chunk.len() as u32).to_le_bytes());
        frame.extend_from_slice(chunk);
    }

    // Chunk index (reconstruction map)
    for &id in &chunk_ids {
        frame.extend_from_slice(&id.to_le_bytes());
    }

    let compressed_size = frame.len();
    Ok(DedupResult {
        data: frame,
        original_size: data.len(),
        compressed_size,
        total_chunks,
        unique_chunks: unique_count,
        dedup_ratio,
    })
}

// ---------------------------------------------------------------------------
// Dedup-aware decompress
// ---------------------------------------------------------------------------

/// Decompress a CPDD frame.
pub fn decompress_dedup(data: &[u8]) -> CpacResult<Vec<u8>> {
    // Minimum header: 2 + 1 + 2 + 4 + 4 + 8 = 21
    if data.len() < 21 || &data[0..2] != DEDUP_MAGIC {
        return Err(CpacError::InvalidFrame("not a CPDD frame".into()));
    }
    if data[2] != DEDUP_VERSION {
        return Err(CpacError::InvalidFrame("unsupported CPDD version".into()));
    }

    let _flags = u16::from_le_bytes([data[3], data[4]]);
    let num_unique = u32::from_le_bytes([data[5], data[6], data[7], data[8]]) as usize;
    let num_total = u32::from_le_bytes([data[9], data[10], data[11], data[12]]) as usize;
    let original_size = u64::from_le_bytes([
        data[13], data[14], data[15], data[16], data[17], data[18], data[19], data[20],
    ]) as usize;

    // Parse unique compressed chunks
    let mut offset = 21;
    let mut unique_decompressed: Vec<Vec<u8>> = Vec::with_capacity(num_unique);
    for _ in 0..num_unique {
        if offset + 4 > data.len() {
            return Err(CpacError::InvalidFrame("truncated chunk header".into()));
        }
        let clen = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]) as usize;
        offset += 4;
        if offset + clen > data.len() {
            return Err(CpacError::InvalidFrame("truncated chunk data".into()));
        }
        let decompressed = crate::decompress(&data[offset..offset + clen])?;
        unique_decompressed.push(decompressed.data);
        offset += clen;
    }

    // Parse index
    let index_size = num_total * 4;
    if offset + index_size > data.len() {
        return Err(CpacError::InvalidFrame("truncated chunk index".into()));
    }

    let mut result = Vec::with_capacity(original_size);
    for i in 0..num_total {
        let idx_offset = offset + i * 4;
        let chunk_id = u32::from_le_bytes([
            data[idx_offset],
            data[idx_offset + 1],
            data[idx_offset + 2],
            data[idx_offset + 3],
        ]) as usize;
        if chunk_id >= unique_decompressed.len() {
            return Err(CpacError::InvalidFrame(format!(
                "chunk index {chunk_id} out of range (have {})",
                unique_decompressed.len()
            )));
        }
        result.extend_from_slice(&unique_decompressed[chunk_id]);
    }

    if result.len() != original_size {
        return Err(CpacError::DecompressFailed(format!(
            "dedup size mismatch: expected {original_size}, got {}",
            result.len()
        )));
    }

    Ok(result)
}

/// Check if data is a CPDD dedup frame.
#[must_use]
pub fn is_dedup_frame(data: &[u8]) -> bool {
    data.len() >= 21 && &data[0..2] == DEDUP_MAGIC && data[2] == DEDUP_VERSION
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cdc_split_basic() {
        let data = vec![0u8; 256 * 1024]; // 256 KB of zeros
        let cfg = DedupConfig {
            min_chunk: 1024,
            avg_chunk: 4096,
            max_chunk: 16384,
        };
        let chunks = cdc_split(&data, &cfg);
        assert!(!chunks.is_empty());
        // All chunks cover the full input
        let total: usize = chunks.iter().map(|&(_, len)| len).sum();
        assert_eq!(total, data.len());
        // No overlaps
        for window in chunks.windows(2) {
            assert_eq!(window[0].0 + window[0].1, window[1].0);
        }
    }

    #[test]
    fn cdc_split_empty() {
        assert!(cdc_split(&[], &DedupConfig::default()).is_empty());
    }

    #[test]
    fn fingerprint_deterministic() {
        let data = b"hello world";
        assert_eq!(fingerprint(data), fingerprint(data));
    }

    #[test]
    fn dedup_index_basic() {
        let mut idx = DedupIndex::default();
        let (id1, new1) = idx.insert(b"chunk_a");
        assert!(new1);
        let (id2, new2) = idx.insert(b"chunk_b");
        assert!(new2);
        assert_ne!(id1, id2);
        let (id3, new3) = idx.insert(b"chunk_a"); // duplicate
        assert!(!new3);
        assert_eq!(id1, id3);
        assert_eq!(idx.unique_count(), 2);
    }

    #[test]
    fn dedup_roundtrip_no_duplicates() {
        let data: Vec<u8> = (0u8..=255).cycle().take(32768).collect();
        let cc = CompressConfig::default();
        let dc = DedupConfig {
            min_chunk: 1024,
            avg_chunk: 4096,
            max_chunk: 8192,
        };
        let result = compress_dedup(&data, &cc, &dc).unwrap();
        assert!(is_dedup_frame(&result.data));
        let restored = decompress_dedup(&result.data).unwrap();
        assert_eq!(restored, data);
    }

    #[test]
    fn dedup_roundtrip_with_duplicates() {
        // Repeat a 4 KB block 16 times → heavy dedup
        let block: Vec<u8> = (0u8..=255).cycle().take(4096).collect();
        let data: Vec<u8> = block.repeat(16);
        let cc = CompressConfig::default();
        let dc = DedupConfig {
            min_chunk: 2048,
            avg_chunk: 4096,
            max_chunk: 8192,
        };
        let result = compress_dedup(&data, &cc, &dc).unwrap();
        assert!(result.unique_chunks <= result.total_chunks);
        // Should see significant dedup
        let restored = decompress_dedup(&result.data).unwrap();
        assert_eq!(restored, data);
    }

    #[test]
    fn not_dedup_frame() {
        assert!(!is_dedup_frame(b"XX"));
        assert!(!is_dedup_frame(b"CS")); // streaming frame magic
    }
}
