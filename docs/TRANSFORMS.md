# CPAC Transform Status

Transform pipeline status as of 2026-03-08, based on calibration across
silesia, canterbury, calgary, enwik8, cloud, and log corpora (1,368+ files).

## Active (recommended by `recommend_from_ssr`)

| Transform | Domain | SSR Gate | Confidence | Notes |
|-----------|--------|----------|------------|-------|
| normalize | text | ascii_ratio > 0.80 | calibrated (~99%) | Tier 1. 99.9% win-rate on structured text. |
| bwt_chain | large text | ascii > 0.85, entropy < 5.5, 32 KB–64 MB | calibrated (~60%) | Tier 1. SA-IS O(n) BWT, 64 MiB cap. |
| predict | binary | ascii < 0.50, 1.0 < entropy < 6.0 | 0.55 | x-ray −20%, mr −8.7%, sao −2.2%. |
| byte_plane | binary | ascii < 0.50, entropy < 6.0 | 0.55 | 22.5% gain on x-ray; medical/sci binary. |
| const_elim | any | entropy < 1.0, size ≥ 64 | 0.95 | Rare (< 1%). Near-constant data only. |
| transpose | binary | ascii < 0.50, entropy < 7.0, size ≥ 256 | 0.45 | Bridges TP→smart. Adaptive trials only. |
| float_split | binary | ascii < 0.50, entropy < 6.5, size ≥ 128 | 0.45 | Bridges TP→smart. IEEE 754 split. |
| rolz | any | 3.5 < entropy < 6.5, size ≥ 512 | 0.40 | Bridges TP→smart. Local pattern LZ. |

## Dormant (registered but rarely/never selected)

| Transform | Confidence | Win Rate | Why dormant |
|-----------|------------|----------|-------------|
| stride_elim | 0.30 | 0% standalone | Below SMART_MIN_CONFIDENCE (0.50). Useful in CAS chains. |
| condition | N/A | 0% (1,368 files) | Removed from recommendations. Zero effect on any file. |
| context_split | N/A | 0% | Overhead always exceeds benefit on Serial data. |

## Never Recommended (Serial input)

These transforms are available for column-level use via CAS/DAG but are
**never** recommended on raw Serial byte streams:

- **delta** — always negative on raw bytes (−12.1M silesia, −6.3M enwik8)
- **rle** — zero gain on all corpora (zstd already handles runs)
- **arith_decomp** — IntColumn-only (errors on Serial input)

## Legacy TP Transforms

Used by the non-smart preprocessing path (`cpac_transforms::preprocess`).
Smart mode now bridges transpose/float_split/rolz into the SSR-gated
recommendation engine (see Active table above). The TP frame path remains
for direct binary preprocessing:

- **transpose** — detects record width, transposes row→column. Helps x-ray blocks.
- **float_split** — splits IEEE 754 mantissa/exponent. Helps float arrays.
- **field_lz** — repeating fixed-width field compression.
- **ROLZ** — reduced-offset LZ. Requires 8% savings for binary, 20% for text.

## Column-Level Transforms (CAS/DAG)

These are designed for typed columns, not raw bytes:

- **arith_decomp** — arithmetic decomposition for IntColumn
- **range_pack** — packs integer ranges
- **prefix** — common prefix extraction for StringColumn
- **row_sort** — row reordering for better column compressibility
- **dedup** — row deduplication
- **projection** — constraint projection (CPSC core)

## SIMD-Accelerated Kernels

Available in `cpac-transforms/src/simd.rs` with runtime dispatch:

- **delta_encode/decode** — AVX-512 → AVX2 → SSE2 → NEON → scalar
- **transpose_encode/decode** — AVX2 → scalar
- **zigzag_encode/decode** — AVX-512 → AVX2 → SSE4.1 → SSE2 → NEON → scalar
