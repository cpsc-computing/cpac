# CPAC

**Constraint-Projected Adaptive Compression engine for Rust**

High-performance, lossless compression engine with SIMD-accelerated transforms,
DAG-based pipelines, block-parallel I/O, post-quantum encryption, and a
drop-in CLI for gzip/zstd/brotli workflows. Written in Rust.

[![Version](https://img.shields.io/badge/version-0.3.0-blue.svg)](https://github.com/cpsc-computing/cpac)
[![License](https://img.shields.io/badge/license-Research%20%26%20Evaluation-orange.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/rust-1.75%2B-blue.svg)](https://www.rust-lang.org)

> ⚠️ **Status:** Research & Evaluation | **License:** CPSC Research & Evaluation License v1.0  
> This repository is released for non-commercial research, evaluation, and educational purposes only. Commercial use requires a separate license. See LICENSE for full terms.

---

## Features

- **Adaptive pipeline** — SSR analysis auto-selects transforms and entropy backend per-file
- **26+ transforms** — BWT (SA-IS), delta, zigzag, transpose, ROLZ, float-split, field-LZ,
  range-pack, tokenize, prefix-strip, dedup, parse-int, normalize, conditioned-BWT, predict,
  byte-plane, const-elim, and more
- **SIMD acceleration** — runtime dispatch: AVX-512 → AVX2 → SSE4.1 → SSE2 → NEON → scalar
- **DAG profiles** — composable transform chains with auto-select and 5 built-in profiles
- **Block-parallel** — rayon-based parallel compress/decompress (CPBL v1/v2/v3 wire formats)
- **Memory-mapped I/O** — auto-mmap for files > 64 MB, manual `--mmap` flag
- **Streaming** — block-based streaming with progress callbacks and adaptive block sizing
- **12 entropy backends** — Zstd, Brotli, Gzip, LZMA, XZ, LZ4, Snappy, LZHAM, Lizard, zlib-ng, OpenZL, Raw
- **Encryption** — ChaCha20-Poly1305, AES-256-GCM, Argon2 KDF
- **Post-quantum crypto** — ML-KEM-768 + X25519 hybrid encryption (CPHE), ML-DSA-65 signatures
- **Archives** — multi-file `.cpar` format with per-entry compression
- **MSN domain handlers** — CSV, JSON, XML, YAML, syslog, Apache, and log file specializations (opt-in)
- **CAS analysis** — constraint inference (range, enum, constant, monotonic, functional dependency)
- **Transcode compression** — lossless image (PNG/BMP/TIFF/WebP) pixel-domain compression
- **Auto-analysis** — directory-level analysis with YAML config generation
- **Benchmarking** — profile-driven benchmark suite with 17+ curated corpora, 12 baseline
  backends, YAML-driven corpus configs, automatic HTTP/ZIP/TAR.GZ downloads
- **Host detection** — CPU, cores, RAM, SIMD tier detection with safe auto-defaults
- **Hardware acceleration** — pluggable accel layer (QAT, IAA, GPU, FPGA, SVE2 stubs)
- **Cross-platform** — Windows (primary), Linux, macOS; x86_64 and aarch64

## Quick Start

```bash
# Build
cargo build --workspace --release

# Run tests
cargo test --workspace

# Compress a file
cpac compress myfile.txt

# Decompress
cpac decompress myfile.txt.cpac
```

### Windows (PowerShell)

All commands go through the unified build system:

```powershell
.\shell.ps1 build --release
.\shell.ps1 test
.\shell.ps1 benchmark-all                  # balanced profile (default)
.\shell.ps1 benchmark-all --profile full   # full profile (17 corpora)
```

If `cargo` is not on your PATH:

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

CPAC is a 21-crate Cargo workspace. See [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md)
for the full design.

```
cpac-types          Shared types, CpacError, ResourceConfig
cpac-ssr            Structural Summary Record analysis
cpac-transforms     26+ encoding transforms + SIMD kernels
cpac-dag            DAG composition, profiles, auto-select
cpac-entropy        12 entropy backends (Zstd, Brotli, Gzip, LZMA, XZ, LZ4, ...)
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
cpac-lab            Benchmarking, calibration, auto-analysis
cpac-conditioning   Data conditioning / partitioning
cpac-predict        Prediction transforms
cpac-transcode      Lossless image transcode compression (CPTC)
cpac-lizard-sys     Lizard C library sys crate
cpac-lzham-sys      LZHAM C library sys crate
cpac-ffi            C/C++ FFI bindings
```

Compression pipeline: `SSR → Preprocess (transforms) → Entropy coding → Frame encoding`

## Wire Formats

See [docs/SPEC.md](docs/SPEC.md) for complete wire format specifications.

- **CP** — standard CPAC frame (single-block)
- **CPBL** — block-parallel frame (v1 basic, v2 shared MSN metadata, v3 auto-dictionary)
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

For full benchmark results, methodology, and corpus descriptions, see
**[docs/BENCHMARKING.md](docs/BENCHMARKING.md)**.

**Headline numbers** (Session 21, balanced profile, 8 corpora, 777 files):

- **loghub2_2k**: 16.63× average best ratio (log data)
- **nasa_logs**: 8.56× (HTTP access logs)
- **silesia**: 4.30× (mixed content)
- **enwik8**: 3.75× (Wikipedia XML)
- **776/776 files OK** on full profile (Session 30, 0 timeouts)
- **12 entropy backends**, 100% lossless verified

Benchmark profiles: quick (1 iter), balanced (3 iter), full (10 iter, 17 corpora).

## Compression Presets

| Preset | Level | Smart | Use Case |
|--------|-------|-------|----------|
| `turbo` | Fast | off | Maximum throughput, real-time pipelines |
| `balanced` | Default | on | General purpose, good ratio/speed balance |
| `maximum` | High | on | Best ratio with reasonable speed |
| `archive` | Best | on | Cold storage, archival workloads |

```bash
cpac compress --preset archive big_dataset.tar
cpac compress --preset turbo streaming_logs.jsonl
```

## Requirements

- **Rust** 1.75+ stable (tested on 1.93)
- **Platforms**: Windows x86_64 (primary), Linux x86_64/aarch64, macOS x86_64/aarch64
- **Optional**: Gnuplot (for Criterion HTML reports)

## Agent Quick Start

This repository supports AI agent workflows. When starting a new conversation
with an AI agent in this repository:

```
Read AGENTS.md and WARP.md, then verify build with .\shell.ps1 build && .\shell.ps1 test.
```

For full agent conventions and session behavior, see `AGENTS.md`.

## Documentation

- `AGENTS.md` — AI agent onboarding guide
- `WARP.md` — Warp IDE project rules
- `docs/BENCHMARKING.md` — Benchmark results and methodology
- `docs/ARCHITECTURE.md` — System architecture
- `docs/MANUAL.md` — User manual and CLI reference
- `docs/SPEC.md` — Wire format specification
- `docs/TRANSFORMS.md` — Transform pipeline status and calibration
- `docs/MSN_GUIDE.md` — Multi-Scale Normalization user guide
- `docs/ROADMAP.md` — Feature roadmap and known issues
- `docs/HARDWARE_ACCEL.md` — Hardware acceleration backends
- `docs/RELEASE.md` — Release process and CI/CD
- `CONTRIBUTING.md` — Contribution guidelines
- `SECURITY.md` — Security policy
- `CHANGELOG.md` — Release notes
- `LEDGER.md` — Session-by-session development record

## License

CPSC Research & Evaluation License v1.0 — Copyright (c) 2026 BitConcepts, LLC.

See [LICENSE](LICENSE) for full terms. Commercial licensing available —
contact info@bitconcepts.tech.

---

**CPAC v0.3.0** | © 2026 BitConcepts, LLC | Licensed under CPSC Research & Evaluation License v1.0
