# CPAC Wire Format Specification

Version: 1.1
Copyright (c) 2026 BitConcepts, LLC. All rights reserved.

Commercial licensing: info@bitconcepts.tech

All multi-byte integers are little-endian unless noted otherwise.

## 1. CP — Standard CPAC Frame

Magic: `"CP"` (0x43 0x50)

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       2     magic ("CP")
2       1     version (1)
3       2     flags (reserved, 0x0000)
5       1     backend_id
6       4     original_size (LE u32)
10      2     dag_descriptor_len (LE u16)
12      N     dag_descriptor (N = dag_descriptor_len)
12+N    ...   compressed_payload
```

Minimum header: 12 bytes.

### Backend IDs

- `0x00` — Raw (passthrough)
- `0x01` — Zstd
- `0x02` — Brotli
- `0x03` — Gzip (RFC 1952, deflate)
- `0x04` — LZMA (raw LZMA stream)
- `0x05` — XZ (LZMA2, XZ container)
- `0x06` — LZ4
- `0x07` — Snappy
- `0x08` — LZHAM
- `0x09` — Lizard
- `0x0A` — zlib-ng
- `0x0B` — OpenZL (delegates to Zstd)

### DAG Descriptor

When `dag_descriptor_len > 0`, the descriptor encodes the transform chain:

```
Offset  Size  Field
──────  ────  ─────────────────────
0       1     transform_count
1       N     transform_ids (1 byte each)
1+N     ...   per-transform: meta_len (LE u16) + meta_bytes
```

Transform IDs are defined in `cpac-dag/src/registry.rs`.

## 2. CPBL — Block-Parallel Frame

Magic: `"CPBL"` (0x43 0x50 0x42 0x4C)

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       4     magic ("CPBL")
4       1     version (1)
5       4     block_count (LE u32)
9       8     original_size (LE u64)
17      4×N   block_size_table (LE u32 per block)
17+4N   ...   block_payloads (concatenated)
```

Each block payload is a complete CP frame, independently decompressible.
Default block size: 1 MiB. Auto-engaged for inputs > 256 KiB.

## 3. TP — Transform Preprocess Frame

Magic: `"TP"` (0x54 0x50)

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       2     magic ("TP")
2       1     version (1)
3       1     transform_count
4       N     transform_ids (1 byte each)
4+N     ...   per-transform: param_len (LE u16) + params
...     ...   payload (transformed data)
```

### Transform IDs (TP frame)

CPAC supports 26 transforms with SIMD acceleration on select kernels:

- `0x01` — Transpose (params: element_width as LE u16)
- `0x02` — FloatSplit (params: none, self-framed)
- `0x03` — FieldLZ (params: none, self-framed)
- `0x04` — ROLZ (params: none, self-framed)
- `0x05` — Delta (params: stride as LE u16)
- `0x06` — ZigZag (params: none)
- `0x07` — RangePack (params: min, max as LE u64)
- `0x08` — Tokenize (params: none, self-framed)
- `0x09` — PrefixStrip (params: prefix_len as LE u16)
- `0x0A` — Dedup (params: none, self-framed)
- `0x0B` — ParseInt (params: none, self-framed)
- `0x0C` — BytePlane (params: none, self-framed)
- `0x0D` — Vocab (params: none, self-framed)
- `0x0E` — RLE (params: none, self-framed)
- `0x0F` — FloatXor (params: none, self-framed)
- `0x10` — RowSort (params: none, self-framed)
- `0x11` — Normalize (params: none, self-framed)
- `0x12` — BwtChain (SA-IS BWT, params: none, self-framed)
- `0x13` — ContextSplit (params: none, self-framed)
- `0x14` — ArithDecomp (params: none, self-framed)
- `0x15` — ConstElim (params: none, self-framed)
- `0x16` — StrideElim (params: none, self-framed)
- `0x17` — Condition (params: none, self-framed)
- `0x18` — Predict (params: none, self-framed)
- `0x19` — Projection (params: none, self-framed)
- `0x1A` — ConditionedBwt (params: none, self-framed)

SIMD runtime dispatch (best to worst): AVX-512 → AVX2 → SSE4.1 → SSE2 → NEON → scalar

Transforms are applied in order during compression and reversed during
decompression. If no TP magic is present, data is treated as raw.

## 4. CS — Streaming Frame

Magic: `"CS"` (0x43 0x53)

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       2     magic ("CS")
2       1     version (1)
3       4     num_blocks (LE u32)
7       8     original_size (LE u64)
15      4     block_size (LE u32)
19      ...   per-block: compressed_len (LE u32) + block_data
```

Each block is independently compressed via the standard CP pipeline.
Supports both sequential and parallel decompression.

## 5. CPHE — Hybrid Post-Quantum Encryption Frame

Magic: `"CPHE"` (0x43 0x50 0x48 0x45)

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       4     magic ("CPHE")
4       1     version (1)
5       32    ephemeral_x25519_public
37      2     mlkem_ciphertext_len (LE u16)
39      M     mlkem_ciphertext (M = mlkem_ciphertext_len)
39+M    1     aead_nonce_len
40+M    K     aead_nonce (K = aead_nonce_len)
40+M+K  ...   aead_ciphertext (ChaCha20-Poly1305)
```

Key derivation: HKDF-SHA256 over concatenation of X25519 shared secret
and ML-KEM-768 shared secret, with salt `"CPHE-hybrid-salt"` and info
`"CPHE-hybrid-v1"`.

## 6. CPAR — Multi-File Archive

Magic: `"CPAR"` (0x43 0x50 0x41 0x52)

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       4     magic ("CPAR")
4       1     version (1)
5       1     flags (reserved, 0x00)
6       4     num_entries (LE u32)
10      ...   entries (sequential)
```

Each entry:

```
Offset  Size  Field
──────  ────  ─────────────────────────────
0       2     path_len (LE u16)
2       P     path (UTF-8, forward slashes)
2+P     8     original_size (LE u64)
10+P    8     compressed_size (LE u64)
18+P    1     flags (reserved)
19+P    8     timestamp (Unix epoch seconds, LE u64)
27+P    C     compressed_data (C = compressed_size, CP frame)
```

## 7. Testing and Validation

CPAC includes comprehensive test infrastructure:

### Golden Vectors

Transform-specific golden vectors ensure deterministic behavior across:
- 11 transforms × 3 backends (Zstd, Brotli, Raw) = 33 golden vector sets
- Generated via `tests/generate_transform_goldens.rs`
- Stored in `tests/goldens/` directory

### Determinism Tests

Validation suite (`tests/determinism.rs`) ensures:
- Empty input handling
- Single-byte reproducibility
- All-zeros compression
- Random data consistency
- Cross-thread determinism
- Multi-iteration stability

### Industry Corpus Validation

CPAC is validated against industry-standard corpora:
- Canterbury Corpus (11 files, ~3 MB)
- Silesia Corpus (12 files, ~211 MB)
- Calgary Corpus (18 files, ~3 MB)
- Large Canterbury (6 files, ~10 MB)
- enwik8/enwik9 (Wikipedia text)
- Maximum Compression (44 files, ~400 MB)
- SQuash Corpus (17 files, ~34 GB)

Corpus management via YAML configs with automatic HTTP/ZIP/TAR.GZ downloads.

### Test Statistics

- **289+ total tests**: regression, golden vectors, property-based, determinism
- **33 transform golden vectors**: all transforms × all backends
- **6 determinism tests**: cross-thread, multi-iteration, edge cases
- **Zero test failures** maintained across all platforms

## 8. Performance Infrastructure

### SIMD Acceleration

Runtime CPU detection with fallback chain:
- x86_64: AVX-512 → AVX2 → SSE4.1 → SSE2 → scalar
- aarch64: NEON → scalar

SIMD kernels implemented for:
- Delta encode/decode
- ZigZag encode/decode
- Transpose (element-wise)

Additional kernels (NEON aarch64): `crates/cpac-transforms/src/simd/neon.rs`

### Profile-Guided Optimization (PGO)

Automated PGO build system (`scripts/pgo-build.ps1`):
1. Instrumented build with PGO flags
2. Training run on representative corpus
3. Optimized rebuild using profile data
4. 5-15% performance improvement typical

### Continuous Integration

Multi-platform CI/CD (`.github/workflows/ci.yml`):
- Platforms: Ubuntu (x86_64), Windows (x86_64), macOS (x86_64/aarch64)
- Tests: 289+ test suite, clippy (deny warnings), rustfmt
- Coverage: cargo-tarpaulin with codecov upload
- Matrix: stable/nightly Rust versions

### Corpus Download Infrastructure

Automatic corpus management (`crates/cpac-engine/src/corpus.rs`):
- HTTP downloads with progress bars (indicatif)
- ZIP/TAR.GZ extraction
- YAML corpus configuration (serde_yaml)
- Feature-gated (`download` feature, optional dependencies)
- 18+ curated benchmark datasets

## 9. Compression Backends

CPAC supports 12 entropy coding backends:

### Zstd (Backend ID: 0x01)

- Implementation: `zstd-safe` crate binding to libzstd
- Compression levels: 1-22 (default: 3)
- Features: dictionary compression, streaming, parallel threads
- Use case: general-purpose, fast decompression

### Brotli (Backend ID: 0x02)

- Implementation: `brotli` crate (pure Rust)
- Compression levels: 0-11 (default: 11)
- Features: quality/window size tuning, streaming
- Use case: maximum compression ratio, web content

### Gzip (Backend ID: 0x03)

- Implementation: `flate2` crate (deflate algorithm, RFC 1952)
- Compression levels: 0-9 (default: 6)
- Features: streaming, CRC32 checksums
- Use case: compatibility, widespread tool support, fast decompression

### LZMA (Backend ID: 0x04)

- Implementation: `xz2` crate (wraps liblzma)
- Compression presets: 0-9 (default: 6)
- Features: high compression ratios, dictionary-based, full level control
- Use case: maximum compression, archival, slow decompression acceptable

### XZ (Backend ID: 0x05)

- Implementation: `xz2` crate (wraps liblzma, XZ container format)
- Compression presets: 0-9 (default: 6 at all CPAC levels)
- Features: LZMA2 algorithm with XZ framing, CRC checksums
- Use case: archival, maximum compression with standardized container

### LZ4 (Backend ID: 0x06)

- Implementation: `lz4_flex` crate (pure Rust)
- Features: block-level compression with prepended size
- Use case: maximum throughput, real-time compression/decompression

### Snappy (Backend ID: 0x07)

- Implementation: `snap` crate (pure Rust)
- Features: single-speed compression, no level control
- Use case: lowest latency, hot-path data, real-time streaming

### LZHAM (Backend ID: 0x08)

- Implementation: `lzham` crate (FFI to C++)
- Compression levels: 0-4 (FASTEST to UBER)
- Features: high compression ratios, dictionary-based
- Use case: high-ratio archival, offline compression

### Lizard (Backend ID: 0x09)

- Implementation: vendored C source via `cc` crate
- Compression levels: 10-49 across 4 modes (fastLZ4, LIZv1, +Huffman variants)
- Features: LZ4-derivative with optional Huffman entropy coding
- Use case: tunable speed/ratio tradeoff

### zlib-ng (Backend ID: 0x0A)

- Implementation: `libz-ng-sys` crate (FFI to zlib-ng)
- Compression levels: 1-9
- Features: gzip-compatible output with optimized zlib-ng engine
- Use case: gzip-compatible compression with better performance

### OpenZL (Backend ID: 0x0B)

- Implementation: delegates to Zstd (future: OpenZL C++ DAG via FFI)
- Compression levels: same as Zstd (1-19)
- Features: CPAC datacenter compression initiative
- Use case: datacenter workloads, managed compression

### Raw (Backend ID: 0x00)

- Passthrough mode (no compression)
- Used for pre-compressed data or incompressible streams
- Zero overhead for already-compressed inputs
- Use case: transform-only pipelines, testing

All backends support:
- Single-threaded and parallel modes
- Memory-mapped I/O (files > 64 MB)
- Streaming with progress callbacks
- Compression/decompression verification

## 10. DAG Profiles

CPAC uses DAG (Directed Acyclic Graph) profiles to compose transform chains:

### Built-in Profiles

1. **Auto** — SSR-based auto-selection (default)
2. **Fast** — minimal transforms, optimize for speed
3. **Balanced** — moderate transform chain
4. **Max** — maximum compression, all applicable transforms
5. **Text** — text-optimized (tokenize, prefix-strip, dedup)

### DAG Composition

- Transform chains specified in `cpac-dag/src/profiles.rs`
- Auto-selection via SSR analysis (`cpac-ssr`)
- Runtime compilation and execution
- Transform metadata stored in DAG descriptor (CP frame offset 12+N)

## 11. Domain-Specific Handlers

CPAC includes specialized handlers for common data types:

- **CSV** — column-aware compression, header detection
- **JSON** — structure-aware, key deduplication
- **XML** — tag folding, attribute optimization
- **YAML** — indentation normalization
- **Logs** — timestamp extraction, pattern recognition

Handlers automatically engaged via file extension or SSR analysis.

## 12. Constraint-Aware Schema (CAS)

CAS infers data constraints for optimization:

- **Range constraints**: min/max value detection
- **Enum detection**: categorical value sets
- **Constant detection**: repeated values
- **Monotonic detection**: sorted sequences
- **Functional dependencies**: column correlations

Used by transform auto-selection and RangePack transform.

## Version History

- v1.1 (2026-03-15) — 26 transforms, 12 backends, corrected transform IDs to
  match `cpac-transforms` source, added Backend IDs 0x05–0x0B
- v1.0 (2026-03-02) — Initial comprehensive specification
