// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Raw FFI bindings to the vendored Lizard (LZ5) compression library.
//!
//! Lizard is an efficient compressor with very fast decompression.
//! Levels 10-49 across 4 modes: fastLZ4, LIZv1, fastLZ4+Huffman, LIZv1+Huffman.
//!
//! See <https://github.com/inikep/lizard> for upstream documentation.

#![allow(non_camel_case_types, clippy::missing_safety_doc)]

use std::os::raw::{c_char, c_int};

extern "C" {
    /// Compress `srcSize` bytes from `src` into `dst` (capacity `maxDstSize`)
    /// at the given `compressionLevel` (10-49).
    ///
    /// Returns the number of bytes written to `dst`, or 0 on failure.
    pub fn Lizard_compress(
        src: *const c_char,
        dst: *mut c_char,
        srcSize: c_int,
        maxDstSize: c_int,
        compressionLevel: c_int,
    ) -> c_int;

    /// Worst-case output size for a given `inputSize`.
    /// Returns 0 if `inputSize` exceeds `LIZARD_MAX_INPUT_SIZE`.
    pub fn Lizard_compressBound(inputSize: c_int) -> c_int;

    /// Decompress `compressedSize` bytes from `source` into `dest`
    /// (capacity `maxDecompressedSize`).
    ///
    /// Returns the number of bytes written to `dest`,
    /// or a negative value on error (malformed / truncated data).
    pub fn Lizard_decompress_safe(
        source: *const c_char,
        dest: *mut c_char,
        compressedSize: c_int,
        maxDecompressedSize: c_int,
    ) -> c_int;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_basic() {
        let input = b"Hello Lizard compression! This is a test of the vendored C library.";
        let bound = unsafe { Lizard_compressBound(input.len() as c_int) };
        assert!(bound > 0, "compressBound should be positive");

        let mut compressed = vec![0u8; bound as usize];
        let comp_size = unsafe {
            Lizard_compress(
                input.as_ptr().cast(),
                compressed.as_mut_ptr().cast(),
                input.len() as c_int,
                bound,
                17, // default level
            )
        };
        assert!(comp_size > 0, "compression should succeed");
        compressed.truncate(comp_size as usize);

        let mut decompressed = vec![0u8; input.len()];
        let decomp_size = unsafe {
            Lizard_decompress_safe(
                compressed.as_ptr().cast(),
                decompressed.as_mut_ptr().cast(),
                comp_size,
                input.len() as c_int,
            )
        };
        assert_eq!(decomp_size as usize, input.len());
        assert_eq!(&decompressed, input);
    }
}
