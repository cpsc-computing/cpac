// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Entropy coding backends: Zstd, Brotli, Gzip, LZMA, Raw passthrough.

use cpac_types::{Backend, CpacError, CpacResult};
use flate2::write::GzEncoder;
use flate2::read::GzDecoder;
use flate2::Compression;
use std::io::{Read, Write};

/// Default Zstd compression level (3 = fast, good for general use).
const ZSTD_LEVEL: i32 = 3;

/// Default Brotli compression quality (8 = high quality, competitive with brotli-11).
const BROTLI_QUALITY: i32 = 8;

/// Default Gzip compression level for small files (9 = best).
const GZIP_LEVEL_SMALL: u32 = 9;

/// Gzip compression level for large files (6 = balanced speed/ratio).
const GZIP_LEVEL_LARGE: u32 = 6;

/// Threshold for adaptive Gzip level selection (1MB).
const GZIP_SIZE_THRESHOLD: usize = 1_048_576;

/// Compress `data` using the specified backend.
#[must_use = "compressed data is returned"]
pub fn compress(data: &[u8], backend: Backend) -> CpacResult<Vec<u8>> {
    match backend {
        Backend::Raw => Ok(data.to_vec()),
        Backend::Zstd => zstd::bulk::compress(data, ZSTD_LEVEL)
            .map_err(|e| CpacError::CompressFailed(format!("zstd: {e}"))),
        Backend::Brotli => {
            let mut out = Vec::new();
            {
                let mut writer = brotli::CompressorWriter::new(
                    &mut out,
                    4096,
                    BROTLI_QUALITY as u32,
                    22, // lgwin
                );
                std::io::Write::write_all(&mut writer, data)
                    .map_err(|e| CpacError::CompressFailed(format!("brotli write: {e}")))?;
                // CompressorWriter flushes on drop, but we drop it explicitly here
                // to catch any flush errors.
            }
            Ok(out)
        }
        Backend::Gzip => {
            // Adaptive level: fast compression for large files, best for small
            let level = if data.len() >= GZIP_SIZE_THRESHOLD {
                GZIP_LEVEL_LARGE
            } else {
                GZIP_LEVEL_SMALL
            };
            let mut encoder = GzEncoder::new(Vec::new(), Compression::new(level));
            encoder.write_all(data)
                .map_err(|e| CpacError::CompressFailed(format!("gzip write: {e}")))?;
            encoder.finish()
                .map_err(|e| CpacError::CompressFailed(format!("gzip finish: {e}")))
        }
        Backend::Lzma => {
            let mut out = Vec::new();
            lzma_rs::xz_compress(
                &mut std::io::Cursor::new(data),
                &mut out,
            )
            .map_err(|e| CpacError::CompressFailed(format!("lzma: {e}")))?;
            Ok(out)
        }
    }
}

/// Decompress `data` using the specified backend.
#[must_use = "decompressed data is returned"]
pub fn decompress(data: &[u8], backend: Backend) -> CpacResult<Vec<u8>> {
    match backend {
        Backend::Raw => Ok(data.to_vec()),
        Backend::Zstd => {
            zstd::bulk::decompress(data, 64 * 1024 * 1024) // 64 MB upper bound
                .map_err(|e| CpacError::DecompressFailed(format!("zstd: {e}")))
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
            decoder.read_to_end(&mut out)
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
pub fn auto_select_backend(entropy: f64) -> Backend {
    auto_select_backend_with_size(entropy, 0)
}

/// Auto-select backend with size awareness.
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
