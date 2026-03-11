# CPAC - Python Bindings

High-performance lossless compression for Python, powered by Rust.

## Installation

```bash
pip install cpac
```

## Quick Start

```python
import cpac

# Simple API
data = b"hello world" * 1000
compressed = cpac.compress(data, backend="zstd")
original = cpac.decompress(compressed)
assert data == original

# Try different backends
compressed_brotli = cpac.compress(data, backend="brotli")
compressed_gzip = cpac.compress(data, backend="gzip")
compressed_lzma = cpac.compress(data, backend="lzma")

# Streaming API for large files
compressor = cpac.Compressor(backend="zstd")
compressor.write(b"chunk 1")
compressor.write(b"chunk 2")
compressor.write(b"chunk 3")
compressed = compressor.finish()

# Streaming decompression
decompressor = cpac.Decompressor()
decompressor.feed(compressed)
output = decompressor.read_output()
```

## Backends

- **zstd** (default) - Fast, good compression ratio
- **brotli** - Best for text, slower
- **gzip** - Widely compatible, moderate compression
- **lzma** - Maximum compression, slowest
- **raw** - No compression (passthrough)

## API Reference

### Functions

#### `compress(data, backend='zstd') -> bytes`

Compress data using specified backend.

**Parameters:**
- `data` (bytes): Data to compress
- `backend` (str, optional): Backend name (default: 'zstd')

**Returns:** Compressed bytes

**Raises:** `ValueError` if compression fails

#### `decompress(data) -> bytes`

Decompress CPAC-compressed data.

**Parameters:**
- `data` (bytes): Compressed data

**Returns:** Original decompressed bytes

**Raises:** `ValueError` if decompression fails

### Classes

#### `Compressor(backend='zstd', block_size=None, max_buffer=None)`

Streaming compressor for incremental compression.

**Methods:**
- `write(data: bytes) -> int` - Write data to compress
- `flush()` - Flush buffered data
- `finish() -> bytes` - Finalize and return compressed data
- `reset()` - Reset to initial state

#### `Decompressor(max_buffer=None)`

Streaming decompressor for incremental decompression.

**Methods:**
- `feed(data: bytes)` - Feed compressed data
- `read_output() -> bytes` - Read decompressed output
- `is_done() -> bool` - Check if decompression complete
- `reset()` - Reset to initial state

## Performance

CPAC is built in Rust for maximum performance. Typical throughput:

- **Zstd**: 400-600 MB/s compress, 800-1200 MB/s decompress
- **Brotli**: 20-40 MB/s compress, 300-500 MB/s decompress
- **Gzip**: 80-120 MB/s compress, 200-400 MB/s decompress

## License

CPSC Research & Evaluation License v1.0

Copyright (c) 2026 BitConcepts, LLC. All rights reserved.

For commercial licensing: info@bitconcepts.tech
