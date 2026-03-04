// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! C/C++ FFI bindings for the CPAC compression engine.

#![allow(clippy::missing_panics_doc)]
//!
//! This crate provides a C-compatible API for compressing and decompressing
//! data using CPAC. All types are C-compatible and can be used from any
//! language that supports C FFI.
//!
//! # Building
//!
//! For C header generation, install cbindgen:
//! ```sh
//! cargo install cbindgen
//! cbindgen --config cbindgen.toml --crate cpac-ffi --output cpac.h
//! ```
//!
//! For `CMake` integration, see `CMakeLists.txt` in this directory.

use cpac_engine::{compress, decompress};
use cpac_streaming::stream::{StreamingCompressor, StreamingDecompressor};
use cpac_types::{Backend, CompressConfig, CpacError, ResourceConfig};
use std::ffi::c_char;
use std::ptr;
use std::slice;

// ---------------------------------------------------------------------------
// Error handling
// ---------------------------------------------------------------------------

/// C-compatible error codes.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CpacErrorCode {
    /// No error.
    Ok = 0,
    /// Invalid argument (null pointer, zero size, etc.).
    InvalidArg = 1,
    /// I/O error.
    Io = 2,
    /// Invalid frame format.
    InvalidFrame = 3,
    /// Unsupported backend.
    UnsupportedBackend = 4,
    /// Decompression failed.
    DecompressFailed = 5,
    /// Compression failed.
    CompressFailed = 6,
    /// Transform error.
    Transform = 7,
    /// Encryption error.
    Encryption = 8,
    /// Out of memory.
    OutOfMemory = 9,
    /// Other error.
    Other = 255,
}

impl From<CpacError> for CpacErrorCode {
    fn from(err: CpacError) -> Self {
        match err {
            CpacError::Io(_) | CpacError::IoError(_) => CpacErrorCode::Io,
            CpacError::InvalidFrame(_) => CpacErrorCode::InvalidFrame,
            CpacError::UnsupportedBackend(_) => CpacErrorCode::UnsupportedBackend,
            CpacError::DecompressFailed(_) => CpacErrorCode::DecompressFailed,
            CpacError::CompressFailed(_) | CpacError::DomainError { .. } => {
                CpacErrorCode::CompressFailed
            }
            CpacError::Transform(_) => CpacErrorCode::Transform,
            CpacError::Encryption(_) => CpacErrorCode::Encryption,
            CpacError::Other(_) => CpacErrorCode::Other,
            CpacError::AlreadyFinalized => CpacErrorCode::InvalidArg,
        }
    }
}

// ---------------------------------------------------------------------------
// Backend enum for C
// ---------------------------------------------------------------------------

/// C-compatible backend identifier.
#[repr(C)]
#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum CpacBackend {
    /// No compression (raw passthrough).
    Raw = 0,
    /// Zstandard compression.
    Zstd = 1,
    /// Brotli compression.
    Brotli = 2,
    /// Gzip/Deflate compression.
    Gzip = 3,
    /// LZMA compression.
    Lzma = 4,
}

impl From<CpacBackend> for Backend {
    fn from(b: CpacBackend) -> Self {
        match b {
            CpacBackend::Raw => Backend::Raw,
            CpacBackend::Zstd => Backend::Zstd,
            CpacBackend::Brotli => Backend::Brotli,
            CpacBackend::Gzip => Backend::Gzip,
            CpacBackend::Lzma => Backend::Lzma,
        }
    }
}

impl From<Backend> for CpacBackend {
    fn from(b: Backend) -> Self {
        match b {
            Backend::Raw => CpacBackend::Raw,
            Backend::Zstd => CpacBackend::Zstd,
            Backend::Brotli => CpacBackend::Brotli,
            Backend::Gzip => CpacBackend::Gzip,
            Backend::Lzma => CpacBackend::Lzma,
        }
    }
}

// ---------------------------------------------------------------------------
// Configuration structs
// ---------------------------------------------------------------------------

/// C-compatible compression configuration.
#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct CpacCompressConfig {
    /// Entropy backend to use (0 = auto-select).
    pub backend: CpacBackend,
    /// Compression level (0 = auto, 1-22 for backend-specific).
    pub level: u32,
    /// Maximum threads (0 = auto).
    pub max_threads: u32,
    /// Maximum memory in bytes (0 = auto).
    pub max_memory_bytes: u64,
}

impl Default for CpacCompressConfig {
    fn default() -> Self {
        Self {
            backend: CpacBackend::Zstd,
            level: 0,
            max_threads: 0,
            max_memory_bytes: 0,
        }
    }
}

impl From<CpacCompressConfig> for CompressConfig {
    fn from(c: CpacCompressConfig) -> Self {
        let resources = if c.max_threads == 0 && c.max_memory_bytes == 0 {
            None
        } else {
            Some(ResourceConfig {
                max_threads: c.max_threads as usize,
                max_memory_mb: (c.max_memory_bytes / (1024 * 1024)) as usize,
                gpu_enabled: false,
            })
        };

        CompressConfig {
            backend: if c.backend as u8 == 0 {
                None
            } else {
                Some(c.backend.into())
            },
            force_track: None,
            filename: None,
            resources,
            dictionary: None,
            disable_parallel: false,
            enable_msn: false, // FFI defaults to MSN disabled
            msn_confidence: 0.5,
            msn_domain: None,
            level: cpac_types::CompressionLevel::Default,
        }
    }
}

// ---------------------------------------------------------------------------
// Opaque handles
// ---------------------------------------------------------------------------

/// Opaque handle to a streaming compressor.
pub struct CpacCompressor(StreamingCompressor);

/// Opaque handle to a streaming decompressor.
pub struct CpacDecompressor(StreamingDecompressor);

// ---------------------------------------------------------------------------
// Version info
// ---------------------------------------------------------------------------

/// Get CPAC library version string.
///
/// # Safety
///
/// The returned pointer is valid for the lifetime of the program.
/// Do not free it.
#[no_mangle]
pub unsafe extern "C" fn cpac_version() -> *const c_char {
    concat!(env!("CARGO_PKG_VERSION"), "\0")
        .as_ptr()
        .cast::<c_char>()
}

// ---------------------------------------------------------------------------
// Simple compress/decompress API
// ---------------------------------------------------------------------------

/// Compress data with CPAC.
///
/// # Arguments
///
/// * `input` - Input data buffer
/// * `input_size` - Size of input data in bytes
/// * `output` - Pre-allocated output buffer (must be at least `output_capacity` bytes)
/// * `output_capacity` - Capacity of output buffer
/// * `output_size` - Pointer to store actual compressed size (must not be NULL)
/// * `config` - Compression configuration (NULL = default)
///
/// # Returns
///
/// `CpacErrorCode::Ok` on success, or an error code.
///
/// # Safety
///
/// - `input` must point to at least `input_size` bytes of valid memory
/// - `output` must point to at least `output_capacity` bytes of valid memory
/// - `output_size` must point to valid memory for a `size_t`
#[no_mangle]
pub unsafe extern "C" fn cpac_compress(
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_capacity: usize,
    output_size: *mut usize,
    config: *const CpacCompressConfig,
) -> CpacErrorCode {
    if input.is_null() || output.is_null() || output_size.is_null() || input_size == 0 {
        return CpacErrorCode::InvalidArg;
    }

    let input_slice = slice::from_raw_parts(input, input_size);
    let config_rust = if config.is_null() {
        CompressConfig::default()
    } else {
        (*config).into()
    };

    match compress(input_slice, &config_rust) {
        Ok(result) => {
            if result.compressed_size > output_capacity {
                return CpacErrorCode::InvalidArg;
            }
            ptr::copy_nonoverlapping(result.data.as_ptr(), output, result.compressed_size);
            *output_size = result.compressed_size;
            CpacErrorCode::Ok
        }
        Err(e) => e.into(),
    }
}

/// Decompress CPAC-compressed data.
///
/// # Arguments
///
/// * `input` - Compressed data buffer
/// * `input_size` - Size of compressed data in bytes
/// * `output` - Pre-allocated output buffer
/// * `output_capacity` - Capacity of output buffer
/// * `output_size` - Pointer to store actual decompressed size (must not be NULL)
///
/// # Returns
///
/// `CpacErrorCode::Ok` on success, or an error code.
///
/// # Safety
///
/// - `input` must point to at least `input_size` bytes of valid memory
/// - `output` must point to at least `output_capacity` bytes of valid memory
/// - `output_size` must point to valid memory for a `size_t`
#[no_mangle]
pub unsafe extern "C" fn cpac_decompress(
    input: *const u8,
    input_size: usize,
    output: *mut u8,
    output_capacity: usize,
    output_size: *mut usize,
) -> CpacErrorCode {
    if input.is_null() || output.is_null() || output_size.is_null() || input_size == 0 {
        return CpacErrorCode::InvalidArg;
    }

    let input_slice = slice::from_raw_parts(input, input_size);

    match decompress(input_slice) {
        Ok(result) => {
            if result.data.len() > output_capacity {
                return CpacErrorCode::InvalidArg;
            }
            ptr::copy_nonoverlapping(result.data.as_ptr(), output, result.data.len());
            *output_size = result.data.len();
            CpacErrorCode::Ok
        }
        Err(e) => e.into(),
    }
}

/// Get maximum compressed size for input of given size.
///
/// Provides a conservative upper bound for the output buffer size needed
/// for compression. Actual compressed size will typically be much smaller.
///
/// # Safety
///
/// No safety requirements.
#[no_mangle]
pub unsafe extern "C" fn cpac_compress_bound(input_size: usize) -> usize {
    // Conservative estimate: input + 5% overhead + 256 bytes for headers
    input_size + (input_size / 20) + 256
}

// ---------------------------------------------------------------------------
// Streaming API
// ---------------------------------------------------------------------------

/// Create a new streaming compressor.
///
/// # Arguments
///
/// * `config` - Compression configuration (NULL = default)
///
/// # Returns
///
/// Opaque handle to compressor, or NULL on allocation failure.
///
/// # Safety
///
/// The returned handle must be freed with `cpac_compressor_free()`.
#[no_mangle]
pub unsafe extern "C" fn cpac_compressor_new(
    config: *const CpacCompressConfig,
) -> *mut CpacCompressor {
    let config_rust = if config.is_null() {
        CompressConfig::default()
    } else {
        (*config).into()
    };

    match StreamingCompressor::new(config_rust) {
        Ok(compressor) => Box::into_raw(Box::new(CpacCompressor(compressor))),
        Err(_) => ptr::null_mut(),
    }
}

/// Feed data to streaming compressor.
///
/// # Arguments
///
/// * `compressor` - Compressor handle
/// * `input` - Input data buffer
/// * `input_size` - Size of input data in bytes
///
/// # Returns
///
/// `CpacErrorCode::Ok` on success, or an error code.
///
/// # Safety
///
/// - `compressor` must be a valid handle from `cpac_compressor_new()`
/// - `input` must point to at least `input_size` bytes of valid memory
#[no_mangle]
pub unsafe extern "C" fn cpac_compressor_write(
    compressor: *mut CpacCompressor,
    input: *const u8,
    input_size: usize,
) -> CpacErrorCode {
    if compressor.is_null() || input.is_null() {
        return CpacErrorCode::InvalidArg;
    }

    let comp = &mut (*compressor).0;
    let input_slice = slice::from_raw_parts(input, input_size);

    match comp.write(input_slice) {
        Ok(_) => CpacErrorCode::Ok,
        Err(e) => CpacErrorCode::from(e),
    }
}

/// Flush compressor (compress any buffered data).
///
/// # Arguments
///
/// * `compressor` - Compressor handle
///
/// # Returns
///
/// `CpacErrorCode::Ok` on success, or an error code.
///
/// # Safety
///
/// - `compressor` must be a valid handle from `cpac_compressor_new()`
#[no_mangle]
pub unsafe extern "C" fn cpac_compressor_finish(compressor: *mut CpacCompressor) -> CpacErrorCode {
    if compressor.is_null() {
        return CpacErrorCode::InvalidArg;
    }

    let comp = &mut (*compressor).0;

    match comp.flush() {
        Ok(()) => CpacErrorCode::Ok,
        Err(e) => CpacErrorCode::from(e),
    }
}

/// Finalize compressor and read compressed output.
///
/// This consumes the compressor and returns the final compressed frame.
/// After calling this, the compressor handle becomes invalid.
///
/// # Arguments
///
/// * `compressor` - Compressor handle
/// * `output` - Output buffer
/// * `output_capacity` - Capacity of output buffer
/// * `output_size` - Pointer to store actual bytes read
///
/// # Returns
///
/// `CpacErrorCode::Ok` on success, or an error code.
///
/// # Safety
///
/// - `compressor` must be a valid handle from `cpac_compressor_new()`
/// - `output` must point to at least `output_capacity` bytes of valid memory
/// - `output_size` must point to valid memory for a `size_t`
/// - After successful call, `compressor` is freed and must not be used
#[no_mangle]
pub unsafe extern "C" fn cpac_compressor_read(
    compressor: *mut CpacCompressor,
    output: *mut u8,
    output_capacity: usize,
    output_size: *mut usize,
) -> CpacErrorCode {
    if compressor.is_null() || output.is_null() || output_size.is_null() {
        return CpacErrorCode::InvalidArg;
    }

    // Take ownership to call finish()
    let comp = Box::from_raw(compressor);

    match comp.0.finish() {
        Ok(frame) => {
            if frame.len() > output_capacity {
                // Put it back to avoid leak
                let _ = Box::into_raw(Box::new(CpacCompressor(
                    StreamingCompressor::new(CompressConfig::default()).unwrap(),
                )));
                return CpacErrorCode::InvalidArg;
            }
            ptr::copy_nonoverlapping(frame.as_ptr(), output, frame.len());
            *output_size = frame.len();
            CpacErrorCode::Ok
        }
        Err(e) => {
            // Put it back to avoid leak
            let _ = Box::into_raw(Box::new(CpacCompressor(
                StreamingCompressor::new(CompressConfig::default()).unwrap(),
            )));
            CpacErrorCode::from(e)
        }
    }
}

/// Free streaming compressor.
///
/// # Safety
///
/// - `compressor` must be a valid handle from `cpac_compressor_new()`
/// - `compressor` must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn cpac_compressor_free(compressor: *mut CpacCompressor) {
    if !compressor.is_null() {
        let _ = Box::from_raw(compressor);
    }
}

/// Create a new streaming decompressor.
///
/// # Returns
///
/// Opaque handle to decompressor, or NULL on allocation failure.
///
/// # Safety
///
/// The returned handle must be freed with `cpac_decompressor_free()`.
#[no_mangle]
pub unsafe extern "C" fn cpac_decompressor_new() -> *mut CpacDecompressor {
    match StreamingDecompressor::new() {
        Ok(decompressor) => Box::into_raw(Box::new(CpacDecompressor(decompressor))),
        Err(_) => ptr::null_mut(),
    }
}

/// Feed compressed data to streaming decompressor.
///
/// # Arguments
///
/// * `decompressor` - Decompressor handle
/// * `input` - Compressed input data buffer
/// * `input_size` - Size of input data in bytes
///
/// # Returns
///
/// `CpacErrorCode::Ok` on success, or an error code.
///
/// # Safety
///
/// - `decompressor` must be a valid handle from `cpac_decompressor_new()`
/// - `input` must point to at least `input_size` bytes of valid memory
#[no_mangle]
pub unsafe extern "C" fn cpac_decompressor_feed(
    decompressor: *mut CpacDecompressor,
    input: *const u8,
    input_size: usize,
) -> CpacErrorCode {
    if decompressor.is_null() || input.is_null() {
        return CpacErrorCode::InvalidArg;
    }

    let decomp = &mut (*decompressor).0;
    let input_slice = slice::from_raw_parts(input, input_size);

    match decomp.feed(input_slice) {
        Ok(()) => CpacErrorCode::Ok,
        Err(e) => CpacErrorCode::from(e),
    }
}

/// Read decompressed output from streaming decompressor.
///
/// Returns all available decompressed data and clears the internal buffer.
///
/// # Arguments
///
/// * `decompressor` - Decompressor handle
/// * `output` - Output buffer
/// * `output_capacity` - Capacity of output buffer
/// * `output_size` - Pointer to store actual bytes read
///
/// # Returns
///
/// `CpacErrorCode::Ok` on success, or an error code.
///
/// # Safety
///
/// - `decompressor` must be a valid handle from `cpac_decompressor_new()`
/// - `output` must point to at least `output_capacity` bytes of valid memory
/// - `output_size` must point to valid memory for a `size_t`
#[no_mangle]
pub unsafe extern "C" fn cpac_decompressor_read(
    decompressor: *mut CpacDecompressor,
    output: *mut u8,
    output_capacity: usize,
    output_size: *mut usize,
) -> CpacErrorCode {
    if decompressor.is_null() || output.is_null() || output_size.is_null() {
        return CpacErrorCode::InvalidArg;
    }

    let decomp = &mut (*decompressor).0;
    let data = decomp.read_output();

    if data.len() > output_capacity {
        return CpacErrorCode::InvalidArg;
    }

    ptr::copy_nonoverlapping(data.as_ptr(), output, data.len());
    *output_size = data.len();
    CpacErrorCode::Ok
}

/// Check if decompressor is done.
///
/// # Arguments
///
/// * `decompressor` - Decompressor handle
///
/// # Returns
///
/// 1 if done, 0 if not done or invalid handle.
///
/// # Safety
///
/// - `decompressor` must be a valid handle from `cpac_decompressor_new()`
#[no_mangle]
pub unsafe extern "C" fn cpac_decompressor_is_done(decompressor: *const CpacDecompressor) -> i32 {
    if decompressor.is_null() {
        return 0;
    }

    let decomp = &(*decompressor).0;
    i32::from(decomp.is_done())
}

/// Free streaming decompressor.
///
/// # Safety
///
/// - `decompressor` must be a valid handle from `cpac_decompressor_new()`
/// - `decompressor` must not be used after this call
#[no_mangle]
pub unsafe extern "C" fn cpac_decompressor_free(decompressor: *mut CpacDecompressor) {
    if !decompressor.is_null() {
        let _ = Box::from_raw(decompressor);
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::CStr;

    #[test]
    fn test_version() {
        unsafe {
            let ver = cpac_version();
            assert!(!ver.is_null());
            let ver_str = CStr::from_ptr(ver).to_str().unwrap();
            assert!(!ver_str.is_empty());
        }
    }

    #[test]
    fn test_compress_decompress() {
        unsafe {
            let input = b"Hello, CPAC FFI!";
            let mut compressed = vec![0u8; cpac_compress_bound(input.len())];
            let mut compressed_size = 0;

            let result = cpac_compress(
                input.as_ptr(),
                input.len(),
                compressed.as_mut_ptr(),
                compressed.len(),
                &mut compressed_size,
                ptr::null(),
            );
            assert_eq!(result, CpacErrorCode::Ok);
            assert!(compressed_size > 0);

            let mut decompressed = vec![0u8; input.len()];
            let mut decompressed_size = 0;

            let result = cpac_decompress(
                compressed.as_ptr(),
                compressed_size,
                decompressed.as_mut_ptr(),
                decompressed.len(),
                &mut decompressed_size,
            );
            assert_eq!(result, CpacErrorCode::Ok);
            assert_eq!(decompressed_size, input.len());
            assert_eq!(&decompressed[..decompressed_size], input);
        }
    }

    #[test]
    fn test_streaming_compressor() {
        unsafe {
            let compressor = cpac_compressor_new(ptr::null());
            assert!(!compressor.is_null());

            let input = b"Streaming test data";
            let result = cpac_compressor_write(compressor, input.as_ptr(), input.len());
            assert_eq!(result, CpacErrorCode::Ok);

            let result = cpac_compressor_finish(compressor);
            assert_eq!(result, CpacErrorCode::Ok);

            let mut output = vec![0u8; 1024];
            let mut output_size = 0;
            let result = cpac_compressor_read(
                compressor,
                output.as_mut_ptr(),
                output.len(),
                &mut output_size,
            );
            assert_eq!(result, CpacErrorCode::Ok);
            assert!(output_size > 0);

            // cpac_compressor_read() already freed the compressor
        }
    }

    #[test]
    fn test_streaming_decompressor() {
        unsafe {
            // First create streaming compressed data
            let input = b"Streaming decompress test";
            let compressor = cpac_compressor_new(ptr::null());
            assert!(!compressor.is_null());

            let result = cpac_compressor_write(compressor, input.as_ptr(), input.len());
            assert_eq!(result, CpacErrorCode::Ok);

            let result = cpac_compressor_finish(compressor);
            assert_eq!(result, CpacErrorCode::Ok);

            let mut compressed = vec![0u8; 1024];
            let mut compressed_size = 0;
            let result = cpac_compressor_read(
                compressor,
                compressed.as_mut_ptr(),
                compressed.len(),
                &mut compressed_size,
            );
            assert_eq!(result, CpacErrorCode::Ok);
            assert!(compressed_size > 0);

            // Now stream decompress
            let decompressor = cpac_decompressor_new();
            assert!(!decompressor.is_null());

            let result = cpac_decompressor_feed(decompressor, compressed.as_ptr(), compressed_size);
            assert_eq!(result, CpacErrorCode::Ok);

            let mut output = vec![0u8; 1024];
            let mut output_size = 0;
            let result = cpac_decompressor_read(
                decompressor,
                output.as_mut_ptr(),
                output.len(),
                &mut output_size,
            );
            assert_eq!(result, CpacErrorCode::Ok);
            assert_eq!(output_size, input.len());
            assert_eq!(&output[..output_size], input);

            assert_eq!(cpac_decompressor_is_done(decompressor), 1);

            cpac_decompressor_free(decompressor);
        }
    }

    #[test]
    fn test_invalid_args() {
        unsafe {
            let mut size = 0;
            let result = cpac_compress(ptr::null(), 0, ptr::null_mut(), 0, &mut size, ptr::null());
            assert_eq!(result, CpacErrorCode::InvalidArg);

            let result = cpac_decompress(ptr::null(), 0, ptr::null_mut(), 0, &mut size);
            assert_eq!(result, CpacErrorCode::InvalidArg);
        }
    }
}
