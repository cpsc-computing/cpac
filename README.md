# CPAC

**Constraint-Projected Adaptive Compression engine for Rust**

High-performance, lossless compression engine with SIMD-accelerated transforms,
DAG-based pipelines, block-parallel I/O, post-quantum encryption, and a
drop-in CLI for gzip/zstd/brotli workflows. Written in Rust.

[![Version](https://img.shields.io/badge/version-0.2.0-blue.svg)](https://github.com/cpsc-computing/cpac)
[![License](https://img.shields.io/badge/license-Research%20%26%20Evaluation-orange.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-blue.svg)](https://www.rust-lang.org)

> ⚠️ **Status:** Research & Evaluation | **License:** CPSC Research & Evaluation License v1.0  
> This repository is released for non-commercial research, evaluation, and educational purposes only. Commercial use requires a separate license. See LICENSE for full terms.

---

## Features

- **Adaptive pipeline** — SSR analysis auto-selects transforms and entropy backend per-file
- **11 transforms** — delta, zigzag, transpose, ROLZ, float-split, field-LZ, range-pack,
  tokenize, prefix-strip, dedup, parse-int
- **SIMD acceleration** — runtime dispatch: AVX-512 → AVX2 → SSE4.1 → SSE2 → NEON → scalar
- **DAG profiles** — composable transform chains with auto-select and 5 built-in profiles
- **Block-parallel** — rayon-based parallel compress/decompress (CPBL wire format)
- **Memory-mapped I/O** — auto-mmap for files > 64 MB, manual `--mmap` flag
- **Streaming** — block-based streaming with progress callbacks and adaptive block sizing
- **12 entropy backends** — Zstd, Brotli, Gzip, LZMA, XZ, LZ4, Snappy, LZHAM, Lizard, zlib-ng, OpenZL, Raw
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
- **Transcode compression** — lossless image (PNG/BMP/TIFF/WebP) pixel-domain compression
- **Auto-analysis** — directory-level analysis with YAML config generation
- **Hardware acceleration** — pluggable accel layer (QAT, IAA, GPU, FPGA, SVE2 stubs)
- **539+ tests** — comprehensive regression suite, golden vectors, property-based tests,
  determinism validation, transform-specific tests

## Agent Quick Start

This repository supports AI agent workflows with session commands.

When starting a new conversation with an AI agent (Warp, Claude, etc.) in this
repository, use the following prompt to establish context:

**Standard load session:**
```
You are in the cpac repository (CPAC compression engine). First read AGENTS.md
and WARP.md, then verify build with cargo build --workspace && cargo test --workspace.
```

On startup, an agent SHOULD:
1. Read `AGENTS.md` (architecture, entry points, conventions)
2. Read `WARP.md` (build commands, presubmit checklist, commit style)
3. Set Windows PATH if needed: `$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"`
4. Verify build: `cargo build --workspace && cargo test --workspace`
5. Run presubmit before any commits: build, test, clippy, fmt (see WARP.md)

For full agent conventions and session behavior, see `AGENTS.md`.

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

CPAC is a 16-crate Cargo workspace. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
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
cpac-msn            Multi-Scale Normalization (domain semantic extraction)
cpac-domains        Domain-aware handlers
cpac-cas            Constraint-Aware Schema inference
cpac-archive        Multi-file .cpar archive format
cpac-dict           Dictionary training (Zstd)
cpac-ffi            C/C++ FFI bindings
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

### Latest Results (Post Phase 1–6 Optimizations)

**Date:** March 2026 | **Version:** 0.2.0 | **Platform:** Windows x86_64, Rust 1.93+  
**Methodology:** 3-iteration balanced benchmark, extended baselines (zstd-3, zstd-12, zstd-19, brotli-11)

#### Headline Numbers

- **loghub2_2k:** 16.63× (MSN + smart transforms)
- **nasa_logs:** 8.56× (MSN + smart transforms)
- **silesia:** 4.30× (smart transforms)
- **canterbury:** 5.84× (smart transforms)
- **calgary:** 4.03× (smart transforms)
- **enwik8:** 3.75× (smart transforms)

#### Production Speed / Ratio Balance (zstd-3)

- OpenStack Cloud Logs: 708 MB/s @ 11.6×
- Linux System Logs: 497 MB/s @ 14.4×
- Apache Web Logs: 470 MB/s @ 15.9×
- HDFS Big Data Logs: 329 MB/s @ 5.3×
- Silesia dickens: 256 MB/s @ 2.8×

#### Key Achievements

- **16.63×** on loghub2_2k — MSN semantic extraction + BWT transforms
- **12 entropy backends** — Zstd, Brotli, Gzip, LZMA, XZ, LZ4, Snappy, LZHAM, Lizard, zlib-ng, OpenZL, Raw
- **100% lossless** — verified across all corpus measurements
- **Parallel + streaming** — auto-parallel >256 KB, streaming with bounded memory
- **Post-quantum encryption** — ML-KEM-768 + X25519 hybrid, ML-DSA-65 signatures

#### Optimization Phases

**Phases 1–2:** Adaptive gzip levels, smart preprocessing, parallel blocks, SIMD delta, dictionary training  
**Phases 3–4:** MSN field extraction, conditioned BWT, per-block backend selection  
**Phases 5–6:** Parallel transforms, MSN dedup, auto-dictionary, CAS bridge, max-ratio preset

### Benchmark Profiles

```bash
# Single file benchmark with baselines
cpac benchmark myfile.txt

# Profile options (matches Python engine)
# Quick: 1 iteration (fast validation)
# Balanced: 3 iterations (default, reliable)
# Full: 10 iterations (publication-grade)
```

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

All future optimizations are **bottleneck signal-driven**. See `BENCHMARKING.md` for the full corpus results.

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

## Requirements

- **Rust** 1.75+ stable (tested on 1.93)
- **Platforms**: Windows x86_64 (primary), Linux x86_64/aarch64, macOS x86_64/aarch64
- **Optional**: Gnuplot (for Criterion HTML reports)

## Project Files

- `AGENTS.md` — AI agent onboarding guide
- `WARP.md` — Warp IDE project rules
- `BENCHMARKING.md` — Industry benchmark results and guide
- `docs/SPEC.md` — Wire format specification
- `docs/ARCHITECTURE.md` — System architecture
- `CONTRIBUTING.md` — Contribution guidelines
- `SECURITY.md` — Security policy

## License

CPSC Research & Evaluation License v1.0 — Copyright (c) 2026 BitConcepts, LLC.

See [LICENSE](LICENSE) for full terms. Commercial licensing available —
contact info@bitconcepts.tech.

---

**CPAC v0.2.0** | © 2026 BitConcepts, LLC | Licensed under CPSC Research & Evaluation License v1.0
