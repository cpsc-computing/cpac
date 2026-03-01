// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Entropy coding backends: Zstd, Brotli, Raw passthrough.

use cpac_types::{Backend, CpacError, CpacResult};

/// Default Zstd compression level.
const ZSTD_LEVEL: i32 = 3;

/// Default Brotli compression quality.
const BROTLI_QUALITY: i32 = 6;

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
    }
}

/// Auto-select backend based on entropy level.
///
/// - entropy < 1.0 → Raw (already very low entropy, compression won't help much)
/// - entropy < 6.0 → Zstd (good general-purpose)
/// - entropy >= 6.0 → Brotli (better on high-entropy structured data)
pub fn auto_select_backend(entropy: f64) -> Backend {
    if entropy < 1.0 {
        Backend::Raw
    } else if entropy < 6.0 {
        Backend::Zstd
    } else {
        Backend::Brotli
    }
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
    fn auto_select() {
        assert_eq!(auto_select_backend(0.5), Backend::Raw);
        assert_eq!(auto_select_backend(4.0), Backend::Zstd);
        assert_eq!(auto_select_backend(7.5), Backend::Brotli);
    }
}
