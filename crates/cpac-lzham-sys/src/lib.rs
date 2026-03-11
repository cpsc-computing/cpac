// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Raw FFI bindings for the vendored LZHAM C++ codec.

#![allow(non_camel_case_types, non_upper_case_globals, non_snake_case)]

use std::os::raw::{c_int, c_uint, c_void};

// ---------------------------------------------------------------------------
// Compression level enum (maps to lzham_compress_level)
// ---------------------------------------------------------------------------

pub const LZHAM_COMP_LEVEL_FASTEST: c_uint = 0;
pub const LZHAM_COMP_LEVEL_FASTER: c_uint = 1;
pub const LZHAM_COMP_LEVEL_DEFAULT: c_uint = 2;
pub const LZHAM_COMP_LEVEL_BETTER: c_uint = 3;
pub const LZHAM_COMP_LEVEL_UBER: c_uint = 4;

// ---------------------------------------------------------------------------
// Compression status codes
// ---------------------------------------------------------------------------

pub const LZHAM_COMP_STATUS_SUCCESS: c_uint = 3;
pub const LZHAM_COMP_STATUS_OUTPUT_BUF_TOO_SMALL: c_uint = 7;

// ---------------------------------------------------------------------------
// Decompression status codes
// ---------------------------------------------------------------------------

pub const LZHAM_DECOMP_STATUS_SUCCESS: c_uint = 3;

// ---------------------------------------------------------------------------
// Dictionary size bounds
// ---------------------------------------------------------------------------

pub const LZHAM_MIN_DICT_SIZE_LOG2: c_uint = 15;

#[cfg(target_pointer_width = "64")]
pub const LZHAM_MAX_DICT_SIZE_LOG2: c_uint = 29;

#[cfg(target_pointer_width = "32")]
pub const LZHAM_MAX_DICT_SIZE_LOG2: c_uint = 26;

// ---------------------------------------------------------------------------
// Compress params (matches lzham_compress_params layout)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug, Clone)]
pub struct lzham_compress_params {
    pub m_struct_size: c_uint,
    pub m_dict_size_log2: c_uint,
    pub m_level: c_uint, // lzham_compress_level
    pub m_table_update_rate: c_uint,
    pub m_max_helper_threads: c_int,
    pub m_compress_flags: c_uint,
    pub m_num_seed_bytes: c_uint,
    pub m_pSeed_bytes: *const c_void,
    pub m_table_max_update_interval: c_uint,
    pub m_table_update_interval_slow_rate: c_uint,
}

impl Default for lzham_compress_params {
    fn default() -> Self {
        Self {
            m_struct_size: std::mem::size_of::<Self>() as c_uint,
            m_dict_size_log2: 20, // 1 MB dictionary
            m_level: LZHAM_COMP_LEVEL_DEFAULT,
            m_table_update_rate: 0,
            m_max_helper_threads: 0,
            m_compress_flags: 0,
            m_num_seed_bytes: 0,
            m_pSeed_bytes: std::ptr::null(),
            m_table_max_update_interval: 0,
            m_table_update_interval_slow_rate: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// Decompress params (matches lzham_decompress_params layout)
// ---------------------------------------------------------------------------

#[repr(C)]
#[derive(Debug, Clone)]
pub struct lzham_decompress_params {
    pub m_struct_size: c_uint,
    pub m_dict_size_log2: c_uint,
    pub m_table_update_rate: c_uint,
    pub m_decompress_flags: c_uint,
    pub m_num_seed_bytes: c_uint,
    pub m_pSeed_bytes: *const c_void,
    pub m_table_max_update_interval: c_uint,
    pub m_table_update_interval_slow_rate: c_uint,
}

impl Default for lzham_decompress_params {
    fn default() -> Self {
        Self {
            m_struct_size: std::mem::size_of::<Self>() as c_uint,
            m_dict_size_log2: 20, // must match compression
            m_table_update_rate: 0,
            m_decompress_flags: 0,
            m_num_seed_bytes: 0,
            m_pSeed_bytes: std::ptr::null(),
            m_table_max_update_interval: 0,
            m_table_update_interval_slow_rate: 0,
        }
    }
}

// ---------------------------------------------------------------------------
// FFI declarations
// ---------------------------------------------------------------------------

extern "C" {
    /// Single-call compression. Returns a `lzham_compress_status_t`.
    pub fn lzham_compress_memory(
        pParams: *const lzham_compress_params,
        pDst_buf: *mut u8,
        pDst_len: *mut usize,
        pSrc_buf: *const u8,
        src_len: usize,
        pAdler32: *mut c_uint,
    ) -> c_uint;

    /// Single-call decompression. Returns a `lzham_decompress_status_t`.
    pub fn lzham_decompress_memory(
        pParams: *const lzham_decompress_params,
        pDst_buf: *mut u8,
        pDst_len: *mut usize,
        pSrc_buf: *const u8,
        src_len: usize,
        pAdler32: *mut c_uint,
    ) -> c_uint;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_basic() {
        let data = b"hello from vendored LZHAM roundtrip test data padding";

        // Compress
        let comp_params = lzham_compress_params::default();

        let mut comp_buf = vec![0u8; data.len() * 2 + 1024];
        let mut comp_len = comp_buf.len();
        let status = unsafe {
            lzham_compress_memory(
                &comp_params,
                comp_buf.as_mut_ptr(),
                &mut comp_len,
                data.as_ptr(),
                data.len(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(
            status, LZHAM_COMP_STATUS_SUCCESS,
            "compress failed: {status}"
        );
        comp_buf.truncate(comp_len);

        // Decompress
        let decomp_params = lzham_decompress_params::default();
        let mut decomp_buf = vec![0u8; data.len()];
        let mut decomp_len = decomp_buf.len();
        let status = unsafe {
            lzham_decompress_memory(
                &decomp_params,
                decomp_buf.as_mut_ptr(),
                &mut decomp_len,
                comp_buf.as_ptr(),
                comp_buf.len(),
                std::ptr::null_mut(),
            )
        };
        assert_eq!(
            status, LZHAM_DECOMP_STATUS_SUCCESS,
            "decompress failed: {status}"
        );
        decomp_buf.truncate(decomp_len);
        assert_eq!(&decomp_buf, data);
    }
}
