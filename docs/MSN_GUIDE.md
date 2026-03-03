# MSN User Guide

Multi-Scale Normalization (MSN) is an optional compression enhancement that extracts semantic structure from structured data formats (JSON, CSV, XML, logs, etc.) to achieve higher compression ratios.

## Table of Contents

- [Overview](#overview)
- [Quick Start](#quick-start)
- [When to Use MSN](#when-to-use-msn)
- [Domain Handlers](#domain-handlers)
- [CLI Usage](#cli-usage)
- [API Usage](#api-usage)
- [Performance Tuning](#performance-tuning)
- [Troubleshooting](#troubleshooting)

## Overview

MSN works by:
1. **Auto-detecting** the data format (JSON, CSV, XML, logs, etc.)
2. **Extracting** repeated semantic fields (column headers, field names, tags)
3. **Compressing** only the unique values (residual)
4. **Storing** lightweight metadata for reconstruction

**Example**: A 1MB JSON log file with 1000 records might have:
- Field names repeated 1000 times → Extract once
- Residual values → Compress with standard entropy coding
- Result: Better compression than entropy coding alone

## Quick Start

### Enable MSN (Simple)

```bash
# Auto-detect domain and use default confidence (0.5)
cpac compress --enable-msn data.json

# List available domains
cpac list-domains
```

### Enable MSN (Advanced)

```bash
# Higher confidence threshold (more selective)
cpac compress --enable-msn --msn-confidence 0.7 logs.txt

# Force specific domain (skip auto-detection)
cpac compress --enable-msn --msn-domain log.apache access.log

# Combined with other options
cpac compress --enable-msn --backend zstd -vv data.csv
```

## When to Use MSN

### ✅ Good Use Cases

| Data Type | Why MSN Helps | Example |
|-----------|---------------|---------|
| **JSON Logs** | Repeated field names across records | Application logs, API responses |
| **CSV Exports** | Column headers + tabular data | Database exports, metrics data |
| **XML Documents** | Repeated tags/attributes | Config files, RSS feeds |
| **Syslog** | Structured log format | System logs, application logs |
| **Apache Logs** | Common format with repeated patterns | Web server access logs |

**Ideal conditions**:
- Large files (>100KB)
- High repetition of structure
- Many records with same schema
- Track 1 data (SSR: structured/text)

### ❌ When NOT to Use MSN

| Data Type | Why MSN Doesn't Help | Alternative |
|-----------|----------------------|-------------|
| **Already compressed** | .gz, .zip, .7z files | Use `--backend raw` |
| **Binary executables** | No semantic structure | Standard compression |
| **Media files** | JPEG, PNG, MP4, FLAC | Skip compression entirely |
| **Small files** | <4KB | Metadata overhead too high |
| **Perfectly random** | Crypto output, /dev/urandom | Use `--backend raw` |

**Rule of thumb**: If standard compression (zstd/brotli) already achieves >20x ratio, MSN likely won't improve it further.

## Domain Handlers

MSN supports 11 domain handlers:

### Text Formats

| Domain ID | Format | Target Ratio | Example Use Case |
|-----------|--------|--------------|------------------|
| `text.json` | JSON objects | >50x | API responses, configs |
| `text.csv` | CSV/TSV | >20x | Database exports, metrics |
| `text.xml` | XML/HTML | >15x | Configs, RSS feeds, SOAP |
| `text.yaml` | YAML | >15x | Config files, K8s manifests |

### Binary Formats

| Domain ID | Format | Target Ratio | Example Use Case |
|-----------|--------|--------------|------------------|
| `binary.msgpack` | MessagePack | >30x | Binary JSON alternative |
| `binary.cbor` | CBOR | >30x | IoT data, COSE |
| `binary.protobuf` | Protocol Buffers | >40x | gRPC, service communication |

### Log Formats

| Domain ID | Format | Target Ratio | Example Use Case |
|-----------|--------|--------------|------------------|
| `log.syslog` | RFC 5424 Syslog | >20x | System logs |
| `log.apache` | Apache Common/Combined | >25x | Web server logs |
| `log.json` | JSON Lines (JSONL) | >50x | Structured app logs |

### Special

| Domain ID | Format | Target Ratio | Description |
|-----------|--------|--------------|-------------|
| `passthrough` | Any | 1x | No extraction (Track 2 fallback) |

## CLI Usage

### Basic Commands

```bash
# Compress with MSN
cpac compress --enable-msn input.json

# Decompress (MSN auto-detected from frame)
cpac decompress input.json.cpac

# List available domains
cpac list-domains

# Show file info (includes MSN domain hint)
cpac info input.json
```

### Advanced Options

#### Confidence Threshold

Controls auto-detection sensitivity (0.0-1.0):

```bash
# Default: 0.5 (balanced)
cpac compress --enable-msn input.json

# High confidence: 0.7+ (more selective, fewer false positives)
cpac compress --enable-msn --msn-confidence 0.8 mixed.txt

# Low confidence: 0.3 (more aggressive, may misdetect)
cpac compress --enable-msn --msn-confidence 0.3 noisy.data
```

**Recommendations**:
- 0.5 (default): Balanced, good for most use cases
- 0.7-0.9: Use when you want high certainty (lower false positive rate)
- 0.3-0.4: Experimental, may apply MSN to non-structured data

#### Force Domain

Skip auto-detection and force a specific handler:

```bash
# Force JSON domain (even if auto-detect says otherwise)
cpac compress --enable-msn --msn-domain text.json input.txt

# Force Apache log format
cpac compress --enable-msn --msn-domain log.apache access.log

# Useful when filename doesn't match content
cpac compress --enable-msn --msn-domain text.csv data.txt
```

#### Verbose Output

See MSN statistics during compression:

```bash
# Basic output
cpac compress --enable-msn -v input.json
#  output/input.json.cpac [15.2x]

# Detailed output
cpac compress --enable-msn -vv input.json
#  Original:   1048576 B
#  Compressed: 68912 B
#  Ratio:      15.21x (93.4% saved)
#  Track:      Track1
#  Backend:    Zstd

# Debug output
cpac compress --enable-msn -vvv input.json
#  (includes thread count, memory, mmap status)
```

### Batch Processing

```bash
# Compress all JSON files in directory
cpac compress --enable-msn --recursive --msn-domain text.json logs/

# Compress with progress bar
cpac compress --enable-msn *.csv
```

## API Usage

### Rust API

```rust
use cpac_engine::{compress, decompress, CompressConfig};

// Basic MSN compression
let data = std::fs::read("data.json")?;
let config = CompressConfig {
    enable_msn: true,
    ..Default::default()
};
let result = compress(&data, &config)?;
println!("Ratio: {:.2}x", result.ratio());

// With custom confidence
let config = CompressConfig {
    enable_msn: true,
    msn_confidence: 0.7,
    ..Default::default()
};

// Force specific domain
let config = CompressConfig {
    enable_msn: true,
    msn_domain: Some("text.csv".to_string()),
    ..Default::default()
};

// Decompression (MSN auto-detected from frame)
let decompressed = decompress(&result.data)?;
assert_eq!(decompressed.data, data);
```

### FFI/C API

MSN is automatically applied based on frame metadata during decompression. For compression, pass `enable_msn=true` in config (FFI support coming soon).

## Performance Tuning

### Optimization Guidelines

1. **Measure Before Enabling**
   ```bash
   # Test without MSN first
   cpac compress data.json
   # Then test with MSN
   cpac compress --enable-msn data.json
   # Compare ratios and decide
   ```

2. **Adjust Confidence Based on Data**
   - Clean, well-formatted data → Lower confidence (0.4-0.5)
   - Mixed or noisy data → Higher confidence (0.6-0.8)
   - Unknown data → Start with default (0.5)

3. **Use --force-domain for Known Formats**
   ```bash
   # Skip auto-detection for known formats
   cpac compress --enable-msn --msn-domain log.apache *.log
   ```

4. **Combine with Backend Selection**
   ```bash
   # MSN + Brotli for maximum text compression
   cpac compress --enable-msn --backend brotli text.json
   
   # MSN + Zstd for balanced speed/ratio
   cpac compress --enable-msn --backend zstd data.csv
   ```

### Performance Characteristics

| Operation | Overhead | Notes |
|-----------|----------|-------|
| **Track 2 (binary)** | < 0.2% | MSN skipped entirely |
| **Auto-detection** | ~1-2µs | Very fast, negligible |
| **Extraction (small)** | ~5-10µs | <10KB files |
| **Extraction (large)** | ~50-500µs | 100KB-10MB files |
| **Metadata overhead** | 50-500 bytes | Depends on # of fields |

**Key insight**: MSN overhead is amortized across file size. Larger files benefit more.

### Benchmarking

```bash
# Benchmark with and without MSN
cpac benchmark --quick data.json

# Then manually test
cpac compress data.json  # baseline
cpac compress --enable-msn data.json  # with MSN

# Compare results
```

## Troubleshooting

### MSN Not Improving Compression

**Symptoms**: With `--enable-msn`, file is same size or larger.

**Possible causes**:
1. **Data already highly compressible**: Standard entropy coding (zstd) already optimal
2. **Small file**: Metadata overhead dominates (try files >100KB)
3. **No repetitive structure**: Binary/random data won't benefit
4. **Wrong domain detected**: Try forcing domain with `--msn-domain`

**Solutions**:
- Check if SSR selected Track 2 (use `cpac info file`)
- Try without MSN if baseline compression is already >20x
- Use `--msn-domain` to force correct handler
- Increase `--msn-confidence` to be more selective

### Auto-Detection Choosing Wrong Domain

**Symptoms**: `cpac info` shows unexpected domain hint.

**Solutions**:
```bash
# Force correct domain
cpac compress --enable-msn --msn-domain text.json file.txt

# Adjust confidence threshold
cpac compress --enable-msn --msn-confidence 0.7 file.txt
```

### MSN Metadata Too Large

**Symptoms**: Compressed file larger than expected.

**Causes**:
- Many unique field names (100+)
- Large metadata per field
- Small data-to-metadata ratio

**Solutions**:
- Disable MSN for this data type
- Use standard compression instead
- Consider preprocessing to normalize field names

### Decompression Fails

**Symptoms**: Error during `cpac decompress`.

**Solutions**:
- Ensure file is valid CP2 frame (`cpac info file.cpac`)
- Check MSN version compatibility (v1 only currently)
- Verify file not corrupted (checksum mismatch)

## Best Practices

### ✅ Do

- Test MSN on representative sample before batch processing
- Use `cpac info` to understand data characteristics
- Combine MSN with appropriate backend (zstd/brotli)
- Set reasonable confidence thresholds (0.4-0.7 range)
- Force domain when format is known and consistent

### ❌ Don't

- Enable MSN for already-compressed data (.gz, .zip)
- Use MSN on tiny files (<4KB)
- Apply MSN to binary executables or media files
- Set confidence too low (< 0.3) without testing
- Expect MSN to help on perfectly random data

## Examples

### JSON API Logs

```bash
# Large JSON log file (10MB+)
cpac compress --enable-msn --msn-domain log.json api.log

# Expected: 15-50x ratio depending on repetition
```

### CSV Database Export

```bash
# CSV with consistent schema
cpac compress --enable-msn --msn-domain text.csv export.csv

# Expected: 10-30x ratio
```

### Apache Access Logs

```bash
# Standard Apache format
cpac compress --enable-msn --msn-domain log.apache access.log

# Expected: 15-40x ratio
```

### Mixed Content Directory

```bash
# Let auto-detection handle different formats
cpac compress --enable-msn --recursive data/

# MSN will apply selectively based on SSR+confidence
```

## Version History

### v1 (Current)
- Initial MSN implementation
- 11 domain handlers
- JSON-serialized metadata
- CP2 frame format

### Planned (v2)
- Compressed metadata section
- Additional domain handlers (Parquet, Avro)
- Streaming MSN support
- Dictionary learning across chunks
