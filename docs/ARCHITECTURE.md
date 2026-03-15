# CPAC Architecture

**Version**: 0.3.0  
**Date**: March 2026  
**Status**: 21-crate workspace, 12 entropy backends, 26+ transforms, MSN disabled by default, CPBL v1/v2/v3 parallel formats

## Executive Summary

CPAC (Content-Preserving Adaptive Compression) is a multi-stage compression pipeline that combines:
- **SSR (Structural Summary Record)**: Lightweight heuristic analysis for track selection
- **MSN (Multi-Scale Normalization)**: Domain-specific semantic extraction for structured data (opt-in)
- **26+ Transforms**: BWT (SA-IS), delta, zigzag, transpose, ROLZ, normalize, conditioned-BWT, predict, byte-plane, and more
- **12 Entropy Backends**: Zstd, Brotli, Gzip, LZMA, XZ, LZ4, Snappy, LZHAM, Lizard, zlib-ng, OpenZL, Raw

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
│  (cpac-entropy) — 12 backends            │
│  • Zstd, Brotli, Gzip, LZMA, XZ         │
│  • LZ4, Snappy, LZHAM, Lizard           │
│  • zlib-ng, OpenZL, Raw                  │
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

## Crate Architecture (21 crates)

```
cpac/
├── crates/
│   ├── cpac-types/           # Shared types, traits, errors
│   ├── cpac-ssr/             # Structural Summary Record
│   ├── cpac-msn/             # Multi-Scale Normalization (19 domain handlers)
│   ├── cpac-transforms/      # 26+ transforms + SIMD kernels
│   ├── cpac-entropy/         # 12 entropy backends
│   ├── cpac-frame/           # Frame encoding/decoding
│   ├── cpac-engine/          # Main compression pipeline + parallel
│   ├── cpac-streaming/       # Streaming compression
│   ├── cpac-archive/         # Archive format (.cpar)
│   ├── cpac-crypto/          # Encryption/signing (AEAD + PQC)
│   ├── cpac-dag/             # Transform DAG composition + profiles
│   ├── cpac-dict/            # Dictionary training (Zstd)
│   ├── cpac-cas/             # Constraint-Aware Schema inference
│   ├── cpac-domains/         # Domain-specific logic
│   ├── cpac-lab/             # Benchmarking, calibration, auto-analysis
│   ├── cpac-conditioning/    # Data conditioning / partitioning
│   ├── cpac-predict/         # Prediction transforms
│   ├── cpac-transcode/       # Lossless image transcode (CPTC)
│   ├── cpac-lizard-sys/      # Lizard C library sys crate
│   ├── cpac-lzham-sys/       # LZHAM C library sys crate
│   ├── cpac-cli/             # Command-line interface (clap)
│   └── cpac-ffi/             # C/C++ FFI bindings
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

## References

- **SSR Implementation**: `crates/cpac-ssr/src/lib.rs`
- **Transform Inventory**: `docs/TRANSFORMS.md`
- **Wire Format Spec**: `docs/SPEC.md`
- **Benchmark Results**: `docs/BENCHMARKING.md`
- **Feature Roadmap**: `docs/ROADMAP.md`

## See Also

- `README.md` — Project overview
- `CONTRIBUTING.md` — Development guidelines
- `docs/BENCHMARKING.md` — Performance benchmarks
- `docs/MANUAL.md` — User manual and CLI reference
