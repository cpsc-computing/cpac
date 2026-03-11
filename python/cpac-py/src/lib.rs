// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPSC-Research-Evaluation-1.0
//! Python bindings for CPAC compression engine.

#![allow(clippy::useless_conversion)]

use cpac_types::{Backend, CompressConfig};
use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;

/// Compress data using CPAC.
///
/// Args:
///     data (bytes): Data to compress
///     backend (str, optional): Backend to use ('zstd', 'brotli', 'gzip', 'lzma', 'raw'). Default: 'zstd'
///
/// Returns:
///     bytes: Compressed data
///
/// Raises:
///     ValueError: If compression fails
///
/// Example:
///     >>> import cpac
///     >>> compressed = cpac.compress(b"hello world", backend="zstd")
#[pyfunction]
#[pyo3(signature = (data, backend=None))]
fn compress(py: Python, data: &[u8], backend: Option<&str>) -> PyResult<Py<PyBytes>> {
    let backend_enum = match backend {
        Some("zstd") | None => Backend::Zstd,
        Some("brotli") => Backend::Brotli,
        Some("gzip") | Some("gz") => Backend::Gzip,
        Some("lzma") | Some("xz") => Backend::Lzma,
        Some("raw") => Backend::Raw,
        Some(other) => {
            return Err(PyValueError::new_err(format!(
                "unknown backend: {other} (available: zstd, brotli, gzip, lzma, raw)"
            )))
        }
    };

    let config = CompressConfig {
        backend: Some(backend_enum),
        ..Default::default()
    };

    let result = cpac_engine::compress(data, &config)
        .map_err(|e| PyValueError::new_err(format!("compression failed: {e}")))?;

    Ok(PyBytes::new_bound(py, &result.data).into())
}

/// Decompress CPAC-compressed data.
///
/// Args:
///     data (bytes): Compressed data
///
/// Returns:
///     bytes: Decompressed data
///
/// Raises:
///     ValueError: If decompression fails
///
/// Example:
///     >>> import cpac
///     >>> original = cpac.decompress(compressed)
#[pyfunction]
fn decompress(py: Python, data: &[u8]) -> PyResult<Py<PyBytes>> {
    let result = cpac_engine::decompress(data)
        .map_err(|e| PyValueError::new_err(format!("decompression failed: {e}")))?;

    Ok(PyBytes::new_bound(py, &result.data).into())
}

/// Streaming compressor for incremental compression.
///
/// Example:
///     >>> import cpac
///     >>> compressor = cpac.Compressor(backend="zstd")
///     >>> compressor.write(b"hello ")
///     >>> compressor.write(b"world")
///     >>> compressed = compressor.finish()
///
///     # With MSN (domain-aware semantic extraction):
///     >>> compressor = cpac.Compressor(backend="zstd", enable_msn=True)
#[pyclass]
struct Compressor {
    inner: Option<cpac_streaming::stream::StreamingCompressor>,
}

#[pymethods]
impl Compressor {
    /// Create a new compressor.
    ///
    /// Args:
    ///     backend (str, optional): Backend to use. Default: 'zstd'
    ///     block_size (int, optional): Block size in bytes. Default: 1048576 (1 MB)
    ///     max_buffer (int, optional): Max buffer size in bytes. Default: 67108864 (64 MB)
    ///     enable_msn (bool, optional): Enable MSN semantic extraction. Default: False
    ///     msn_confidence (float, optional): MSN confidence threshold 0.0-1.0. Default: 0.5
    #[new]
    #[pyo3(signature = (backend=None, block_size=None, max_buffer=None, enable_msn=false, msn_confidence=0.5))]
    fn new(
        backend: Option<&str>,
        block_size: Option<usize>,
        max_buffer: Option<usize>,
        enable_msn: bool,
        msn_confidence: f64,
    ) -> PyResult<Self> {
        let backend_enum = match backend {
            Some("zstd") | None => Backend::Zstd,
            Some("brotli") => Backend::Brotli,
            Some("gzip") | Some("gz") => Backend::Gzip,
            Some("lzma") | Some("xz") => Backend::Lzma,
            Some("raw") => Backend::Raw,
            Some(other) => return Err(PyValueError::new_err(format!("unknown backend: {other}"))),
        };

        let config = CompressConfig {
            backend: Some(backend_enum),
            enable_msn,
            msn_confidence,
            ..Default::default()
        };
        let msn_cfg = cpac_streaming::MsnConfig {
            enable: enable_msn,
            confidence_threshold: msn_confidence,
            ..Default::default()
        };
        let bs = block_size.unwrap_or(1 << 20);
        let mb = max_buffer.unwrap_or(64 << 20);
        let compressor = cpac_streaming::stream::StreamingCompressor::with_msn(config, msn_cfg, bs, mb)
            .map_err(|e| PyValueError::new_err(format!("compressor init failed: {e}")))?;

        Ok(Self {
            inner: Some(compressor),
        })
    }

    /// Write data to the compressor.
    ///
    /// Args:
    ///     data (bytes): Data to compress
    ///
    /// Returns:
    ///     int: Number of bytes written
    fn write(&mut self, data: &[u8]) -> PyResult<usize> {
        let comp = self
            .inner
            .as_mut()
            .ok_or_else(|| PyValueError::new_err("compressor already finalized"))?;

        comp.write(data)
            .map_err(|e| PyValueError::new_err(format!("write failed: {e}")))
    }

    /// Flush any buffered data.
    fn flush(&mut self) -> PyResult<()> {
        let comp = self
            .inner
            .as_mut()
            .ok_or_else(|| PyValueError::new_err("compressor already finalized"))?;

        comp.flush()
            .map_err(|e| PyValueError::new_err(format!("flush failed: {e}")))
    }

    /// Finalize compression and return compressed data.
    ///
    /// Returns:
    ///     bytes: Compressed data
    fn finish(&mut self, py: Python) -> PyResult<Py<PyBytes>> {
        let comp = self
            .inner
            .take()
            .ok_or_else(|| PyValueError::new_err("compressor already finalized"))?;

        let data = comp
            .finish()
            .map_err(|e| PyValueError::new_err(format!("finish failed: {e}")))?;

        Ok(PyBytes::new_bound(py, &data).into())
    }

    /// Reset the compressor to initial state.
    fn reset(&mut self) -> PyResult<()> {
        let comp = self
            .inner
            .as_mut()
            .ok_or_else(|| PyValueError::new_err("compressor already finalized"))?;

        comp.reset();
        Ok(())
    }
}

/// Streaming decompressor for incremental decompression.
///
/// Example:
///     >>> import cpac
///     >>> decompressor = cpac.Decompressor()
///     >>> decompressor.feed(compressed_data)
///     >>> output = decompressor.read_output()
#[pyclass]
struct Decompressor {
    inner: cpac_streaming::stream::StreamingDecompressor,
}

#[pymethods]
impl Decompressor {
    /// Create a new decompressor.
    ///
    /// Args:
    ///     max_buffer (int, optional): Max buffer size in bytes. Default: 16777216 (16 MB)
    #[new]
    #[pyo3(signature = (max_buffer=None))]
    fn new(max_buffer: Option<usize>) -> PyResult<Self> {
        let inner = if let Some(mb) = max_buffer {
            cpac_streaming::stream::StreamingDecompressor::with_max_buffer(mb)
        } else {
            cpac_streaming::stream::StreamingDecompressor::new()
        }
        .map_err(|e| PyValueError::new_err(format!("decompressor init failed: {e}")))?;

        Ok(Self { inner })
    }

    /// Feed compressed data to the decompressor.
    ///
    /// Args:
    ///     data (bytes): Compressed data
    fn feed(&mut self, data: &[u8]) -> PyResult<()> {
        self.inner
            .feed(data)
            .map_err(|e| PyValueError::new_err(format!("feed failed: {e}")))
    }

    /// Read decompressed output.
    ///
    /// Returns:
    ///     bytes: Decompressed data (may be empty if more input needed)
    fn read_output(&mut self, py: Python) -> PyResult<Py<PyBytes>> {
        let data = self.inner.read_output();
        Ok(PyBytes::new_bound(py, &data).into())
    }

    /// Check if decompression is complete.
    ///
    /// Returns:
    ///     bool: True if done
    fn is_done(&self) -> bool {
        self.inner.is_done()
    }

    /// Reset decompressor to initial state.
    fn reset(&mut self) {
        self.inner.reset();
    }
}

/// CPAC - Constraint-Projected Adaptive Compression
///
/// High-performance lossless compression with multiple backends.
#[pymodule]
fn cpac(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_function(wrap_pyfunction!(compress, m)?)?;
    m.add_function(wrap_pyfunction!(decompress, m)?)?;
    m.add_class::<Compressor>()?;
    m.add_class::<Decompressor>()?;
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    // Python tests will be in tests/test_cpac.py
}
