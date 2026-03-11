# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[unreleased]: https://github.com/cpsc-computing/cpac/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/cpsc-computing/cpac/releases/tag/v0.1.0
