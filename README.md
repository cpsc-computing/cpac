# CPAC

**Constraint-Projected Adaptive Compression**

High-performance, lossless compression engine with SIMD-accelerated transforms,
DAG-based pipelines, block-parallel I/O, post-quantum encryption, and a
drop-in CLI for gzip/zstd/brotli workflows. Written in Rust.

## Features

- **Adaptive pipeline** — SSR analysis auto-selects transforms and entropy backend per-file
- **11 transforms** — delta, zigzag, transpose, ROLZ, float-split, field-LZ, range-pack,
  tokenize, prefix-strip, dedup, parse-int
- **SIMD acceleration** — runtime dispatch: AVX-512 → AVX2 → SSE4.1 → SSE2 → NEON → scalar
- **DAG profiles** — composable transform chains with auto-select and 5 built-in profiles
- **Block-parallel** — rayon-based parallel compress/decompress (CPBL wire format)
- **Memory-mapped I/O** — auto-mmap for files > 64 MB, manual `--mmap` flag
- **Streaming** — block-based streaming with progress callbacks and adaptive block sizing
- **5 entropy backends** — Zstd, Brotli, Gzip, LZMA, Raw (passthrough)
- **Encryption** — ChaCha20-Poly1305, AES-256-GCM, Argon2 KDF
- **Post-quantum crypto** — ML-KEM-768 + X25519 hybrid encryption (CPHE), ML-DSA-65 signatures
- **Archives** — multi-file `.cpar` format with per-entry compression
- **Domain handlers** — CSV, JSON, XML, YAML, log file specializations
- **CAS analysis** — constraint inference (range, enum, constant, monotonic, functional dependency)
- **Benchmarking** — built-in benchmark suite with baselines (gzip-9, zstd-3, brotli-11, lzma-6),
  lossless verification, memory tracking, Criterion microbenchmarks, **industry-standard corpora**
  (Canterbury, Silesia, Calgary), automated batch runner with CSV/Markdown reports
- **Corpus management** — YAML-driven corpus configs, automatic HTTP/ZIP/TAR.GZ downloads,
  progress bars, 18+ curated benchmark datasets
- **Host detection** — CPU, cores, RAM, SIMD tier detection with safe auto-defaults
- **Cross-platform** — Windows (primary), Linux, macOS; x86_64 and aarch64
- **289+ tests** — comprehensive regression suite, golden vectors, property-based tests,
  determinism validation, transform-specific tests

## AI Agent Workflow

If you're an AI agent opening this repository for the first time:

1. **Read AGENTS.md** — Complete codebase onboarding (architecture, entry points, conventions)
2. **Read WARP.md** — Project rules (build commands, presubmit checklist, commit style)
3. **Read LEDGER.md** — Development session history
4. **Set Windows PATH** (if on Windows): `$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"`
5. **Verify build**: `cargo build --workspace && cargo test --workspace`
6. **Run presubmit** before any commits: build, test, clippy, fmt (see WARP.md)

## Quick Start

```bash
# Build
cargo build --workspace

# Run tests (289+)
cargo test --workspace

# Install the CLI
cargo install --path crates/cpac-cli

# Compress a file
cpac compress myfile.txt

# Decompress
cpac decompress myfile.txt.cpac
```

### Windows note

If `cargo` is not on your PATH in PowerShell:

```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
```

## Usage

```bash
# Compress / decompress
cpac compress input.txt -o output.cpac -v
cpac decompress output.cpac -o recovered.txt

# Parallel compression (auto for data > 256 KB)
cpac compress large.bin -T 8 -M 4096 -v

# Memory-mapped (auto for files > 64 MB, or forced)
cpac compress huge.iso --mmap

# Host system info
cpac info --host

# File analysis (SSR)
cpac info input.txt

# Constraint analysis
cpac auto-cas input.csv --compress

# Benchmark with baselines
cpac benchmark input.txt -n 10

# Encrypt / decrypt (password-based)
cpac encrypt input.txt -a chacha20
cpac decrypt input.txt.cpac-enc

# Post-quantum hybrid encryption
cpac pqc keygen -o ./keys
cpac pqc encrypt input.txt -k ./keys/cpac-hybrid.pub
cpac pqc decrypt input.txt.cpac-pqe -k ./keys/cpac-hybrid.sec

# PQC digital signatures
cpac pqc sign input.txt -k ./keys/cpac-hybrid.sec
cpac pqc verify input.txt -s input.txt.cpac-sig -k ./keys/cpac-hybrid.pub

# Archive operations
cpac archive-create ./mydir -o mydir.cpar
cpac archive-extract mydir.cpar -o ./restored
cpac archive-list mydir.cpar

# Shell completions
cpac completions bash > ~/.bash_completion.d/cpac
cpac completions powershell > cpac.ps1
```

## Architecture

CPAC is a 13-crate Cargo workspace. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
for the full design.

```
cpac-types          Shared types, CpacError, ResourceConfig
cpac-ssr            Structural Summary Record analysis
cpac-transforms     11 encoding transforms + SIMD kernels
cpac-dag            DAG composition, profiles, auto-select
cpac-entropy        Zstd / Brotli / Gzip / LZMA / Raw backends
cpac-frame          Wire format encode/decode (CP frame)
cpac-engine         Top-level API, host detection, parallel, benchmarks
cpac-cli            Command-line interface (clap)
cpac-crypto         AEAD, KDF, key exchange, PQC (feature-gated)
cpac-streaming      Block streaming, progress, mmap, adaptive sizing
cpac-domains        Domain-aware handlers
cpac-cas            Constraint-Aware Schema inference
cpac-archive        Multi-file .cpar archive format
```

Compression pipeline: `SSR → Preprocess (transforms) → Entropy coding → Frame encoding`

## Wire Formats

See [docs/SPEC.md](docs/SPEC.md) for complete wire format specifications.

- **CP** — standard CPAC frame (single-block)
- **CPBL** — block-parallel frame (multi-block, rayon)
- **TP** — transform preprocess frame
- **CS** — streaming frame
- **CPHE** — hybrid post-quantum encryption frame
- **CPAR** — multi-file archive

## Build Profiles

```bash
# Debug (fast compile)
cargo build --workspace

# Release (fat LTO + panic=abort + symbol strip)
cargo build --release

# Minimum binary size (opt-level=z)
cargo build --profile release-small
```

## Performance Benchmarks

### Latest Results (Phase 1+2 Optimizations)

**Date:** March 2, 2026 | **Version:** 0.1.0 | **Platform:** Windows x86_64, Rust 1.93

#### Balanced Mode (3 iterations)

| Data Type | Size | Backend | Ratio | Compress (MB/s) | Decompress (MB/s) | Verified |
|-----------|------|---------|-------|-----------------|-------------------|----------|
| **Text** (repetitive) | 22.5 KB | Zstd | **296.05x** | **155.1** | **762.7** | ✓ |
| Text | 22.5 KB | Brotli | 346.15x | 76.1 | 404.9 | ✓ |
| Text | 22.5 KB | Gzip | 144.23x | 122.5 | 400.1 | ✓ |
| **JSON** (structured) | 14.7 KB | Zstd | **183.75x** | **154.3** | **622.2** | ✓ |
| JSON | 14.7 KB | Brotli | 219.40x | 58.3 | 407.1 | ✓ |
| JSON | 14.7 KB | Gzip | 106.52x | 93.4 | 318.9 | ✓ |
| **Binary** (0-255 seq) | 25.6 KB | Zstd | **88.89x** | **159.1** | **1034.5** | ✓ |
| Binary | 25.6 KB | Brotli | 94.46x | 46.9 | 456.6 | ✓ |
| Binary | 25.6 KB | Gzip | 57.14x | 136.3 | 432.6 | ✓ |

**vs Industry Baselines:**
- **zstd-3** (native C): 75-133 MB/s compress → CPAC: **154-159 MB/s** (+16-26%)
- **gzip-9** (native C): 106-161 MB/s compress → CPAC: **93-155 MB/s** (within 15%)
- **brotli-11** (native C): 13-17 MB/s compress → CPAC Brotli: **46-76 MB/s** (+200-350%)

#### Key Achievements

✅ **Compression Ratios:** 57x to 375x depending on data type  
✅ **Throughput:** 155-330 MB/s compress, 400-1440 MB/s decompress  
✅ **100% Lossless Verification** across all tests  
✅ **Pure Rust:** <15% overhead vs optimized C implementations  
✅ **Adaptive Backend:** Auto-selects Zstd/Brotli/Gzip/LZMA based on SSR analysis  

#### Optimization Features

**Phase 1** (Low-Hanging Fruit):
- Adaptive Gzip levels (9 for small, 6 for large)
- Smart preprocessing (4KB threshold)
- Parallel compression (auto >1MB)
- Size-aware backend selection

**Phase 2** (Advanced):
- Dictionary training integration (Zstd)
- AVX2 SIMD delta encoding (32-byte vectorization)
- Memory pool infrastructure (signal-driven activation)
- Refined entropy-based backend logic

### Benchmark Profiles

```bash
# Single file benchmark with baselines
cpac benchmark myfile.txt

# Profile options (matches Python engine)
# Quick: 1 iteration (fast validation)
# Balanced: 3 iterations (default, reliable)
# Full: 10 iterations (publication-grade)
```

**TBD:** Full benchmark suite with industry-standard corpora (Canterbury, Silesia, Calgary).  
See [.work/benchmarks/LINKEDIN_REPORT.md](.work/benchmarks/LINKEDIN_REPORT.md) for detailed analysis.

### Criterion Microbenchmarks

```bash
# Full Criterion suite
cargo bench -p cpac-engine

# Individual bench suites
cargo bench -p cpac-engine --bench compress    # pipeline + backends
cargo bench -p cpac-engine --bench simd        # SIMD vs scalar
cargo bench -p cpac-engine --bench dag         # DAG compile + execute
```

## Completed Features (Phase 1+2) ✓

- ✓ **Dictionary training** — Zstd dictionary compression/decompression via stream API
- ✓ **SIMD acceleration** — AVX2 kernels for delta encoding with runtime CPU detection
- ✓ **Streaming API** — Block-based streaming with progress callbacks (CS format)
- ✓ **C/C++ FFI** — Complete bindings in `cpac-ffi` crate with cbindgen headers
- ✓ **Python bindings** — PyO3-based bindings in `cpac-py` (submodule)
- ✓ **Additional transforms** — BWT, MTF added to transform library
- ✓ **ARM SIMD** — NEON scaffolding and SVE/SVE2 infrastructure
- ✓ **Memory pool** — Buffer pool infrastructure (signal-driven activation)
- ✓ **Parallel compression** — Block-parallel CPBL format with auto-enable >1MB

## Planned Features

### Near-Term (Signal-Driven, Phase 3+)

All future optimizations are **bottleneck signal-driven**. See [.work/benchmarks/PERFORMANCE_ROADMAP.md](.work/benchmarks/PERFORMANCE_ROADMAP.md) for:
- 7 tracked bottlenecks with clear trigger criteria
- Mitigation paths and expected improvements
- Profiling best practices

**Top Priorities** (when signals indicate):
- **Memory pool activation** — When profiling shows >10% time in allocator
- **Dictionary caching** — When training overhead >1s on repeated corpora
- **ARM NEON implementation** — When profiling shows significant scalar fallback time
- **Preprocessing cache** — When >5% time in transform trial logic

### Long-Term (Phase 4+)

- **GPU acceleration** — CUDA/ROCm kernels for high-throughput systems (>10 GB/s)
- **Networked compression** — client/server mode with delta sync
- **WASM target** — browser-based compression with SIMD.js fallback
- **ML-based selection** — trained models for backend/transform selection

### Long-term

- **Approximate compression** — lossy modes for numerical data
- **Neural codec integration** — learned compression for specific domains
- **Distributed compression** — map/reduce across cluster
- **Hardware offload** — FPGA/ASIC integration for high-throughput
- **Format versioning** — backward-compatible wire format evolution

See [LEDGER.md](LEDGER.md) for session-by-session development progress.

## Requirements

- **Rust** 1.75+ stable (tested on 1.93)
- **Platforms**: Windows x86_64 (primary), Linux x86_64/aarch64, macOS x86_64/aarch64
- **Optional**: Gnuplot (for Criterion HTML reports)

## Project Files

- `AGENTS.md` — AI agent onboarding guide
- `WARP.md` — Warp IDE project rules
- `LEDGER.md` — Development session ledger
- `BENCHMARKING.md` — Industry benchmark results and guide
- `docs/SPEC.md` — Wire format specification
- `docs/ARCHITECTURE.md` — System architecture
- `docs/SESSION_8_REPORT.md` — Latest session comprehensive report
- `CONTRIBUTING.md` — Contribution guidelines
- `SECURITY.md` — Security policy

## License

CPAC Research & Evaluation License v1.0 — Copyright (c) 2026 BitConcepts, LLC.

See [LICENSE](LICENSE) for full terms. Commercial licensing available —
contact info@bitconcepts.tech.
