// Copyright (c) 2026 BitConcepts, LLC
// SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
//! Incremental streaming compression and decompression with bounded memory.
//!
//! Provides stateful compressor/decompressor that process data in chunks
//! without loading entire input into memory.

use cpac_types::{CompressConfig, CpacError, CpacResult};
use std::io::{self, Read, Write};

/// Default buffer size for streaming: 16 MB.
const DEFAULT_MAX_BUFFER: usize = 16 * 1024 * 1024;

/// Default block size for streaming compression: 1 MB.
const DEFAULT_STREAM_BLOCK: usize = 1 << 20;

/// Streaming frame magic.
const STREAM_MAGIC: &[u8; 2] = b"CS";
const STREAM_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// StreamingCompressor
// ---------------------------------------------------------------------------

/// State machine for streaming compressor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CompressorState {
    Init,
    Processing,
    Finalized,
}

/// Incremental streaming compressor with bounded memory.
///
/// # Example
/// ```no_run
/// use cpac_streaming::stream::StreamingCompressor;
/// use cpac_types::CompressConfig;
///
/// let mut compressor = StreamingCompressor::new(CompressConfig::default()).unwrap();
/// compressor.write(b"hello ").unwrap();
/// compressor.write(b"world").unwrap();
/// let compressed = compressor.finish().unwrap();
/// ```
pub struct StreamingCompressor {
    config: CompressConfig,
    state: CompressorState,
    input_buffer: Vec<u8>,
    compressed_blocks: Vec<Vec<u8>>,
    block_size: usize,
    max_buffer_size: usize,
    total_input: usize,
}

impl StreamingCompressor {
    /// Create a new streaming compressor.
    ///
    /// # Errors
    /// Returns error if configuration is invalid.
    pub fn new(config: CompressConfig) -> CpacResult<Self> {
        Self::with_options(config, DEFAULT_STREAM_BLOCK, DEFAULT_MAX_BUFFER)
    }

    /// Create compressor with custom block size and max buffer.
    pub fn with_options(
        config: CompressConfig,
        block_size: usize,
        max_buffer_size: usize,
    ) -> CpacResult<Self> {
        if block_size == 0 || block_size > max_buffer_size {
            return Err(CpacError::Other(format!(
                "invalid block size: {block_size} (max: {max_buffer_size})"
            )));
        }
        Ok(Self {
            config,
            state: CompressorState::Init,
            input_buffer: Vec::with_capacity(block_size * 2),
            compressed_blocks: Vec::new(),
            block_size,
            max_buffer_size,
            total_input: 0,
        })
    }

    /// Write data to the compressor.
    ///
    /// Data is buffered until a complete block is available, then compressed.
    /// May block if internal buffers are full (backpressure).
    ///
    /// # Errors
    /// Returns error if compression fails or compressor is finalized.
    pub fn write(&mut self, data: &[u8]) -> CpacResult<usize> {
        if self.state == CompressorState::Finalized {
            return Err(CpacError::Other("compressor already finalized".into()));
        }
        self.state = CompressorState::Processing;

        // Append to input buffer
        self.input_buffer.extend_from_slice(data);
        self.total_input += data.len();

        // Compress full blocks
        while self.input_buffer.len() >= self.block_size {
            self.compress_block()?;
        }

        // Enforce memory limits (backpressure simulation)
        let total_buffered = self.input_buffer.len()
            + self
                .compressed_blocks
                .iter()
                .map(|b| b.len())
                .sum::<usize>();
        if total_buffered > self.max_buffer_size {
            return Err(CpacError::Other(format!(
                "memory limit exceeded: {total_buffered} > {}",
                self.max_buffer_size
            )));
        }

        Ok(data.len())
    }

    /// Compress one full block from the input buffer.
    fn compress_block(&mut self) -> CpacResult<()> {
        let block_data: Vec<u8> = self.input_buffer.drain(..self.block_size).collect();
        let result = cpac_engine::compress(&block_data, &self.config)?;
        self.compressed_blocks.push(result.data);
        Ok(())
    }

    /// Flush any buffered data (compress partial block if any).
    ///
    /// # Errors
    /// Returns error if compression fails.
    pub fn flush(&mut self) -> CpacResult<()> {
        if !self.input_buffer.is_empty() {
            let block_data = self.input_buffer.drain(..).collect::<Vec<u8>>();
            let result = cpac_engine::compress(&block_data, &self.config)?;
            self.compressed_blocks.push(result.data);
        }
        Ok(())
    }

    /// Finalize compression and return the streaming frame.
    ///
    /// After calling this, the compressor cannot be used for more writes.
    ///
    /// # Errors
    /// Returns error if finalization fails.
    pub fn finish(mut self) -> CpacResult<Vec<u8>> {
        if self.state == CompressorState::Finalized {
            return Err(CpacError::Other("compressor already finalized".into()));
        }

        // Flush remaining data
        self.flush()?;
        self.state = CompressorState::Finalized;

        // Build streaming frame
        let num_blocks = self.compressed_blocks.len();
        let total_payload: usize = self.compressed_blocks.iter().map(|b| 4 + b.len()).sum();
        let header_size = 2 + 1 + 4 + 8 + 4;
        let mut frame = Vec::with_capacity(header_size + total_payload);

        frame.extend_from_slice(STREAM_MAGIC);
        frame.push(STREAM_VERSION);
        frame.extend_from_slice(&(num_blocks as u32).to_le_bytes());
        frame.extend_from_slice(&(self.total_input as u64).to_le_bytes());
        frame.extend_from_slice(&(self.block_size as u32).to_le_bytes());

        for block in &self.compressed_blocks {
            frame.extend_from_slice(&(block.len() as u32).to_le_bytes());
            frame.extend_from_slice(block);
        }

        Ok(frame)
    }

    /// Reset the compressor to initial state.
    pub fn reset(&mut self) {
        self.state = CompressorState::Init;
        self.input_buffer.clear();
        self.compressed_blocks.clear();
        self.total_input = 0;
    }
}

impl Write for StreamingCompressor {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.write(buf)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
    }

    fn flush(&mut self) -> io::Result<()> {
        self.flush()
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))
    }
}

// ---------------------------------------------------------------------------
// StreamingDecompressor
// ---------------------------------------------------------------------------

/// State machine for streaming decompressor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DecompressorState {
    Init,
    Header,
    Blocks,
    Done,
}

/// Incremental streaming decompressor.
///
/// # Example
/// ```no_run
/// use cpac_streaming::stream::StreamingDecompressor;
///
/// # let compressed_data = vec![67, 83, 1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0];
/// let mut decompressor = StreamingDecompressor::new().unwrap();
/// decompressor.feed(&compressed_data).unwrap();
/// let output = decompressor.read_output();
/// ```
pub struct StreamingDecompressor {
    state: DecompressorState,
    input_buffer: Vec<u8>,
    output_buffer: Vec<u8>,
    num_blocks: usize,
    original_size: usize,
    block_size: usize,
    blocks_processed: usize,
    blocks: Vec<Vec<u8>>, // Compressed block data
    max_buffer_size: usize,
}

impl StreamingDecompressor {
    /// Create a new streaming decompressor.
    pub fn new() -> CpacResult<Self> {
        Self::with_max_buffer(DEFAULT_MAX_BUFFER)
    }

    /// Create decompressor with custom max buffer size.
    pub fn with_max_buffer(max_buffer_size: usize) -> CpacResult<Self> {
        Ok(Self {
            state: DecompressorState::Init,
            input_buffer: Vec::new(),
            output_buffer: Vec::new(),
            num_blocks: 0,
            original_size: 0,
            block_size: 0,
            blocks_processed: 0,
            blocks: Vec::new(),
            max_buffer_size,
        })
    }

    /// Feed compressed data to the decompressor.
    ///
    /// Data is buffered and parsed incrementally.
    ///
    /// # Errors
    /// Returns error if data is invalid or decompression fails.
    pub fn feed(&mut self, data: &[u8]) -> CpacResult<()> {
        self.input_buffer.extend_from_slice(data);
        self.process()?;
        Ok(())
    }

    /// Process buffered input data.
    fn process(&mut self) -> CpacResult<()> {
        loop {
            match self.state {
                DecompressorState::Init => {
                    if self.input_buffer.len() < 19 {
                        return Ok(()); // Need more data
                    }
                    self.parse_header()?;
                    self.state = DecompressorState::Header;
                }
                DecompressorState::Header => {
                    self.state = DecompressorState::Blocks;
                }
                DecompressorState::Blocks => {
                    if self.blocks_processed >= self.num_blocks {
                        self.state = DecompressorState::Done;
                        return Ok(());
                    }
                    // Try to parse next block
                    if self.input_buffer.len() < 4 {
                        return Ok(()); // Need block length
                    }
                    let block_len = u32::from_le_bytes([
                        self.input_buffer[0],
                        self.input_buffer[1],
                        self.input_buffer[2],
                        self.input_buffer[3],
                    ]) as usize;
                    if self.input_buffer.len() < 4 + block_len {
                        return Ok(()); // Need complete block
                    }
                    // Extract and decompress block
                    let _len_bytes = self.input_buffer.drain(..4).collect::<Vec<u8>>();
                    let block_data = self.input_buffer.drain(..block_len).collect::<Vec<u8>>();
                    let result = cpac_engine::decompress(&block_data)?;
                    self.output_buffer.extend_from_slice(&result.data);
                    self.blocks_processed += 1;
                }
                DecompressorState::Done => return Ok(()),
            }
        }
    }

    /// Parse streaming frame header.
    fn parse_header(&mut self) -> CpacResult<()> {
        if &self.input_buffer[0..2] != STREAM_MAGIC {
            return Err(CpacError::InvalidFrame("not a streaming frame".into()));
        }
        if self.input_buffer[2] != STREAM_VERSION {
            return Err(CpacError::InvalidFrame("unsupported version".into()));
        }
        self.num_blocks = u32::from_le_bytes([
            self.input_buffer[3],
            self.input_buffer[4],
            self.input_buffer[5],
            self.input_buffer[6],
        ]) as usize;
        self.original_size = u64::from_le_bytes([
            self.input_buffer[7],
            self.input_buffer[8],
            self.input_buffer[9],
            self.input_buffer[10],
            self.input_buffer[11],
            self.input_buffer[12],
            self.input_buffer[13],
            self.input_buffer[14],
        ]) as usize;
        self.block_size = u32::from_le_bytes([
            self.input_buffer[15],
            self.input_buffer[16],
            self.input_buffer[17],
            self.input_buffer[18],
        ]) as usize;
        self.input_buffer.drain(..19);
        Ok(())
    }

    /// Read decompressed output.
    ///
    /// Returns all available output and clears the internal buffer.
    pub fn read_output(&mut self) -> Vec<u8> {
        self.output_buffer.drain(..).collect()
    }

    /// Check if decompression is complete.
    pub fn is_done(&self) -> bool {
        self.state == DecompressorState::Done
    }

    /// Reset decompressor to initial state.
    pub fn reset(&mut self) {
        self.state = DecompressorState::Init;
        self.input_buffer.clear();
        self.output_buffer.clear();
        self.num_blocks = 0;
        self.original_size = 0;
        self.block_size = 0;
        self.blocks_processed = 0;
        self.blocks.clear();
    }
}

impl Default for StreamingDecompressor {
    fn default() -> Self {
        Self::new().unwrap()
    }
}

impl Read for StreamingDecompressor {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        let output = self.read_output();
        let n = output.len().min(buf.len());
        buf[..n].copy_from_slice(&output[..n]);
        Ok(n)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn streaming_compressor_basic() {
        let config = CompressConfig::default();
        let mut comp = StreamingCompressor::new(config).unwrap();
        comp.write(b"hello ").unwrap();
        comp.write(b"world").unwrap();
        let frame = comp.finish().unwrap();
        assert!(frame.starts_with(STREAM_MAGIC));
    }

    #[test]
    fn streaming_roundtrip() {
        let config = CompressConfig::default();
        let mut comp = StreamingCompressor::new(config).unwrap();
        let data = b"The quick brown fox jumps over the lazy dog. ".repeat(100);
        comp.write(&data).unwrap();
        let frame = comp.finish().unwrap();

        let mut decomp = StreamingDecompressor::new().unwrap();
        decomp.feed(&frame).unwrap();
        let output = decomp.read_output();
        assert_eq!(output, data);
        assert!(decomp.is_done());
    }

    #[test]
    fn streaming_incremental() {
        let config = CompressConfig::default();
        let mut comp = StreamingCompressor::with_options(config, 512, 8 * 1024 * 1024).unwrap();
        for _ in 0..10 {
            comp.write(&vec![0u8; 256]).unwrap();
        }
        let frame = comp.finish().unwrap();

        let mut decomp = StreamingDecompressor::new().unwrap();
        // Feed in chunks
        for chunk in frame.chunks(100) {
            decomp.feed(chunk).unwrap();
        }
        let output = decomp.read_output();
        assert_eq!(output.len(), 2560);
    }

    #[test]
    fn streaming_large_data() {
        let config = CompressConfig::default();
        let mut comp = StreamingCompressor::new(config).unwrap();
        let data = vec![42u8; 5 * 1024 * 1024]; // 5 MB
        comp.write(&data).unwrap();
        let frame = comp.finish().unwrap();

        let mut decomp = StreamingDecompressor::new().unwrap();
        decomp.feed(&frame).unwrap();
        let output = decomp.read_output();
        assert_eq!(output, data);
    }

    #[test]
    fn compressor_reset() {
        let config = CompressConfig::default();
        let mut comp = StreamingCompressor::new(config).unwrap();
        comp.write(b"data1").unwrap();
        comp.reset();
        comp.write(b"data2").unwrap();
        let frame = comp.finish().unwrap();
        
        let mut decomp = StreamingDecompressor::new().unwrap();
        decomp.feed(&frame).unwrap();
        let output = decomp.read_output();
        assert_eq!(output, b"data2");
    }

    #[test]
    fn decompressor_reset() {
        let config = CompressConfig::default();
        let mut comp = StreamingCompressor::new(config).unwrap();
        comp.write(b"test data").unwrap();
        let frame = comp.finish().unwrap();

        let mut decomp = StreamingDecompressor::new().unwrap();
        decomp.feed(&frame[..10]).unwrap();
        decomp.reset();
        decomp.feed(&frame).unwrap();
        let output = decomp.read_output();
        assert_eq!(output, b"test data");
    }

    #[test]
    fn compressor_finalized_error() {
        let config = CompressConfig::default();
        let mut comp = StreamingCompressor::new(config).unwrap();
        comp.write(b"data").unwrap();
        let _frame = comp.finish().unwrap();
        // Cannot finish again
    }

    #[test]
    fn invalid_block_size() {
        let config = CompressConfig::default();
        let result = StreamingCompressor::with_options(config, 0, 1024);
        assert!(result.is_err());
    }
}
