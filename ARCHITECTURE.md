# CPAC Architecture

**Version**: 0.1.0 (Rust Implementation)  
**Date**: March 3, 2026  
**Status**: SSR implemented, MSN planned for v0.2.0

## Executive Summary

CPAC (Content-Preserving Adaptive Compression) is a multi-stage compression pipeline that combines:
- **SSR (Structural Summary Record)**: Lightweight heuristic analysis for track selection
- **MSN (Multi-Scale Normalization)**: Domain-specific semantic extraction (Python-only, planned for Rust)
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
┌─────────────────┐                   │
│ Stage 1: MSN    │  ◄─ FUTURE (v0.2.0)
│ Extraction      │     Domain-specific
│ (cpac-msn)      │     semantic extraction
│ PYTHON ONLY     │
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
│  • CP format (current)                   │
│  • CPBL format (parallel blocks)         │
│  • CP2 format (future with MSN metadata) │
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
// Proposed Rust design (not yet implemented)
pub struct MsnResult {
    pub fields: HashMap<String, Value>,  // Extracted semantic data
    pub residual: Vec<u8>,                // Remaining bytes
    pub applied: bool,                    // Was MSN actually used?
    pub domain_id: Option<String>,        // "text.json", "text.csv", etc.
    pub confidence: f64,                  // Detection confidence
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
- **cpac-transforms**: Delta, zigzag, transpose, ROLZ
- **cpac-entropy**: Zstd, Brotli, Gzip, Lzma, Raw backends
- **cpac-frame**: CP and CPBL frame formats
- **cpac-engine**: Main compression pipeline (without MSN)
- **cpac-cli**: Command-line interface

### ❌ Not Yet Implemented in Rust
- **cpac-msn**: Multi-Scale Normalization (Python-only)
- **Domain handlers**: JSON, CSV, XML, MessagePack, etc.
- **CP2 frame format**: Frame format with MSN metadata

## Crate Architecture

```
cpac/
├── crates/
│   ├── cpac-types/          # Shared types, traits, errors
│   ├── cpac-ssr/            # ✅ Structural Summary Record
│   ├── cpac-msn/            # ❌ Multi-Scale Normalization (FUTURE)
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

### CP2 Format (Future with MSN)
```
┌────────┬─────────┬────────────┬────────────┬─────────────┐
│ Magic  │ Version │ Header     │ MSN Fields │ Residual    │
│ "CPA2" │ 0x02    │ (variable) │ (optional) │ (variable)  │
└────────┴─────────┴────────────┴────────────┴─────────────┘

Header contains:
- Backend type
- Original size
- MSN applied flag
- Domain ID (if MSN used)
- MSN fields size

MSN Fields (MessagePack):
- Extracted semantic data
- Domain metadata for reconstruction
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

### v0.1.0 (Current)
- ✅ Complete Rust implementation without MSN
- ✅ SSR track selection
- ✅ Generic transforms
- ✅ 5 entropy backends
- ✅ Parallel compression (CPBL)
- ✅ Benchmarking infrastructure

### v0.2.0 (MSN Port)
- [ ] Create cpac-msn crate
- [ ] Port JSON, CSV, XML domain handlers
- [ ] Implement CP2 frame format
- [ ] CLI flag: `--enable-msn`
- [ ] Benchmarks: match Python ratios

### v0.3.0 (MSN Stable)
- [ ] Port all Python domain handlers
- [ ] MSN enabled by default for Track1
- [ ] Production validation
- [ ] Fuzzing (100M+ iterations)

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
