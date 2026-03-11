// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Entropy coding backends: Zstd, Brotli, Gzip, LZMA, Raw passthrough.

#![allow(clippy::missing_errors_doc, clippy::missing_panics_doc)]

use cpac_types::{Backend, CompressionLevel, CpacError, CpacResult};
use flate2::read::GzDecoder;
use flate2::write::GzEncoder;
use flate2::Compression;
use std::io::{Read, Write};

/// Gzip compression level for each `CompressionLevel`.
/// Range: 1 (fastest) – 9 (best). Default is typically 6.
fn gzip_level(level: CompressionLevel) -> u32 {
    match level {
        CompressionLevel::UltraFast => 1,
        CompressionLevel::Fast => 3,
        CompressionLevel::Default => 6,
        CompressionLevel::High => 8,
        CompressionLevel::Best => 9,
    }
}

/// Brotli quality for each `CompressionLevel`.
/// Range: 0 (fastest) – 11 (best). Window size set separately.
fn brotli_quality(level: CompressionLevel) -> u32 {
    match level {
        CompressionLevel::UltraFast => 1,
        CompressionLevel::Fast => 4,
        CompressionLevel::Default => 6,
        CompressionLevel::High => 9,
        CompressionLevel::Best => 11,
    }
}

/// Zstd compression level for each `CompressionLevel`.
fn zstd_level(level: CompressionLevel) -> i32 {
    match level {
        CompressionLevel::UltraFast => 1,
        CompressionLevel::Fast => 3,
        CompressionLevel::Default => 6,
        CompressionLevel::High => 12,
        CompressionLevel::Best => 19,
    }
}

/// LZMA preset level for each `CompressionLevel`.
/// Range: 0 (fastest) – 9 (best). Default is 6.
#[allow(dead_code)]
fn lzma_level(level: CompressionLevel) -> u32 {
    match level {
        CompressionLevel::UltraFast => 0,
        CompressionLevel::Fast => 2,
        CompressionLevel::Default => 6,
        CompressionLevel::High => 8,
        CompressionLevel::Best => 9,
    }
}

/// XZ preset level for each `CompressionLevel`.
/// Same mapping as LZMA — both use `xz2::write::XzEncoder` (LZMA2).
fn xz_level(level: CompressionLevel) -> u32 {
    match level {
        CompressionLevel::UltraFast => 0,
        CompressionLevel::Fast => 2,
        CompressionLevel::Default => 6,
        CompressionLevel::High => 8,
        CompressionLevel::Best => 9,
    }
}

/// OpenZL compression level for each `CompressionLevel`.
/// Same mapping as Zstd — OpenZL currently delegates to Zstd internally.
fn openzl_level(level: CompressionLevel) -> i32 {
    zstd_level(level)
}

/// LZHAM compression level for each `CompressionLevel`.
/// Range: 0 (FASTEST) – 4 (UBER). Maps 1:1 to CPAC's 5 levels.
fn lzham_level(level: CompressionLevel) -> u32 {
    match level {
        CompressionLevel::UltraFast => 0, // FASTEST (greedy parser)
        CompressionLevel::Fast => 1,      // FASTER
        CompressionLevel::Default => 2,   // DEFAULT
        CompressionLevel::High => 3,      // BETTER
        CompressionLevel::Best => 4,      // UBER
    }
}

/// LZ4/LZ4_HC compression level for each `CompressionLevel`.
///
/// Negative values → LZ4 fast mode with `|value|` as the acceleration parameter.
/// Positive values → LZ4_HC level (1–12).
fn lz4_level(level: CompressionLevel) -> i32 {
    match level {
        CompressionLevel::UltraFast => -65, // LZ4 fast, accel = 65
        CompressionLevel::Fast => -1,       // LZ4 fast, accel = 1 (default speed)
        CompressionLevel::Default => 4,     // LZ4_HC level 4
        CompressionLevel::High => 9,        // LZ4_HC level 9
        CompressionLevel::Best => 12,       // LZ4_HC level 12 (max)
    }
}

/// Lizard compression level for each `CompressionLevel`.
/// Lizard uses levels 10-49 across 4 modes.
fn lizard_level(level: CompressionLevel) -> i32 {
    match level {
        CompressionLevel::UltraFast => 10, // fastLZ4 mode
        CompressionLevel::Fast => 13,
        CompressionLevel::Default => 26, // LIZv1 mode
        CompressionLevel::High => 32,    // fastLZ4+Huffman
        CompressionLevel::Best => 49,    // LIZv1+Huffman max
    }
}

/// zlib-ng compression level for each `CompressionLevel`.
/// Range: 1 (fastest) – 9 (best). Same semantics as Gzip.
fn zlibng_level(level: CompressionLevel) -> i32 {
    match level {
        CompressionLevel::UltraFast => 1,
        CompressionLevel::Fast => 3,
        CompressionLevel::Default => 6,
        CompressionLevel::High => 8,
        CompressionLevel::Best => 9,
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
            let lvl = gzip_level(level);
            let mut encoder = GzEncoder::new(Vec::new(), Compression::new(lvl));
            encoder
                .write_all(data)
                .map_err(|e| CpacError::CompressFailed(format!("gzip write: {e}")))?;
            encoder
                .finish()
                .map_err(|e| CpacError::CompressFailed(format!("gzip finish: {e}")))
        }
        Backend::Lzma => {
            let lvl = lzma_level(level);
            let mut out = Vec::new();
            {
                let mut encoder = xz2::write::XzEncoder::new(&mut out, lvl);
                encoder
                    .write_all(data)
                    .map_err(|e| CpacError::CompressFailed(format!("lzma write: {e}")))?;
                encoder
                    .finish()
                    .map_err(|e| CpacError::CompressFailed(format!("lzma finish: {e}")))?;
            }
            Ok(out)
        }
        Backend::Xz => {
            let lvl = xz_level(level);
            let mut out = Vec::new();
            {
                let mut encoder = xz2::write::XzEncoder::new(&mut out, lvl);
                encoder
                    .write_all(data)
                    .map_err(|e| CpacError::CompressFailed(format!("xz write: {e}")))?;
                encoder
                    .finish()
                    .map_err(|e| CpacError::CompressFailed(format!("xz finish: {e}")))?;
            }
            Ok(out)
        }
        Backend::Lz4 => {
            let raw = lz4_level(level);
            let mut compressed = Vec::new();
            if raw < 0 {
                // LZ4 fast mode — acceleration = |raw|
                lzzzz::lz4::compress_to_vec(data, &mut compressed, -raw)
                    .map_err(|e| CpacError::CompressFailed(format!("lz4: {e}")))?;
            } else {
                // LZ4_HC mode
                lzzzz::lz4_hc::compress_to_vec(data, &mut compressed, raw)
                    .map_err(|e| CpacError::CompressFailed(format!("lz4_hc: {e}")))?;
            }
            // 4-byte LE size prefix for decompression
            let mut out = Vec::with_capacity(4 + compressed.len());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&compressed);
            Ok(out)
        }
        Backend::Snappy => {
            let mut encoder = snap::raw::Encoder::new();
            let compressed = encoder
                .compress_vec(data)
                .map_err(|e| CpacError::CompressFailed(format!("snappy: {e}")))?;
            // Prefix with original size (4-byte LE) for decompression
            let mut out = Vec::with_capacity(4 + compressed.len());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&compressed);
            Ok(out)
        }
        Backend::OpenZl => {
            // OpenZL delegates to Zstd with the same level mapping.
            let lvl = openzl_level(level);
            zstd::bulk::compress(data, lvl)
                .map_err(|e| CpacError::CompressFailed(format!("openzl/zstd: {e}")))
        }
        Backend::Lzham => {
            let lvl = lzham_level(level);
            let params = cpac_lzham_sys::lzham_compress_params {
                m_level: lvl,
                ..cpac_lzham_sys::lzham_compress_params::default()
            };
            // Allocate generous output buffer
            let mut comp_buf = vec![0u8; data.len() + data.len() / 2 + 1024];
            let mut comp_len = comp_buf.len();
            let status = unsafe {
                cpac_lzham_sys::lzham_compress_memory(
                    &params,
                    comp_buf.as_mut_ptr(),
                    &mut comp_len,
                    data.as_ptr(),
                    data.len(),
                    std::ptr::null_mut(),
                )
            };
            if status != cpac_lzham_sys::LZHAM_COMP_STATUS_SUCCESS {
                return Err(CpacError::CompressFailed(format!(
                    "lzham compress_memory status {status}"
                )));
            }
            comp_buf.truncate(comp_len);
            // 4-byte LE size prefix for decompression
            let mut out = Vec::with_capacity(4 + comp_buf.len());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&comp_buf);
            Ok(out)
        }
        Backend::Lizard => {
            let lvl = lizard_level(level);
            let bound =
                unsafe { cpac_lizard_sys::Lizard_compressBound(data.len() as std::os::raw::c_int) };
            if bound <= 0 {
                return Err(CpacError::CompressFailed(
                    "lizard: input too large for Lizard_compressBound".into(),
                ));
            }
            let mut compressed = vec![0u8; bound as usize];
            let comp_size = unsafe {
                cpac_lizard_sys::Lizard_compress(
                    data.as_ptr().cast(),
                    compressed.as_mut_ptr().cast(),
                    data.len() as std::os::raw::c_int,
                    bound,
                    lvl as std::os::raw::c_int,
                )
            };
            if comp_size <= 0 {
                return Err(CpacError::CompressFailed(
                    "lizard: Lizard_compress returned 0 (compression failed)".into(),
                ));
            }
            compressed.truncate(comp_size as usize);
            // Prefix with original size (4-byte LE) for decompression
            let mut out = Vec::with_capacity(4 + compressed.len());
            out.extend_from_slice(&(data.len() as u32).to_le_bytes());
            out.extend_from_slice(&compressed);
            Ok(out)
        }
        Backend::ZlibNg => {
            let lvl = zlibng_level(level);
            let compressed = zlibng_compress(data, lvl)?;
            Ok(compressed)
        }
    }
}

/// Compress data using zlib-ng with gzip framing.
fn zlibng_compress(data: &[u8], level: i32) -> CpacResult<Vec<u8>> {
    use libz_ng_sys::*;
    unsafe {
        let mut stream_mem = std::mem::MaybeUninit::<z_stream>::zeroed();
        let strm = stream_mem.as_mut_ptr();
        // zeroed memory sets zalloc/zfree/opaque to zero;
        // deflateInit2_ detects null alloc and installs defaults.
        let ret = deflateInit2_(
            strm,
            level,
            Z_DEFLATED,
            15 + 16,
            8,
            Z_DEFAULT_STRATEGY,
            zlibVersion(),
            std::mem::size_of::<z_stream>() as i32,
        );
        if ret != Z_OK {
            return Err(CpacError::CompressFailed(format!(
                "zlib-ng deflateInit2 failed: {ret}"
            )));
        }
        let bound = deflateBound(strm, data.len() as _);
        let mut out = vec![0u8; bound as usize];
        (*strm).next_in = data.as_ptr() as *mut u8;
        (*strm).avail_in = data.len() as _;
        (*strm).next_out = out.as_mut_ptr();
        (*strm).avail_out = out.len() as _;
        let ret = deflate(strm, Z_FINISH);
        if ret != Z_STREAM_END {
            deflateEnd(strm);
            return Err(CpacError::CompressFailed(format!(
                "zlib-ng deflate failed: {ret}"
            )));
        }
        let written = (*strm).total_out;
        deflateEnd(strm);
        out.truncate(written);
        Ok(out)
    }
}

/// Decompress gzip data using zlib-ng.
fn zlibng_decompress(data: &[u8]) -> CpacResult<Vec<u8>> {
    use libz_ng_sys::*;
    unsafe {
        let mut stream_mem = std::mem::MaybeUninit::<z_stream>::zeroed();
        let strm = stream_mem.as_mut_ptr();
        // zeroed memory sets zalloc/zfree/opaque to zero;
        // inflateInit2_ detects null alloc and installs defaults.
        // 15 + 16 = auto-detect gzip header
        let ret = inflateInit2_(
            strm,
            15 + 16,
            zlibVersion(),
            std::mem::size_of::<z_stream>() as i32,
        );
        if ret != Z_OK {
            return Err(CpacError::DecompressFailed(format!(
                "zlib-ng inflateInit2 failed: {ret}"
            )));
        }
        (*strm).next_in = data.as_ptr() as *mut u8;
        (*strm).avail_in = data.len() as _;
        let mut out = Vec::with_capacity(data.len() * 4);
        let mut buf = [0u8; 65536];
        loop {
            (*strm).next_out = buf.as_mut_ptr();
            (*strm).avail_out = buf.len() as _;
            let ret = inflate(strm, Z_NO_FLUSH);
            let produced = buf.len() - (*strm).avail_out as usize;
            out.extend_from_slice(&buf[..produced]);
            if ret == Z_STREAM_END {
                break;
            }
            if ret != Z_OK {
                inflateEnd(strm);
                return Err(CpacError::DecompressFailed(format!(
                    "zlib-ng inflate failed: {ret}"
                )));
            }
        }
        inflateEnd(strm);
        Ok(out)
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
                // Use streaming decoder — bulk::decompress requires a size cap
                // which fails for files larger than the cap (e.g. 195 MB logs).
                let mut decoder = zstd::stream::Decoder::new(data)
                    .map_err(|e| CpacError::DecompressFailed(format!("zstd decoder: {e}")))?;
                let mut out = Vec::new();
                decoder
                    .read_to_end(&mut out)
                    .map_err(|e| CpacError::DecompressFailed(format!("zstd read: {e}")))?;
                Ok(out)
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
        Backend::Lzma | Backend::Xz => {
            let mut decoder = xz2::read::XzDecoder::new(data);
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .map_err(|e| CpacError::DecompressFailed(format!("xz/lzma: {e}")))?;
            Ok(out)
        }
        Backend::Lz4 => {
            if data.len() < 4 {
                return Err(CpacError::DecompressFailed(
                    "lz4: data too short for size prefix".into(),
                ));
            }
            let orig_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
            let mut decompressed = vec![0u8; orig_size];
            lzzzz::lz4::decompress(&data[4..], &mut decompressed)
                .map_err(|e| CpacError::DecompressFailed(format!("lz4: {e}")))?;
            Ok(decompressed)
        }
        Backend::Snappy => {
            if data.len() < 4 {
                return Err(CpacError::DecompressFailed(
                    "snappy: data too short for size prefix".into(),
                ));
            }
            let _orig_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
            let mut decoder = snap::raw::Decoder::new();
            let decompressed = decoder
                .decompress_vec(&data[4..])
                .map_err(|e| CpacError::DecompressFailed(format!("snappy: {e}")))?;
            Ok(decompressed)
        }
        Backend::OpenZl => {
            // OpenZL delegates to Zstd for decompression (streaming for large data).
            let mut decoder = zstd::stream::Decoder::new(data)
                .map_err(|e| CpacError::DecompressFailed(format!("openzl decoder: {e}")))?;
            let mut out = Vec::new();
            decoder
                .read_to_end(&mut out)
                .map_err(|e| CpacError::DecompressFailed(format!("openzl read: {e}")))?;
            Ok(out)
        }
        Backend::Lzham => {
            if data.len() < 4 {
                return Err(CpacError::DecompressFailed(
                    "lzham: data too short for size prefix".into(),
                ));
            }
            let orig_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
            let params = cpac_lzham_sys::lzham_decompress_params::default();
            let mut decomp_buf = vec![0u8; orig_size];
            let mut decomp_len = decomp_buf.len();
            let status = unsafe {
                cpac_lzham_sys::lzham_decompress_memory(
                    &params,
                    decomp_buf.as_mut_ptr(),
                    &mut decomp_len,
                    data[4..].as_ptr(),
                    data.len() - 4,
                    std::ptr::null_mut(),
                )
            };
            if status != cpac_lzham_sys::LZHAM_DECOMP_STATUS_SUCCESS {
                return Err(CpacError::DecompressFailed(format!(
                    "lzham decompress_memory status {status}"
                )));
            }
            decomp_buf.truncate(decomp_len);
            Ok(decomp_buf)
        }
        Backend::Lizard => {
            if data.len() < 4 {
                return Err(CpacError::DecompressFailed(
                    "lizard: data too short for size prefix".into(),
                ));
            }
            let orig_size = u32::from_le_bytes([data[0], data[1], data[2], data[3]]) as usize;
            let mut decompressed = vec![0u8; orig_size];
            let decomp_size = unsafe {
                cpac_lizard_sys::Lizard_decompress_safe(
                    data[4..].as_ptr().cast(),
                    decompressed.as_mut_ptr().cast(),
                    (data.len() - 4) as std::os::raw::c_int,
                    orig_size as std::os::raw::c_int,
                )
            };
            if decomp_size < 0 {
                return Err(CpacError::DecompressFailed(format!(
                    "lizard: Lizard_decompress_safe failed ({})",
                    decomp_size
                )));
            }
            decompressed.truncate(decomp_size as usize);
            Ok(decompressed)
        }
        Backend::ZlibNg => {
            let decompressed = zlibng_decompress(data)?;
            Ok(decompressed)
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
    fn roundtrip_xz() {
        let data = b"hello world, this is a test of xz compression";
        let compressed = compress(data, Backend::Xz).unwrap();
        let decompressed = decompress(&compressed, Backend::Xz).unwrap();
        assert_eq!(&decompressed, data);
    }

    #[test]
    fn roundtrip_lz4() {
        let data = b"hello world, this is a test of lz4 compression";
        let compressed = compress(data, Backend::Lz4).unwrap();
        let decompressed = decompress(&compressed, Backend::Lz4).unwrap();
        assert_eq!(&decompressed, data);
    }

    #[test]
    fn roundtrip_lz4_all_levels() {
        let data =
            b"LZ4 levels: fast acceleration (UltraFast/Fast) and HC modes (Default/High/Best)";
        for level in [
            CompressionLevel::UltraFast,
            CompressionLevel::Fast,
            CompressionLevel::Default,
            CompressionLevel::High,
            CompressionLevel::Best,
        ] {
            let compressed = compress_at_level(data, Backend::Lz4, level, None)
                .unwrap_or_else(|e| panic!("lz4 compress at {level:?} failed: {e}"));
            let decompressed = decompress(&compressed, Backend::Lz4)
                .unwrap_or_else(|e| panic!("lz4 decompress at {level:?} failed: {e}"));
            assert_eq!(&decompressed, data, "lz4 roundtrip failed at {level:?}");
        }
    }

    #[test]
    fn roundtrip_snappy() {
        let data = b"hello world, this is a test of snappy compression";
        let compressed = compress(data, Backend::Snappy).unwrap();
        let decompressed = decompress(&compressed, Backend::Snappy).unwrap();
        assert_eq!(&decompressed, data);
    }

    #[test]
    fn roundtrip_openzl() {
        let data = b"hello world, this is a test of openzl compression";
        let compressed = compress(data, Backend::OpenZl).unwrap();
        let decompressed = decompress(&compressed, Backend::OpenZl).unwrap();
        assert_eq!(&decompressed, data);
    }

    #[test]
    fn roundtrip_zlibng() {
        let data = b"hello world, this is a test of zlib-ng compression";
        let compressed = compress(data, Backend::ZlibNg).unwrap();
        let decompressed = decompress(&compressed, Backend::ZlibNg).unwrap();
        assert_eq!(&decompressed, data);
    }

    #[test]
    fn roundtrip_lzham() {
        let data = b"hello world, this is a test of lzham compression padding";
        let compressed = compress(data, Backend::Lzham).unwrap();
        let decompressed = decompress(&compressed, Backend::Lzham).unwrap();
        assert_eq!(&decompressed, data);
    }

    #[test]
    fn roundtrip_lzham_all_levels() {
        let data = b"LZHAM levels 0-4: FASTEST, FASTER, DEFAULT, BETTER, UBER level test";
        for level in [
            CompressionLevel::UltraFast,
            CompressionLevel::Fast,
            CompressionLevel::Default,
            CompressionLevel::High,
            CompressionLevel::Best,
        ] {
            let compressed = compress_at_level(data, Backend::Lzham, level, None)
                .unwrap_or_else(|e| panic!("lzham compress at {level:?} failed: {e}"));
            let decompressed = decompress(&compressed, Backend::Lzham)
                .unwrap_or_else(|e| panic!("lzham decompress at {level:?} failed: {e}"));
            assert_eq!(&decompressed, data, "lzham roundtrip failed at {level:?}");
        }
    }

    #[test]
    fn roundtrip_lizard() {
        let data = b"hello world, this is a test of lizard compression";
        let compressed = compress(data, Backend::Lizard).unwrap();
        let decompressed = decompress(&compressed, Backend::Lizard).unwrap();
        assert_eq!(&decompressed, data);
    }

    #[test]
    fn roundtrip_lizard_all_levels() {
        let data = b"Lizard levels 10-49 across four modes: fastLZ4, LIZv1, fastLZ4+Huffman, LIZv1+Huffman";
        for level in [
            CompressionLevel::UltraFast,
            CompressionLevel::Fast,
            CompressionLevel::Default,
            CompressionLevel::High,
            CompressionLevel::Best,
        ] {
            let compressed = compress_at_level(data, Backend::Lizard, level, None)
                .unwrap_or_else(|e| panic!("lizard compress at {level:?} failed: {e}"));
            let decompressed = decompress(&compressed, Backend::Lizard)
                .unwrap_or_else(|e| panic!("lizard decompress at {level:?} failed: {e}"));
            assert_eq!(&decompressed, data, "lizard roundtrip failed at {level:?}");
        }
    }

    #[test]
    fn zlibng_cross_compat_with_gzip() {
        // Compress with zlib-ng, decompress with gzip (flate2) — both use gzip framing
        let data = b"cross compatibility test between zlib-ng and gzip";
        let compressed = compress(data, Backend::ZlibNg).unwrap();
        let decompressed = decompress(&compressed, Backend::Gzip).unwrap();
        assert_eq!(&decompressed, data);
    }

    #[test]
    fn auto_select() {
        assert_eq!(auto_select_backend(0.5), Backend::Raw);
        assert_eq!(auto_select_backend(4.0), Backend::Zstd);
        assert_eq!(auto_select_backend(7.5), Backend::Brotli);
    }
}
