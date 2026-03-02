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
- **3 entropy backends** — Zstd, Brotli, Raw (passthrough)
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
cpac-entropy        Zstd / Brotli / Raw backends
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

## Benchmarks

### Industry-Standard Corpora

CPAC is validated against published compression benchmarks:

```bash
# Run automated benchmark suite
pwsh scripts/run-benchmarks.ps1 -Mode quick      # ~2 min, 5 files
pwsh scripts/run-benchmarks.ps1 -Mode balanced   # ~10 min, 13 files
pwsh scripts/run-benchmarks.ps1 -Mode full       # ~2-4 hours, all files

# Single file benchmark
cpac benchmark .work/benchdata/canterbury/alice29.txt --quick
cpac benchmark .work/benchdata/silesia/xml
```

**Results** (see [BENCHMARKING.md](BENCHMARKING.md) for full report):
- Canterbury alice29.txt: **2.93x** (CPAC Brotli) vs 2.80x (gzip-9) — **+4.6%**
- Canterbury kennedy.xls: **9.21x** (CPAC Zstd) vs 4.92x (gzip-9) — **+87%**
- Silesia XML: **12.42x** (brotli-11 max), **6.62x** (CPAC Brotli @ 25 MB/s)
- Beats gzip-9 on **5/5 Canterbury files**

### Criterion Microbenchmarks

```bash
# Full Criterion suite
cargo bench -p cpac-engine

# Individual bench suites
cargo bench -p cpac-engine --bench compress    # pipeline + backends
cargo bench -p cpac-engine --bench simd        # SIMD vs scalar
cargo bench -p cpac-engine --bench dag         # DAG compile + execute
```

## Planned Features

### Near-term (Phase 4+)

- **GPU acceleration** — CUDA/ROCm kernels for transforms and entropy coding
- **Dictionary training** — Zstd dictionary generation and management
- **Streaming API** — incremental compress/decompress with bounded memory
- **Networked compression** — client/server mode with delta sync
- **Additional transforms** — BWT, MTF, context modeling, LZ77 variants
- **ARM SVE/SVE2** — scalable vector extensions for aarch64
- **WASM target** — browser-based compression with SIMD.js fallback
- **C/C++ FFI** — drop-in replacement library for zlib/lz4/zstd
- **Python bindings** — PyO3-based cpac-py package

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
