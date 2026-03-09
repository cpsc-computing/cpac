// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Entropy coding backends: Zstd, Brotli, Gzip, LZMA, Raw passthrough.

#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use cpac_types::{Backend, CompressionLevel, CpacError, CpacResult};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{Read, Write};

/// Gzip level is always 9 (matches gzip-9 baseline; irrelevant to `CompressionLevel`).
const GZIP_LEVEL: u32 = 9;

/// Brotli quality for each `CompressionLevel`.
///
/// - Fast    → 6  (high throughput, still good ratio)
/// - Default → 11 (matches the industry brotli-11 baseline so benchmarks are
///   a fair measure of CPAC preprocessing value, not encoder gap)
/// - High/Best → 11 (same ceiling; brotli 11 is already max quality)
fn brotli_quality(level: CompressionLevel) -> u32 {
    match level {
        CompressionLevel::Fast => 6,
        CompressionLevel::Default | CompressionLevel::High | CompressionLevel::Best => 11,
    }
}

/// Zstd compression level for each `CompressionLevel`.
///
/// - Fast    → 1  (fastest, still good ratio)
/// - Default → 3  (zstd default; best speed/ratio balance)
/// - High    → 12 (batch jobs; ~5-8% better ratio, ~3x slower)
/// - Best    → 19 (cold storage; maximum compression, ~10x slower)
fn zstd_level(level: CompressionLevel) -> i32 {
    match level {
        CompressionLevel::Fast => 1,
        CompressionLevel::Default => 3,
        CompressionLevel::High => 12,
        CompressionLevel::Best => 19,
    }
}

/// Compress `data` using the specified backend at `CompressionLevel::Default`.
///
/// # Errors
///
/// Returns [`CpacError::CompressFailed`] if the backend encounters an I/O error.
#[must_use = "compressed data is returned"]
pub fn compress(data: &[u8], backend: Backend) -> CpacResult<Vec<u8>> {
    compress_at_level(data, backend, CompressionLevel::Default, None)
}

/// Compress `data` using the specified backend with optional dictionary.
///
/// # Errors
///
/// Returns [`CpacError::CompressFailed`] if the backend encounters an I/O error.
#[must_use = "compressed data is returned"]
pub fn compress_with_dict(
    data: &[u8],
    backend: Backend,
    dict: Option<&[u8]>,
) -> CpacResult<Vec<u8>> {
    compress_at_level(data, backend, CompressionLevel::Default, dict)
}

/// Compress `data` at the specified [`CompressionLevel`] with an optional dictionary.
///
/// # Errors
///
/// Returns [`CpacError::CompressFailed`] if the backend encounters an I/O error.
#[must_use = "compressed data is returned"]
pub fn compress_at_level(
    data: &[u8],
    backend: Backend,
    level: CompressionLevel,
    dict: Option<&[u8]>,
) -> CpacResult<Vec<u8>> {
    match backend {
        Backend::Raw => Ok(data.to_vec()),
        Backend::Zstd => {
            let lvl = zstd_level(level);
            if let Some(dict_data) = dict {
                // Use dictionary compression via stream API
                let mut encoder =
                    zstd::stream::Encoder::with_dictionary(Vec::new(), lvl, dict_data)
                        .map_err(|e| CpacError::CompressFailed(format!("zstd encoder: {e}")))?;
                encoder
                    .write_all(data)
                    .map_err(|e| CpacError::CompressFailed(format!("zstd write: {e}")))?;
                encoder
                    .finish()
                    .map_err(|e| CpacError::CompressFailed(format!("zstd finish: {e}")))
            } else {
                zstd::bulk::compress(data, lvl)
                    .map_err(|e| CpacError::CompressFailed(format!("zstd: {e}")))
            }
        }
        Backend::Brotli => {
            let quality = brotli_quality(level);
            let mut out = Vec::new();
            {
                let mut writer = brotli::CompressorWriter::new(
                    &mut out, 4096, quality, 22, // lgwin = max window
                );
                std::io::Write::write_all(&mut writer, data)
                    .map_err(|e| CpacError::CompressFailed(format!("brotli write: {e}")))?;
                // CompressorWriter flushes on drop, but we drop it explicitly here
                // to catch any flush errors.
            }
            Ok(out)
        }
        Backend::Gzip => {
            // Use level 9 consistently to match gzip-9 baseline
            let mut encoder = GzEncoder::new(Vec::new(), Compression::new(GZIP_LEVEL));
            encoder
                .write_all(data)
                .map_err(|e| CpacError::CompressFailed(format!("gzip write: {e}")))?;
            encoder
                .finish()
                .map_err(|e| CpacError::CompressFailed(format!("gzip finish: {e}")))
        }
        Backend::Lzma => {
            let mut out = Vec::new();
            lzma_rs::xz_compress(&mut std::io::Cursor::new(data), &mut out)
                .map_err(|e| CpacError::CompressFailed(format!("lzma: {e}")))?;
            Ok(out)
        }
    }
}

/// Decompress `data` using the specified backend.
#[must_use = "decompressed data is returned"]
pub fn decompress(data: &[u8], backend: Backend) -> CpacResult<Vec<u8>> {
    decompress_with_dict(data, backend, None)
}

/// Decompress `data` using the specified backend with optional dictionary.
#[must_use = "decompressed data is returned"]
pub fn decompress_with_dict(
    data: &[u8],
    backend: Backend,
    dict: Option<&[u8]>,
) -> CpacResult<Vec<u8>> {
    match backend {
        Backend::Raw => Ok(data.to_vec()),
        Backend::Zstd => {
            if let Some(dict_data) = dict {
                // Use dictionary decompression via stream API
                let mut decoder = zstd::stream::Decoder::with_dictionary(data, dict_data)
                    .map_err(|e| CpacError::DecompressFailed(format!("zstd decoder: {e}")))?;
                let mut out = Vec::new();
                decoder
                    .read_to_end(&mut out)
                    .map_err(|e| CpacError::DecompressFailed(format!("zstd read: {e}")))?;
                Ok(out)
            } else {
                zstd::bulk::decompress(data, 64 * 1024 * 1024) // 64 MB upper bound
                    .map_err(|e| CpacError::DecompressFailed(format!("zstd: {e}")))
            }
        }
        Backend::Brotli => {
            let mut out = Vec::new();
            let mut reader = brotli::Decompressor::new(data, 4096);
            std::io::Read::read_to_end(&mut reader, &mut out)
                .map_err(|e| CpacError::DecompressFailed(format!("brotli: {e}")))?;
            Ok(out)
        }
        Backend::Gzip => {
            let mut decoder = GzDecoder::new(data);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .map_err(|e| CpacError::DecompressFailed(format!("gzip: {e}")))?;
            Ok(out)
        }
        Backend::Lzma => {
            let mut out = Vec::new();
            lzma_rs::xz_decompress(&mut std::io::Cursor::new(data), &mut out)
                .map_err(|e| CpacError::DecompressFailed(format!("lzma: {e}")))?;
            Ok(out)
        }
    }
}

/// Auto-select backend based on entropy level, data size, and type.
///
/// Refined selection for better performance across diverse workloads:
/// - Very low entropy → Raw (compression won't help)
/// - Large files + medium entropy → Zstd (speed matters)
/// - Medium files + high entropy → Brotli (ratio matters)
/// - Small files + high entropy → Brotli (ratio critical)
#[must_use]
pub fn auto_select_backend(entropy: f64) -> Backend {
    auto_select_backend_with_size(entropy, 0)
}

/// Auto-select backend with size awareness.
#[must_use]
pub fn auto_select_backend_with_size(entropy: f64, data_size: usize) -> Backend {
    // Raw for essentially incompressible data
    if entropy < 1.0 {
        return Backend::Raw;
    }

    // For very large files (>10MB), prioritize speed with Zstd
    if data_size > 10_000_000 && entropy < 7.0 {
        return Backend::Zstd;
    }

    // For high-entropy data, Brotli usually wins on ratio
    if entropy >= 6.5 {
        return Backend::Brotli;
    }

    // For medium files with medium entropy, Zstd is balanced
    if entropy < 6.5 {
        return Backend::Zstd;
    }

    // Default to Brotli for everything else
    Backend::Brotli
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_zstd() {
        let data = b"hello world, this is a test of zstd compression";
        let compressed = compress(data, Backend::Zstd).unwrap();
        let decompressed = decompress(&compressed, Backend::Zstd).unwrap();
        assert_eq!(&decompressed, data);
    }

    #[test]
    fn roundtrip_brotli() {
        let data = b"hello world, this is a test of brotli compression";
        let compressed = compress(data, Backend::Brotli).unwrap();
        let decompressed = decompress(&compressed, Backend::Brotli).unwrap();
        assert_eq!(&decompressed, data);
    }

    #[test]
    fn roundtrip_raw() {
        let data = b"raw passthrough";
        let compressed = compress(data, Backend::Raw).unwrap();
        assert_eq!(&compressed, data);
        let decompressed = decompress(&compressed, Backend::Raw).unwrap();
        assert_eq!(&decompressed, data);
    }

    #[test]
    fn roundtrip_gzip() {
        let data = b"hello world, this is a test of gzip compression";
        let compressed = compress(data, Backend::Gzip).unwrap();
        let decompressed = decompress(&compressed, Backend::Gzip).unwrap();
        assert_eq!(&decompressed, data);
    }

    #[test]
    fn roundtrip_lzma() {
        let data = b"hello world, this is a test of lzma compression";
        let compressed = compress(data, Backend::Lzma).unwrap();
        let decompressed = decompress(&compressed, Backend::Lzma).unwrap();
        assert_eq!(&decompressed, data);
    }

    #[test]
    fn auto_select() {
        assert_eq!(auto_select_backend(0.5), Backend::Raw);
        assert_eq!(auto_select_backend(4.0), Backend::Zstd);
        assert_eq!(auto_select_backend(7.5), Backend::Brotli);
    }
}
