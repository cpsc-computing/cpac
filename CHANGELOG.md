# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.2.0] - 2026-03-12

### Added

- **Transcode compression** (`cpac-transcode`): New crate with CPTC wire format for lossless image compression via byte-plane split + delta encoding + zstd. CLI `--transcode` flag.
- **Closed-loop auto-analysis** (`auto-analyze` / `aa`): New CLI subcommand analyzes files, reports optimal backends, and optionally writes a YAML config. Also wired into `scripts/cpac.py`.
- **Hardware acceleration probing**: Runtime detection for Intel QAT, Intel IAA, NVIDIA GPU (CUDA), and ARM SVE2. Feature flags: `accel-qat`, `accel-iaa`, `accel-gpu`, `accel-sve2`. See `docs/HARDWARE_ACCEL.md`.
- **Inline descriptor compression**: DAG descriptors with metadata exceeding the u16 limit are now zstd-compressed inline, removing the previous bail-out.
- **Extended default baselines**: `bench_directory` now includes zstd-12, zstd-19, and brotli-11 alongside matched baselines.
- **CPBL v2 wire format**: MSN metadata stored once in the header instead of per-block (cross-block deduplication).
- **CPBL v3 wire format**: Shared zstd dictionary trained from initial blocks, stored in the CPBL header.
- **ConditionedBwtTransform** (ID 26): Partitions input via conditioning, applies BWT+MTF+RLE0 per qualifying stream.
- **Per-block backend selection**: Each parallel block runs `auto_select_backend()` using its own SSR analysis.
- **CAS bridge for MSN fields**: `TypedColumns` exposes MSN-extracted fields for CAS constraint inference and per-column transforms.
- **Python bindings CI**: New `python-bindings` job in CI builds `cpac-py` with maturin and runs a smoke test.

### Changed

- Parallel smart transforms re-enabled (BWT now runs on parallel sub-blocks). +15–45% ratio on large text files.
- Balanced profile: timeout 900→3600s, large_file_threshold 50→15 MB.
- Removed `publish-crates` job from `release.yml` (crates are not published to crates.io).
- Updated README benchmarks with post-improvement results and feature list (12 backends, 539+ tests).

### Fixed

- Parallel smart transform roundtrip corruption (BWT + normalize interaction with DAG descriptors on large text).
- MSN large-file regression: eliminated double-copy on passthrough, added per-domain size guards (16 MB top-level, 8 MB per-domain, 2 MB XML), fixed XML O(N×tags) blowup.
- Parallel block size capped at `MAX_DOMAIN_EXTRACT_SIZE` (8 MB) when MSN is enabled to prevent silent extraction failures.
- Hardcoded `Track::Track2` in `compress_parallel()` replaced with per-block track from SSR analysis.

### Security

- Committed `Cargo.lock` to repository, enabling Dependabot to resolve ml-dsa dependency versions and close 3 moderate alerts.
- Added `permissions: contents: read` to `ci.yml`, resolving 9 CodeQL code-scanning alerts.

### Performance

Benchmark results (balanced profile, best ratio per file):
- loghub2_2k: **16.63×** — nasa_logs: **8.56×** — canterbury: **5.84×**
- silesia: **4.30×** (nci: 20.68×) — calgary: **4.03×** — enwik8: **3.75×**

## [0.1.0] - 2026-03-11

### Added

- CPAC compression engine with 25-crate workspace architecture.
- MSN (Multi-Strategy Normalization) structured data domains: JSON, XML, CSV, YAML, JSONL, MessagePack, CBOR, Protobuf, and log formats (syslog, Apache, structured).
- JSON columnar transform for arrays of objects (`extract_single_array` / `reconstruct_single_array`) achieving 59% improvement over Zstd on compact JSON data.
- Multiple compression backends: Zstd, Brotli, LZ4, Snappy, Gzip, LZMA, Lizard, LZHAM.
- SSR (Signal-Space Representation) transform with SIMD acceleration.
- CP2 frame format with MSN metadata support.
- CLI tool (`cpac-cli`) with `compress`, `decompress`, `benchmark`, and `analyze` commands.
- Python bindings (`cpac-py`) via PyO3 and maturin.
- C/C++ FFI library (`cpac-ffi`).
- Streaming API with S3 multipart upload support and GCS/Azure stubs.
- Content-addressable storage (CAS) module for deduplication.
- Archive support (tar, zip) with per-member compression.
- CPAC_TRACE diagnostic instrumentation for pipeline profiling.
- Dictionary training infrastructure.
- Normalize transform with XML whitespace mode.
- Corpus benchmark infrastructure with download scripts and profile presets.
- GitHub Actions CI/CD: cross-platform test matrix (Linux, macOS, Windows), clippy, rustfmt, coverage.
- GitHub Actions release automation: multi-platform binary builds, checksums, crates.io publishing.
- Fuzzing infrastructure for roundtrip, frame decode, and transform decode.
- Comprehensive documentation: SPEC, BENCHMARKING, ARCHITECTURE, MSN_GUIDE, RELEASE, DEV_PROFILING.

### Fixed

- JSON domain `detect()` O(N) hang on large files replaced with 32KB sample-based key analysis.
- XML domain detection using proper `<?xml` prefix and tag density scoring (confidence 0.85).
- JSONL `extract()` incorrectly succeeding on single-document JSON files.
- XML `extract()` returning error on non-UTF-8 data (now returns passthrough).
- Normalize descriptor bloat on structured data gated by 1% threshold.
- Pipeline overhead reduced from 5–9× to ±7% vs standalone backends (P6–P10 optimizations).
- Parallel threshold tuned to 32MB for text to keep structured files on single-block MSN path.
- MSN regressions on JSON/log/YAML false-positive domain detection.
- Binary domain false positives and CSV line ending handling.
- Clippy warnings resolved across all crates (zero warnings with `-D warnings`).

[unreleased]: https://github.com/cpsc-computing/cpac/compare/v0.2.0...HEAD
[0.2.0]: https://github.com/cpsc-computing/cpac/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/cpsc-computing/cpac/releases/tag/v0.1.0
