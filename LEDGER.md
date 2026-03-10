# CPAC Development Ledger

Session-by-session record of significant changes, investigations, and decisions.

## Session 21 — 2026-03-10 (Transform Roundtrip Investigation)

### Focus
Investigate why CPAC's SSR/MSN/smart transforms are NOT producing better
compression ratios than standalone codecs in benchmarks.

### Key Findings

#### 1. Smart Transforms DO Improve Ratios — But Decompression Is Broken
The `bench_file` path (forced backend, `enable_smart_transforms: true`) shows
dramatically better ratios on large text files — but **fails roundtrip
verification**:

| File | CPAC (Zstd forced) | Standalone zstd-3 | Improvement | Verified |
|---|---|---|---|---|
| silesia/nci | 17.07x | 11.76x | +45% | NO |
| silesia/webster | 3.96x | 3.41x | +16% | NO |
| silesia/reymont | 3.92x | 3.40x | +15% | NO |
| silesia/dickens | 2.84x | 2.77x | +2.5% | NO |
| enwik8 | 2.85x | 2.81x | +1.4% | NO |

The smart transforms (primarily `bwt_chain` and `normalize`) produce excellent
forward compression but the reconstructed data doesn't match the original.
The decompress path runs (output is correct size) but content is corrupted.

#### 2. MSN IS Working on Log Files
The `bench_file_auto` path with MSN enabled shows verified ratio improvements
on structured log data:

| File | T1(SSR/Zstd) | T1(MSN/Zstd) | Improvement | Verified |
|---|---|---|---|---|
| Thunderbird_2k | 10.56x | 11.62x | +10.0% | YES |
| Spark_2k | 13.83x | 14.46x | +4.5% | YES |
| Hadoop_2k | 22.00x | 22.92x | +4.2% | YES |
| Mac_2k | 7.02x | 7.21x | +2.7% | YES |
| OpenStack_2k | 11.59x | 11.73x | +1.2% | YES |
| HealthApp_2k | 9.65x | 9.83x | +1.9% | YES |

#### 3. Parallel Path Interaction
The roundtrip bug manifests specifically when:
- File > 4 MiB (triggers `compress_parallel`)
- Smart transforms are enabled (default)
- Text data with ascii_ratio > 0.80 (triggers `normalize` + `bwt_chain`)

Individual transform roundtrip tests pass at 100KB and 5MB. The failure occurs
in the parallel compression path, likely due to DAG descriptor interaction
with block boundaries.

#### 4. `compress_parallel` Always Reports Track2
`compress_parallel()` hardcodes `track: Track::Track2` in its `CompressResult`,
regardless of actual block content. This means benchmark labels like
"T2(SSR/Zstd)" for large text files are misleading — the blocks may actually
be Track1.

### Tests Added
- `roundtrip_smart_transforms_large_text` — 50KB text, single-block, smart transforms
- `roundtrip_bwt_chain_direct_large` — 100KB BWT chain encode/decode
- `roundtrip_bwt_chain_direct_5mb` — 5MB BWT chain encode/decode
- `roundtrip_normalize_direct_large` — 100KB normalize encode/decode
- `roundtrip_smart_transforms_parallel_text` — 5MB+ text through parallel path (**FAILS** — reproduces the bug)

### Next Steps (Priority Order)
1. **Fix parallel + smart transforms roundtrip** — The parallel path's
   interaction with DAG descriptors is producing corrupt output on large text.
   This blocks all ratio improvement claims.
2. **Make production path (`bench_file_auto`) leverage transforms** — After fix,
   ensure the auto-route applies transforms that improve ratio.
3. **Re-benchmark** with fixed transforms to produce verified ratio wins.

### Files Modified
- `crates/cpac-engine/src/lib.rs` — Added 5 new roundtrip tests

---

## Session 20 — 2026-03-10 (Pipeline Validation + Calibration)

Full pipeline validation: 134+ tests passing, 0 errors, 0 warnings.
Completed: file reorganization, xz/snappy external benchmarks, benchmark
reporting rules, THESIS.md, ROADMAP.md, OpenZL feature parity, zstd-12/zstd-19
baselines, clippy fixes, calibration system, dictionary compression, preset
matrix (Turbo/Balanced/Maximum/Archive/MaxRatio).
