// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Memory-mapped file compression / decompression.
//!
//! Uses `memmap2` to map files into virtual memory, avoiding the need
//! to allocate a contiguous buffer for very large inputs.  Compresses
//! the mapped region through the standard CPAC pipeline.

use cpac_types::{CompressConfig, CompressResult, CpacError, CpacResult, DecompressResult};
use std::path::Path;

/// Threshold above which mmap is preferred (64 MiB).
pub const MMAP_THRESHOLD: u64 = 64 * 1024 * 1024;

/// Compress a file via memory-mapping.
///
/// The file is mapped read-only and the resulting slice is passed to
/// the standard CPAC compressor.  This avoids a heap allocation for
/// the input data, which is beneficial for files larger than
/// [`MMAP_THRESHOLD`].
pub fn mmap_compress(path: &Path, config: &CompressConfig) -> CpacResult<CompressResult> {
    let file = std::fs::File::open(path)
        .map_err(|e| CpacError::IoError(format!("{}: {e}", path.display())))?;

    // SAFETY: the file is opened read-only and we hold it open for the
    // lifetime of the mmap.  The mapping is only read, never written.
    let mmap = unsafe {
        memmap2::Mmap::map(&file)
            .map_err(|e| CpacError::IoError(format!("mmap {}: {e}", path.display())))?
    };

    cpac_engine::compress(&mmap, config)
}

/// Decompress a file via memory-mapping.
pub fn mmap_decompress(path: &Path) -> CpacResult<DecompressResult> {
    let file = std::fs::File::open(path)
        .map_err(|e| CpacError::IoError(format!("{}: {e}", path.display())))?;

    let mmap = unsafe {
        memmap2::Mmap::map(&file)
            .map_err(|e| CpacError::IoError(format!("mmap {}: {e}", path.display())))?
    };

    // Auto-detect CPBL vs standard frame
    if cpac_engine::is_cpbl(&mmap) {
        let threads = cpac_engine::auto_resource_config().max_threads;
        cpac_engine::decompress_parallel(&mmap, threads)
    } else {
        cpac_engine::decompress(&mmap)
    }
}

/// Returns `true` if the file size exceeds [`MMAP_THRESHOLD`] and
/// mmap should be preferred over a normal `std::fs::read`.
#[must_use] 
pub fn should_use_mmap(path: &Path) -> bool {
    path.metadata()
        .map(|m| m.len() >= MMAP_THRESHOLD)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_file(data: &[u8]) -> tempfile::NamedTempFile {
        let mut f = tempfile::NamedTempFile::new().unwrap();
        f.write_all(data).unwrap();
        f.flush().unwrap();
        f
    }

    #[test]
    fn mmap_compress_roundtrip() {
        let data = b"Hello mmap world! ".repeat(100);
        let file = write_temp_file(&data);

        let config = CompressConfig::default();
        let compressed = mmap_compress(file.path(), &config).unwrap();
        assert!(compressed.compressed_size > 0);

        // Write compressed data to another temp file and mmap-decompress
        let cfile = write_temp_file(&compressed.data);
        let decompressed = mmap_decompress(cfile.path()).unwrap();
        assert_eq!(decompressed.data, data);
    }

    #[test]
    fn should_use_mmap_small_file() {
        let file = write_temp_file(b"tiny");
        assert!(!should_use_mmap(file.path()));
    }

    #[test]
    fn mmap_compress_empty() {
        let file = write_temp_file(b"");
        let config = CompressConfig::default();
        let compressed = mmap_compress(file.path(), &config).unwrap();
        let cfile = write_temp_file(&compressed.data);
        let decompressed = mmap_decompress(cfile.path()).unwrap();
        assert_eq!(decompressed.data, b"");
    }
}
