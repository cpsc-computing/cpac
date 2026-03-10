# CPAC Roadmap

This document tracks planned features and architectural directions for the
CPAC compression engine. Items are grouped by theme; priority is indicated
where known.

## Transcode Compression (Compressed Media Re-Encoding)

### Problem

Many real-world datasets contain files that are **already compressed** in a
domain-specific format: JPEG/WebP images, H.264/H.265 video, AAC/Opus audio,
FLAC, etc. Generic compressors (including CPAC today) achieve near-1.0× ratio
on these because the entropy has already been reduced by the domain codec.
However, these domain codecs are often not optimal — a JPEG saved at quality 95
may be re-encodable at quality 95 in a more efficient codec (AVIF, JXL) with
significant size reduction, or the raw pixel/sample data itself may compress
better under CPAC's transform pipeline than the domain codec's residual.

### Approach

CPAC will support a **transcode compression** mode for compressed media:

1. **Detect compressed format.** SSR and MSN identify the domain codec via
   magic bytes and header parsing (JPEG SOI, PNG IHDR, RIFF/WAVE, MP4 ftyp,
   etc.).

2. **Decode to raw representation.** Decompress the media to its raw form:
   pixel buffer (images), PCM samples (audio), raw frames (video). This step
   uses well-known libraries (libjpeg-turbo, libpng, ffmpeg bindings, etc.)
   and is fully lossless for lossless formats (PNG, FLAC, lossless WebP).

3. **Compress raw data with CPAC.** Apply CPAC's full transform pipeline
   (byte-plane separation for image channels, delta coding for PCM, etc.)
   followed by the optimal entropy backend. This often achieves significantly
   better ratio than the original domain codec, especially on lossless formats.

4. **Store original codec metadata.** The CPAC frame stores enough metadata
   (codec ID, resolution, sample rate, bit depth, color space, quality level)
   to reconstruct the original domain-coded file on decompression.

5. **Restore on decompression.** When CPAC decompresses, it reconstructs the
   raw data, then re-encodes it into the original domain format using the
   stored metadata. The output is bit-identical to the original file for
   lossless formats. For lossy formats (JPEG, H.264), the restored file is
   perceptually identical — the same quality level and codec parameters are
   used, producing output that is either bit-identical (if the encoder is
   deterministic) or within the codec's own rounding tolerance.

### Phases

- **Phase 1 — Lossless image formats**: PNG, BMP, TIFF, lossless WebP.
  Decode to pixel buffer, compress with byte-plane + delta + zstd. Expected
  10–30% improvement over original PNG.
- **Phase 2 — Lossless audio**: FLAC, ALAC, WAV. Decode to PCM, compress
  with delta + float-split + zstd. Expected 5–15% improvement.
- **Phase 3 — Lossy image formats**: JPEG, WebP lossy. Transcode via DCT
  coefficient extraction (no re-quantisation) for bit-exact restoration.
- **Phase 4 — Video**: H.264/H.265 elementary streams. Frame-level chunking
  with motion-compensated delta coding. This is the most complex phase and
  may require FFmpeg integration.

### Constraints

- **Fully lossless round-trip** is mandatory for lossless source formats.
- For lossy formats, CPAC must restore files that are **functionally
  identical** — same visual/audio quality, same metadata, playable by standard
  decoders.
- Transcode mode is opt-in (flag or auto-detected) and can be disabled for
  files where the overhead is not justified.

## Closed-Loop Auto-Analysis System

### Problem

Today, CPAC requires manual experimentation to determine the best compression
strategy for a new data type: run `cpac profile`, examine gap analysis, tweak
MSN domains, recalibrate, repeat. For organisations with hundreds of data
formats flowing through their infrastructure, this is not scalable.

### Vision

A **fully automated closed-loop system** where the user drops in one or more
files, clicks "go", and receives:

1. A complete analysis of the data (structure detection, entropy profile,
   domain classification, redundancy map).
2. The best compression configuration discovered (backend, level, transforms,
   MSN settings).
3. A quantified comparison against all available codecs.
4. Actionable recommendations — including whether implementing new transforms
   or domain handlers would yield further gains.
5. If gains are possible, the system suggests or auto-generates new components.

### Architecture

```
  ┌────────────┐
  │  Drop Files │
  └─────┬──────┘
        ▼
  ┌───────────────────┐
  │  Analysis Engine   │  ← SSR + MSN + profiler + cross-file analysis
  │  (cpac-lab)        │
  └─────┬─────────────┘
        ▼
  ┌───────────────────┐
  │  Trial Matrix      │  ← Try all combinations: backends × levels × transforms
  │  (parallel)        │
  └─────┬─────────────┘
        ▼
  ┌───────────────────┐
  │  Gap Analysis      │  ← Compare best config vs default, vs external codecs
  └─────┬─────────────┘
        ▼
  ┌───────────────────┐
  │  Recommendation    │  ← "Enable MSN", "Add CSV domain", "Train dictionary"
  │  Engine            │
  └─────┬─────────────┘
        ▼
  ┌───────────────────┐
  │  Report + Config   │  ← Human-readable report + machine-readable YAML config
  └────────────────────┘
```

### Third-Party Module System

To enable extensibility without requiring changes to core CPAC:

- **Plugin trait**: A `CpacPlugin` trait that third-party crates can implement,
  providing custom transforms, domain detectors, or entropy backends.
- **Dynamic loading**: Plugins are shared libraries (`.so`/`.dll`) loaded at
  runtime from a configurable plugin directory (`~/.cpac/plugins/` or
  `$CPAC_PLUGIN_DIR`).
- **Plugin manifest**: Each plugin ships a `plugin.toml` declaring its
  capabilities (transforms, domains, backends), version compatibility, and
  resource requirements.
- **Sandboxing**: Plugins run in-process but with bounded memory and timeout
  constraints. A plugin that panics or exceeds limits is unloaded and the
  pipeline falls back to built-in behaviour.
- **Registration**: At startup, `cpac-dag`'s `TransformRegistry` and
  `cpac-msn`'s domain registry scan the plugin directory and register
  discovered capabilities alongside built-in ones.

### Phases

- **Phase 1 — CLI auto-analyze**: `cpac auto-analyze <dir>` runs the full
  pipeline and produces a Markdown report + YAML config. No plugins yet.
- **Phase 2 — Recommendation engine**: Suggestions for new MSN domains,
  dictionary training, transform chains. Includes "what-if" estimates.
- **Phase 3 — Plugin system**: `CpacPlugin` trait, dynamic loading,
  plugin manifest, registry integration.
- **Phase 4 — Continuous mode**: Watch a directory, re-analyse on new files,
  update recommendations. Suitable for integration into data pipelines.

## Managed Compression (Auto-Retraining)

Inspired by OpenZL's managed compression concept:

- Periodic re-calibration of transform gates and backend routing based on
  accumulated production data.
- A/B testing of new configurations with automatic rollback if ratio or
  throughput regresses.
- Integration with `cpac-lab` experiment infrastructure.
- Dashboard for monitoring compression KPIs over time.

## Known Issues (Critical)

### Smart Transform Roundtrip Failure on Parallel Path
**Status**: Identified, reproduction test exists, fix pending.

Smart transforms (`normalize` + `bwt_chain`) produce significantly better
compression ratios on large text data (up to +45% on silesia/nci) but fail
roundtrip verification when the file exceeds the 4 MiB parallel threshold.
The decompressed output has the correct size but corrupted content.

- Individual transforms roundtrip correctly at 100KB and 5MB (single-block).
- The failure occurs specifically in the `compress_parallel` → per-block
  smart transform → `decompress_parallel` path.
- Reproduction test: `roundtrip_smart_transforms_parallel_text` in
  `cpac-engine/src/lib.rs`.
- This is the #1 blocking issue for ratio improvement claims.

### `compress_parallel` Track Reporting
`compress_parallel()` hardcodes `track: Track::Track2` in `CompressResult`,
making benchmark labels misleading for large text files. Should report the
actual track from block-level SSR analysis.

## Additional Baseline Codecs

- **zstd-12 and zstd-19 baselines**: Add to `bench.rs` `BaselineEngine` to
  enable fair comparison against high-compression zstd settings. This is
  critical for validating that CPAC's preprocessing transforms provide value
  beyond what simply turning up the zstd level achieves.
- **xz/liblzma baseline**: Already available in benchmark-external; add to
  internal bench.rs.
- **snappy baseline**: Already available in benchmark-external; add to
  internal bench.rs.

## OpenZL Backend Integration

Validate and document the CPAC → OpenZL backend path:

- Ensure the Rust frontend can invoke OpenZL's C++ DAG via FFI when available.
- Benchmark CPAC preprocessing + OpenZL entropy vs CPAC preprocessing + zstd.
- Document configuration and deployment.

## Hardware Acceleration

- **Intel QAT**: Zstd offload for datacenter deployments.
- **Intel IAA**: In-line compression acceleration on Sapphire Rapids+.
- **GPU compute**: CUDA/Vulkan batch compression for bulk workloads.
- **ARM SVE2**: SIMD transform kernels for AArch64.

Stubs exist in `cpac-engine/src/accel.rs`; activation requires driver
detection and fallback logic.
