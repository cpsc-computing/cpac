# CPAC Architecture

**Version**: 0.1.0 (Rust Implementation)  
**Date**: March 3, 2026  
**Status**: SSR + MSN fully implemented, CP2 frame format available

## Executive Summary

CPAC (Content-Preserving Adaptive Compression) is a multi-stage compression pipeline that combines:
- **SSR (Structural Summary Record)**: Lightweight heuristic analysis for track selection
- **MSN (Multi-Scale Normalization)**: Domain-specific semantic extraction for structured data
- **Generic Transforms**: Delta, zigzag, transpose, ROLZ
- **Entropy Backends**: Zstd, Brotli, Gzip, Lzma, Raw

## High-Level Pipeline

```
┌────────────────────────────────────────────────────────────┐
│                   CPAC Compression Pipeline                 │
└────────────────────────────────────────────────────────────┘

Input Data (bytes)
     │
     ▼
┌─────────────────┐
│  Stage 0: SSR   │  ◄─ Always runs first
│  Analysis       │     • Shannon entropy
│  (cpac-ssr)     │     • ASCII ratio  
└────────┬────────┘     • Simple domain hints
         │              • Viability score → Track selection
         ▼
    ┌────────┐
    │ Track? │
    └───┬────┘
        │
     ┌──┴───┐
     │      │
  Track 1   Track 2
  (v≥0.3)   (v<0.3)
  Structured Generic
     │      │
     │      └──────────────────────────┐
     ▼                                 │
┌─────────────────┐
│ Stage 1: MSN    │  ◄─ Track 1 only (optional)
│ Extraction      │     Domain-specific
│ (cpac-msn)      │     semantic extraction
│                 │     • JSON/JSONL field names
│                 │     • CSV headers
│                 │     • XML tags
│                 │     • Log patterns
│                 │     • Binary format keys
└────────┬────────┘
         │                             │
         ▼                             ▼
┌──────────────────────────────────────────┐
│  Stage 2: Generic Transforms             │
│  (cpac-transforms)                       │
│  • Delta encoding                        │
│  • Zigzag (varint)                       │
│  • Transpose (byte interleaving)         │
│  • ROLZ (reduced offset LZ)              │
└──────────────────┬───────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────┐
│  Stage 3: Entropy Coding                 │
│  (cpac-entropy)                          │
│  • Zstd (fast, 5-15x)                    │
│  • Brotli (max compression, 7-25x)       │
│  • Gzip (ubiquitous, 2-18x)              │
│  • Lzma (high ratio, 1.7-2.7x)           │
│  • Raw (passthrough, 1.0x)               │
└──────────────────┬───────────────────────┘
                   │
                   ▼
┌──────────────────────────────────────────┐
│  Stage 4: Frame Encoding                 │
│  (cpac-frame)                            │
│  • CP format (v1, legacy)                │
│  • CP2 format (v2, with MSN metadata)    │
│  • CPBL format (parallel blocks)         │
└──────────────────┬───────────────────────┘
                   │
                   ▼
            Compressed Output
```

## SSR + MSN: Non-Conflicting Design

### SSR (Structural Summary Record)
**Purpose**: Fast gatekeeper - decides if data has exploitable structure  
**When**: Always runs first (Stage 0), <1ms overhead  
**Cost**: O(n) single pass  
**Output**: `Track1` (structured) or `Track2` (generic)

```rust
pub struct SSRResult {
    pub entropy_estimate: f64,    // 0.0-8.0 bits/byte
    pub ascii_ratio: f64,          // 0.0-1.0
    pub data_size: usize,
    pub viability_score: f64,      // Weighted score
    pub track: Track,              // Track1 or Track2
    pub domain_hint: Option<DomainHint>,  // Json, Xml, Csv, etc.
}
```

**Viability Formula**:
```
viability = (1 - entropy/8.0) * 0.4  // Low entropy bonus
          + ascii_ratio * 0.4         // Text data bonus  
          + domain_bonus * 0.2        // Structure detected bonus

Track1 if viability >= 0.3, else Track2
```

### MSN (Multi-Scale Normalization)
**Purpose**: Deep semantic extraction - extracts repeated structures  
**When**: Only if SSR selects Track1 (conditional stage)  
**Cost**: O(n) with parsing overhead (format-specific)  
**Output**: Semantic fields + residual bytes

```rust
pub struct MsnResult {
    pub fields: HashMap<String, serde_json::Value>,  // Extracted semantic data
    pub residual: Vec<u8>,                            // Remaining bytes
    pub applied: bool,                                // Was MSN actually used?
    pub domain_id: Option<String>,                    // "text.json", "text.csv", etc.
    pub confidence: f64,                              // Detection confidence
}
```

**Key Insight**: SSR and MSN are **sequential, non-conflicting stages**:
1. SSR analyzes → determines Track
2. If Track1 → MSN extracts structure
3. If Track2 → MSN is skipped entirely

No overlap, no conflict. SSR is the cheap filter, MSN is the expensive extractor.

## Current Implementation Status

### ✅ Implemented in Rust
- **cpac-ssr**: SSR analysis (entropy, ASCII ratio, domain hints)
- **cpac-msn**: Multi-Scale Normalization with 10 domain handlers
  - Text: JSON, CSV, XML
  - Binary: MessagePack, CBOR, Protobuf
  - Logs: Syslog, Apache, JSON Log
  - Passthrough (Track 2)
- **cpac-transforms**: Delta, zigzag, transpose, ROLZ
- **cpac-entropy**: Zstd, Brotli, Gzip, Lzma, Raw backends
- **cpac-frame**: CP (v1), CP2 (v2 with MSN), and CPBL frame formats
- **cpac-engine**: Main compression pipeline with MSN integration
- **cpac-cli**: Command-line interface with --enable-msn flag

## Crate Architecture

```
cpac/
├─── crates/
│   ├─── cpac-types/          # Shared types, traits, errors
│   ├─── cpac-ssr/            # ✅ Structural Summary Record
│   ├─── cpac-msn/            # ✅ Multi-Scale Normalization
│   ├── cpac-transforms/     # ✅ Generic transforms (delta, zigzag, etc.)
│   ├── cpac-entropy/        # ✅ Entropy backends (Zstd, Brotli, etc.)
│   ├── cpac-frame/          # ✅ Frame encoding/decoding
│   ├── cpac-engine/         # ✅ Main compression pipeline
│   ├── cpac-streaming/      # ✅ Streaming compression
│   ├── cpac-archive/        # ✅ Archive format (.cpac)
│   ├── cpac-crypto/         # ✅ Encryption/signing
│   ├── cpac-dag/            # ✅ Transform DAG composition
│   ├── cpac-dict/           # ✅ Dictionary training
│   ├── cpac-cas/            # ✅ CAS-YAML modeling
│   ├── cpac-domains/        # ✅ Domain-specific logic
│   ├── cpac-cli/            # ✅ Command-line interface
│   └── cpac-ffi/            # ✅ C FFI bindings
```

## Frame Formats

### CP Format (Current)
```
┌────────┬─────────┬────────────────┬─────────────┐
│ Magic  │ Version │ Header         │ Payload     │
│ "CPAC" │ 0x01    │ (variable)     │ (variable)  │
│ 4 bytes│ 1 byte  │                │             │
└────────┴─────────┴────────────────┴─────────────┘

Header contains:
- Backend type (1 byte)
- Original size (varint)
- DAG descriptor length (varint)
- DAG descriptor (optional)
```

### CPBL Format (Parallel Blocks)
```
┌────────┬─────────┬────────────────┬─────────────┬─────┬─────────────┐
│ Magic  │ Version │ Header         │ Block 1     │ ... │ Block N     │
│ "CPBL" │ 0x01    │ (variable)     │ (variable)  │     │ (variable)  │
└────────┴─────────┴────────────────┴─────────────┴─────┴─────────────┘

Used for parallel compression of large files (>1MB)
```

### CP2 Format (Version 2 with MSN)
```
"CP" (2B) | version=2 (1B) | flags (2B) | backend_id (1B)
| original_size (4B LE) | dag_descriptor_len (2B LE) | msn_metadata_len (2B LE)
| dag_descriptor | msn_metadata | payload
```

Backward compatible with CP v1:
- decode_frame() auto-detects version
- v1 frames have empty msn_metadata
- v2 frames include serialized MsnResult when MSN was applied

MSN Metadata (JSON):
- Extracted semantic fields (HashMap<String, Value>)
- Domain ID for reconstruction
- Residual already in payload
```

## Performance Characteristics

### Without MSN (Current Rust)
| Data Type | CPAC Ratio | Why? |
|-----------|------------|------|
| Text (Canterbury) | 2.7-3.3x | Generic text compression |
| Logs (Apache) | 15-25x | High repetition, still good |
| Logs (Linux) | 12-21x | Structured but no extraction |
| XML (Silesia) | 6-12x | Treated as generic text |
| JSON | 2-5x | No structure extraction |

### With MSN (Python Implementation)
| Data Type | CPAC Ratio | Why? |
|-----------|------------|------|
| JSON (repetitive) | 50-219x | Field name extraction |
| CSV (structured) | 20-50x | Column structure extraction |
| XML (nested) | 15-30x | Tag name normalization |
| Logs (parsed) | 30-100x | Pattern extraction |
| Text (repetitive) | 100-346x | MSN finds structure |

**Gap Explanation**: MSN extracts repeated semantic patterns that generic compressors miss.

## Roadmap

### v0.1.0 (March 3, 2026)
- ✅ Complete Rust implementation with MSN
- ✅ SSR track selection
- ✅ MSN with 10 domain handlers (JSON, CSV, XML, logs, binary)
- ✅ CP2 frame format with MSN metadata
- ✅ Generic transforms
- ✅ 5 entropy backends
- ✅ Parallel compression (CPBL)
- ✅ CLI `--enable-msn` flag
- ✅ Benchmarking infrastructure

### v0.2.0 (Planned)
- [ ] MSN performance optimization
- [ ] Additional domain handlers (Parquet, Avro, etc.)
- [ ] Adaptive MSN confidence thresholds
- [ ] MSN benchmarks vs Python implementation

### v0.3.0 (Future)
- [ ] MSN enabled by default for Track1
- [ ] Production validation
- [ ] Fuzzing (100M+ iterations)
- [ ] Domain-specific compression tuning

### v1.0.0 (Full Parity)
- [ ] Feature-complete with Python
- [ ] Performance equal or better
- [ ] MSN standard feature
- [ ] Full documentation

## References

- **SSR Implementation**: `crates/cpac-ssr/src/lib.rs`
- **Python MSN**: `../cpac-engine-python/src/cpac/core/msn.py`
- **MSN Plan**: See Warp plan "MSN Integration Architecture & Rust Port Plan"
- **Benchmark Results**: `BENCHMARKING.md`

## See Also

- `CONTRIBUTING.md` - Development guidelines
- `BENCHMARKING.md` - Performance benchmarks
- `README.md` - Project overview
- `.github/BRANCH_RULESETS.md` - GitFlow workflow
