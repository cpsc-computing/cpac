// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Block-parallel compression and decompression using rayon.
//!
//! Wire format (CPBL):
//!
//! ## Version 1
//! ```text
//! "CPBL" (4B) | version=1 (1B) | block_count (4B LE) | original_size (8B LE)
//! | [compressed_block_size: u32 LE] × block_count
//! | [block_data] × block_count
//! ```
//!
//! ## Version 2 (Phase 2: shared MSN metadata)
//! ```text
//! "CPBL" (4B) | version=2 (1B) | block_count (4B LE) | original_size (8B LE)
//! | shared_meta_len (4B LE)
//! | [compressed_block_size: u32 LE] × block_count
//! | [block_flags: u8] × block_count
//! | shared_metadata (shared_meta_len bytes)
//! | [block_data] × block_count
//! ```
//!
//! Block flags (bit field):
//! - bit 0: `msn_applied` — block data is MSN residual; reconstruct with shared metadata.
//!
//! Each block is independently compressed using the normal CPAC pipeline,
//! allowing full parallel decompression.

use cpac_types::{CompressConfig, CompressResult, CpacError, CpacResult, DecompressResult};
use rayon::prelude::*;

/// CPBL magic bytes (block-parallel format).
pub const CPBL_MAGIC: &[u8; 4] = b"CPBL";

/// CPBL format version 1 (no shared metadata).
pub const CPBL_VERSION_V1: u8 = 1;

/// CPBL format version 2 (Phase 2: shared MSN metadata).
pub const CPBL_VERSION_V2: u8 = 2;

/// CPBL format version 3 (Phase 3: auto-dictionary + shared MSN metadata).
pub const CPBL_VERSION_V3: u8 = 3;

/// Current CPBL format version.
pub const CPBL_VERSION: u8 = CPBL_VERSION_V3;

/// Block flag: MSN was applied; decompressed data is residual.
const BLOCK_FLAG_MSN: u8 = 0x01;

/// Maximum number of blocks to sample for dictionary training.
const DICT_TRAIN_MAX_BLOCKS: usize = 8;

/// Minimum number of blocks required to train a dictionary (need enough variety).
const DICT_TRAIN_MIN_BLOCKS: usize = 3;

/// Maximum dictionary size for parallel auto-dict (64 KB).
const DICT_MAX_SIZE: usize = 64 * 1024;

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
/// parallelism.  Files below 32 MiB that are mostly ASCII stay on the
/// single-stream path for better ratios and lower preprocessing overhead.
/// Raised from 16 MiB to 32 MiB so MSN extraction sees full-file context
/// for structured data (JSON, XML, YAML) where repeated keys/tags span
/// the entire file.  Block-level MSN extraction loses this cross-block
/// context and typically fails to produce a smaller residual.
pub const PARALLEL_THRESHOLD_TEXT: usize = 32 * 1024 * 1024;

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

/// CPBL v1 header: magic(4) + version(1) + `block_count(4)` + `original_size(8)` = 17 bytes.
const CPBL_HEADER_SIZE_V1: usize = 4 + 1 + 4 + 8;

/// CPBL v2 header: v1 header + `shared_meta_len(4)` = 21 bytes.
const CPBL_HEADER_SIZE_V2: usize = CPBL_HEADER_SIZE_V1 + 4;

/// CPBL v3 header: v2 header + `dict_len(4)` = 25 bytes.
const CPBL_HEADER_SIZE_V3: usize = CPBL_HEADER_SIZE_V2 + 4;

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
    cpac_trace!(
        "[TRACE] compress_parallel: size={}B block_size={}B blocks={} threads={}",
        original_size,
        bs,
        block_count,
        num_threads
    );

    // Use shared global thread pool (Phase 4B) instead of creating a new one per call.
    let pool = crate::pool::get_or_init_thread_pool(num_threads);

    // Compress blocks in parallel with disable_parallel flag to prevent recursion
    let mut block_config = config.clone();
    block_config.disable_parallel = true;

    // P1: PHASE 1 FIX — allow BWT on parallel sub-blocks.
    // Previously set skip_expensive_transforms = true, which prevented
    // all ratio-improving transforms from running on parallel blocks.
    // BWT is O(n) via SA-IS and roundtrips correctly at block sizes.
    // block_config.skip_expensive_transforms = true;  // REMOVED

    // Phase 4A: MSN field-map caching — probe the first block to discover
    // the domain and build the field map, then reuse it across all blocks.
    // This avoids O(N_blocks × N_domains) detection overhead for large
    // homogeneous files (e.g. 100 MB YAML split into 25 × 4 MB blocks).
    //
    // Phase 2: When MSN metadata is discovered, enable external metadata
    // mode so sub-blocks don't embed metadata in their own frames.
    // The metadata will be stored once in the CPBL v2 header.
    let mut shared_msn_metadata: Vec<u8> = Vec::new();
    if config.enable_msn && block_config.cached_msn_metadata.is_none() && !blocks.is_empty() {
        let probe_filename = config.filename.as_deref();
        // Phase 2: cap probe sample to MAX_DOMAIN_EXTRACT_SIZE.  Adaptive
        // block sizing can produce blocks larger than the domain extraction
        // limit; truncating the probe is safe because we only need enough
        // data to detect the domain and build the field map.
        let probe_len = blocks[0].len().min(cpac_msn::MAX_DOMAIN_EXTRACT_SIZE);
        if let Ok(probe_result) = cpac_msn::extract(
            &blocks[0][..probe_len],
            probe_filename,
            config.msn_confidence,
        ) {
            cpac_trace!(
                "[TRACE] parallel MSN probe: applied={} domain={:?} conf={:.3} fields={}",
                probe_result.applied,
                probe_result.domain_id,
                probe_result.confidence,
                probe_result.fields.len()
            );
            if probe_result.applied {
                if let Ok(encoded) = cpac_msn::encode_metadata_compact(&probe_result.metadata()) {
                    cpac_trace!(
                        "[TRACE] parallel MSN probe: cached metadata={}B (Phase 2: external)",
                        encoded.len()
                    );
                    block_config.cached_msn_metadata = Some(encoded.clone());
                    // Phase 2: store metadata for CPBL header and enable external mode.
                    shared_msn_metadata = encoded;
                    block_config.msn_metadata_external = true;
                }

                // Phase 6: CAS bridge — run constraint analysis on MSN-extracted fields.
                // This identifies Fixed, Derived, Stride, Range constraints that inform
                // transform selection (logged for now; future phases use for projection).
                let typed_cols = probe_result.typed_columns();
                if !typed_cols.is_empty() {
                    let analysis = cpac_cas::analyze_columns(&typed_cols.int_columns);
                    cpac_trace!(
                        "[TRACE] Phase 6: CAS bridge → {} int cols, {} constraints, benefit={:.3}",
                        typed_cols.int_columns.len(),
                        analysis
                            .constraints
                            .iter()
                            .map(|(_, cs)| cs.len())
                            .sum::<usize>(),
                        analysis.estimated_benefit
                    );
                    for (col_name, constraints) in &analysis.constraints {
                        for c in constraints {
                            cpac_trace!("[TRACE] Phase 6: CAS col '{}': {:?}", col_name, c);
                        }
                    }
                    // Also analyze string columns for enumeration/length constraints
                    for (name, values) in &typed_cols.string_columns {
                        let sc = cpac_cas::infer_string_constraints(name, values);
                        if !sc.is_empty() {
                            cpac_trace!(
                                "[TRACE] Phase 6: CAS string col '{}': {} constraints",
                                name,
                                sc.len()
                            );
                        }
                    }
                    // Float column constraints
                    for (name, values) in &typed_cols.float_columns {
                        let fc = cpac_cas::infer_float_constraints(values);
                        if !fc.is_empty() {
                            cpac_trace!(
                                "[TRACE] Phase 6: CAS float col '{}': {} constraints",
                                name,
                                fc.len()
                            );
                        }
                    }
                }
            }
        } else {
            cpac_trace!("[TRACE] parallel MSN probe: extract failed");
        }
    }

    // Phase 3: Auto-dictionary training from first N blocks (Zstd only).
    // Dictionary compression dramatically improves ratio on small similar blocks.
    let mut trained_dict: Vec<u8> = Vec::new();
    let effective_backend = config.backend.unwrap_or(cpac_types::Backend::Zstd);
    if effective_backend == cpac_types::Backend::Zstd
        && block_count >= DICT_TRAIN_MIN_BLOCKS
        && config.dictionary.is_none()
    {
        let sample_count = block_count.min(DICT_TRAIN_MAX_BLOCKS);
        let samples: Vec<Vec<u8>> = blocks[..sample_count].iter().map(|b| b.to_vec()).collect();
        match cpac_dict::CpacDictionary::train(&samples, DICT_MAX_SIZE) {
            Ok(dict) => {
                cpac_trace!(
                    "[TRACE] Phase 3: trained dict from {} blocks, size={}B",
                    sample_count,
                    dict.data.len()
                );
                block_config.dictionary = Some(dict.data.clone());
                trained_dict = dict.data;
            }
            Err(_e) => {
                cpac_trace!("[TRACE] Phase 3: dict training failed, continuing without");
            }
        }
    }

    // P2+P9+P10: Cache transform recommendations AND SSR from the first block
    // and reuse across all blocks.  For homogeneous files (JSON, logs), every
    // block has the same structure, so re-running analyze_structure/SSR is waste.
    if block_config.cached_transform_recs.is_none() && !blocks.is_empty() {
        let probe_ssr = cpac_ssr::analyze(blocks[0]);

        // P9: cache SSR so per-block compress() calls skip cpac_ssr::analyze().
        block_config.cached_ssr = Some(cpac_types::CachedSsr::from(&probe_ssr));

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
        cpac_trace!(
            "[TRACE] parallel probe SSR: entropy={:.3} ascii={:.3} track={:?}",
            probe_ssr.entropy_estimate,
            probe_ssr.ascii_ratio,
            probe_ssr.track
        );
        cpac_trace!(
            "[TRACE] parallel probe transforms: {:?} (all: {:?})",
            rec_names,
            probe_profile
                .recommended_chain
                .iter()
                .map(|r| format!("{}:{:.2}", r.name, r.confidence))
                .collect::<Vec<_>>()
        );
        if !rec_names.is_empty() {
            block_config.cached_transform_recs = Some(rec_names);
        } else {
            // P10: no transforms recommended — disable smart_preprocess for
            // all blocks to avoid the per-block trial overhead entirely.
            cpac_trace!("[TRACE] parallel: P10 disabling smart_transforms (no recs)");
            block_config.enable_smart_transforms = false;
        }
    }

    // Phase 2: collect both compressed data and msn_applied flag per block.
    let compressed_blocks: Vec<CpacResult<(Vec<u8>, bool)>> = pool.install(|| {
        blocks
            .par_iter()
            .map(|block| {
                let result = crate::compress(block, &block_config)?;
                Ok((result.data, result.msn_applied))
            })
            .collect()
    });

    // Collect results, propagating any errors
    let mut block_data: Vec<Vec<u8>> = Vec::with_capacity(block_count);
    let mut block_flags: Vec<u8> = Vec::with_capacity(block_count);
    for result in compressed_blocks {
        let (data, msn_applied) = result?;
        block_data.push(data);
        block_flags.push(if msn_applied { BLOCK_FLAG_MSN } else { 0 });
    }

    let has_shared_meta = !shared_msn_metadata.is_empty();
    let has_dict = !trained_dict.is_empty();
    let use_v3 = has_dict;
    let use_v2 = has_shared_meta && !use_v3;

    // Encode CPBL wire format (v3 with dict, v2 with metadata, v1 otherwise)
    let block_sizes: Vec<u32> = block_data.iter().map(|b| b.len() as u32).collect();
    let payload_size: usize = block_data.iter().map(std::vec::Vec::len).sum();
    let total = if use_v3 {
        CPBL_HEADER_SIZE_V3
            + block_count * 4
            + block_count
            + shared_msn_metadata.len()
            + trained_dict.len()
            + payload_size
    } else if use_v2 {
        CPBL_HEADER_SIZE_V2
            + block_count * 4
            + block_count
            + shared_msn_metadata.len()
            + payload_size
    } else {
        CPBL_HEADER_SIZE_V1 + block_count * 4 + payload_size
    };
    let mut out = Vec::with_capacity(total);

    // Header
    out.extend_from_slice(CPBL_MAGIC);
    if use_v3 {
        out.push(CPBL_VERSION_V3);
    } else if use_v2 {
        out.push(CPBL_VERSION_V2);
    } else {
        out.push(CPBL_VERSION_V1);
    }
    out.extend_from_slice(&(block_count as u32).to_le_bytes());
    out.extend_from_slice(&(original_size as u64).to_le_bytes());

    if use_v3 || use_v2 {
        // V2/V3: shared_meta_len
        out.extend_from_slice(&(shared_msn_metadata.len() as u32).to_le_bytes());
    }
    if use_v3 {
        // V3: dict_len
        out.extend_from_slice(&(trained_dict.len() as u32).to_le_bytes());
    }

    // Block size table
    for &sz in &block_sizes {
        out.extend_from_slice(&sz.to_le_bytes());
    }

    if use_v3 || use_v2 {
        // V2/V3: block flags + shared metadata
        out.extend_from_slice(&block_flags);
        out.extend_from_slice(&shared_msn_metadata);
    }
    if use_v3 {
        // V3: dictionary data
        out.extend_from_slice(&trained_dict);
        cpac_trace!(
            "[TRACE] CPBL v3: shared_meta={}B dict={}B flags={:?}",
            shared_msn_metadata.len(),
            trained_dict.len(),
            block_flags
        );
    } else if use_v2 {
        cpac_trace!(
            "[TRACE] CPBL v2: shared_meta={}B flags={:?}",
            shared_msn_metadata.len(),
            block_flags
        );
    }

    // Block payloads
    for block in &block_data {
        out.extend_from_slice(block);
    }

    let compressed_size = out.len();

    // Phase 5: derive track from block-level SSR instead of hardcoding Track2.
    let actual_track = block_config
        .cached_ssr
        .as_ref()
        .map(|ssr| ssr.track)
        .unwrap_or(cpac_types::Track::Track2);

    Ok(CompressResult {
        data: out,
        original_size,
        compressed_size,
        track: actual_track,
        backend: config.backend.unwrap_or(cpac_types::Backend::Zstd),
        msn_applied: false,
    })
}

/// Decompress CPBL block-parallel data.
///
/// Reads the CPBL header (v1 or v2), locates block boundaries, decompresses
/// each block in parallel, applies MSN reconstruction for v2 shared metadata
/// blocks, and concatenates the results.
pub fn decompress_parallel(data: &[u8], num_threads: usize) -> CpacResult<DecompressResult> {
    if data.len() < CPBL_HEADER_SIZE_V1 {
        return Err(CpacError::DecompressFailed("CPBL data too short".into()));
    }

    // Parse header
    if &data[..4] != CPBL_MAGIC {
        return Err(CpacError::DecompressFailed("not a CPBL frame".into()));
    }

    let version = data[4];
    if version != CPBL_VERSION_V1 && version != CPBL_VERSION_V2 && version != CPBL_VERSION_V3 {
        return Err(CpacError::DecompressFailed(format!(
            "unsupported CPBL version: {version}"
        )));
    }

    let block_count = u32::from_le_bytes([data[5], data[6], data[7], data[8]]) as usize;
    let original_size = u64::from_le_bytes([
        data[9], data[10], data[11], data[12], data[13], data[14], data[15], data[16],
    ]) as usize;

    // V2/V3: read shared_meta_len and optional dict_len
    let (shared_meta_len, dict_len, header_end) = if version == CPBL_VERSION_V3 {
        if data.len() < CPBL_HEADER_SIZE_V3 {
            return Err(CpacError::DecompressFailed(
                "CPBL v3 header too short".into(),
            ));
        }
        let ml = u32::from_le_bytes([data[17], data[18], data[19], data[20]]) as usize;
        let dl = u32::from_le_bytes([data[21], data[22], data[23], data[24]]) as usize;
        (ml, dl, CPBL_HEADER_SIZE_V3)
    } else if version == CPBL_VERSION_V2 {
        if data.len() < CPBL_HEADER_SIZE_V2 {
            return Err(CpacError::DecompressFailed(
                "CPBL v2 header too short".into(),
            ));
        }
        let ml = u32::from_le_bytes([data[17], data[18], data[19], data[20]]) as usize;
        (ml, 0, CPBL_HEADER_SIZE_V2)
    } else {
        (0, 0, CPBL_HEADER_SIZE_V1)
    };

    // Read block size table
    let table_start = header_end;
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

    // V2/V3: read block flags + shared metadata + optional dict
    let (block_flags, shared_metadata, dict_data) = if version >= CPBL_VERSION_V2 {
        let flags_start = table_end;
        let flags_end = flags_start + block_count;
        if data.len() < flags_end {
            return Err(CpacError::DecompressFailed(
                "CPBL truncated block flags".into(),
            ));
        }
        let flags = data[flags_start..flags_end].to_vec();

        let meta_start = flags_end;
        let meta_end = meta_start + shared_meta_len;
        if data.len() < meta_end {
            return Err(CpacError::DecompressFailed(
                "CPBL truncated shared metadata".into(),
            ));
        }
        let meta = &data[meta_start..meta_end];

        // V3: dictionary follows shared metadata
        let dict_start = meta_end;
        let dict_end = dict_start + dict_len;
        if dict_len > 0 && data.len() < dict_end {
            return Err(CpacError::DecompressFailed(
                "CPBL v3 truncated dictionary".into(),
            ));
        }
        let dict = if dict_len > 0 {
            &data[dict_start..dict_end]
        } else {
            &data[0..0]
        };
        (flags, meta, dict)
    } else {
        (vec![0u8; block_count], &data[0..0], &data[0..0])
    };

    // Locate block boundaries (after flags + shared metadata + dict)
    let payload_start = if version >= CPBL_VERSION_V2 {
        table_end + block_count + shared_meta_len + dict_len
    } else {
        table_end
    };
    let mut blocks: Vec<&[u8]> = Vec::with_capacity(block_count);
    let mut cursor = payload_start;
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

    // Phase 3: pass dictionary to per-block decompression when present.
    let dict_opt: Option<&[u8]> = if dict_data.is_empty() {
        None
    } else {
        Some(dict_data)
    };
    let decompressed_blocks: Vec<CpacResult<Vec<u8>>> = pool.install(|| {
        blocks
            .par_iter()
            .map(|block| {
                let result = crate::decompress_with_dict(block, dict_opt)?;
                Ok(result.data)
            })
            .collect()
    });

    // Phase 2: decode shared MSN metadata once (if present) for reconstruction.
    let shared_msn_meta = if !shared_metadata.is_empty() {
        Some(cpac_msn::decode_metadata_compact(shared_metadata)?)
    } else {
        None
    };

    // Concatenate results, applying MSN reconstruction for flagged blocks.
    let mut output = Vec::with_capacity(original_size);
    for (i, result) in decompressed_blocks.into_iter().enumerate() {
        let block_bytes = result?;
        if block_flags[i] & BLOCK_FLAG_MSN != 0 {
            // Phase 2: this block's data is an MSN residual — reconstruct.
            if let Some(ref meta) = shared_msn_meta {
                let msn_result = meta.clone().with_residual(block_bytes);
                let reconstructed = cpac_msn::reconstruct(&msn_result)?;
                output.extend_from_slice(&reconstructed);
            } else {
                return Err(CpacError::DecompressFailed(
                    "CPBL v2: block has MSN flag but no shared metadata".into(),
                ));
            }
        } else {
            output.extend_from_slice(&block_bytes);
        }
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
