# CPAC Development Ledger

Session-by-session record of significant changes, investigations, and decisions.

## Session 31 — 2026-03-15 (Documentation Overhaul + Corpus Download Fixes)

### Focus
Comprehensive documentation overhaul: bring all 9 maintained docs up to date
with the current v0.3.0 codebase (21 crates, 12 backends, 26 transforms,
MSN disabled by default).  Also fix corpus download handlers and Windows
symlink enumeration in `cpac.py`.

### Documentation Updates (9 files)

1. **README.md** — Stripped all inline benchmark tables; added link to
   `docs/BENCHMARKING.md`.  Updated crate count 16→21, features (12 backends,
   26+ transforms, MSN opt-in).  Removed stale "Completed/Planned Features".

2. **docs/BENCHMARKING.md** — Removed Sessions 11–20 historical results; kept
   Session 21 as latest comprehensive benchmark.  Added Session 22–30
   infrastructure notes.  Full corpora table (17 corpora with sizes).  Updated
   download commands to `--profile full`.

3. **docs/ARCHITECTURE.md** — v0.3.0 header, accurate 21-crate tree with new
   crates (cpac-lab, cpac-conditioning, cpac-predict, cpac-transcode, sys
   crates), 12 backends in pipeline diagram.  Removed stale v0.2.0/v0.3.0
   roadmap.  Updated references section.

4. **docs/MANUAL.md** — v0.3.0 header.  Expanded §6 Entropy Backends from 5
   to 12 with full table.  Added MSN-disabled-by-default callout in §7.
   Updated `--backend` help text.

5. **docs/MSN_GUIDE.md** — Added prominent opt-in notice at top.  Updated
   version history: 19 domain handlers, u32 metadata, MSN off by default.

6. **docs/MSN_SPEC.md** — Fixed `msn_metadata_len` from u16 (2 bytes) to u32
   (4 bytes) matching code (`cpac-frame/src/lib.rs`).  CP2 minimum header now
   correctly documented as 16 bytes.  Added `FLAG_MSN_INLINE` (0x0001).
   Updated backend ID range 0x00–0x0B.

7. **docs/RELEASE.md** — v0.3.0 examples throughout.  Updated crate publish
   order from 15 to 21 crates (added sys crates, cpac-conditioning,
   cpac-predict, cpac-lab, cpac-transcode).

8. **docs/SPEC.md** — v1.1.  All 12 backend IDs (0x00–0x0B).  All 26
   transform IDs corrected to match `cpac-transforms` source (old SPEC had
   wrong ID assignments for Delta/ZigZag/Transpose/etc.).

9. **docs/ROADMAP.md** — Marked transcode Phase 1 (cpac-transcode crate) and
   auto-analysis Phase 1 (cpac-lab auto_analyze module) as done.

### Corpus Download Fixes (scripts/cpac.py)

- Fixed `http_targz` handler for multi-URL corpora (docker_layers)
- Fixed `http_gzip_multi` handler (github_events_large)
- Fixed `http_zip_nested` handler (loghub2_full)
- Added `_safe_rglob_files()` helper to skip Windows symlinks/reparse points
  (Alpine minirootfs untrusted reparse points caused WinError 448)
- Applied safe enumeration to benchmark file collection, download summary,
  and "already present" check

### Files Modified
- `README.md`
- `docs/ARCHITECTURE.md`
- `docs/BENCHMARKING.md`
- `docs/MANUAL.md`
- `docs/MSN_GUIDE.md`
- `docs/MSN_SPEC.md`
- `docs/RELEASE.md`
- `docs/ROADMAP.md`
- `docs/SPEC.md`
- `scripts/cpac.py`

---

## Session 30 — 2026-03-15 (Benchmark Infrastructure Fixes + YAML Parser Bug)

### Focus
Fix all benchmark timeouts, add large/very-large file tiering, exclude
pre-compressed files from corpora, investigate MSN mismatch on loghub2_2k,
and fix a YAML inline-comment parsing bug in `cpac.py`.

### A1: enwik8 Timeout Fix
enwik8 (100 MB) timed out at 3600 s on the balanced profile.  Added
`large_file_levels` support to `cpac.py` and `profile_balanced.yaml`:
- Files > 15 MB skip High level (use `[fast, default, best]`)
- Timeout raised from 3600 → 7200 s (balanced) and 600 → 14400 s (full)

### A2: NASA .gz Exclusion
Added `exclude_extensions: [".gz"]` to `corpus_nasa_logs.yaml` and
implemented `exclude_extensions` support in `cpac.py` file collection logic.
Pre-compressed `.gz` originals always benchmark at 1.00× and waste time.

### A3: NASA Raw Log Timeouts (~180 MB files)
Added `very_large_file_threshold_mb: 100` and
`very_large_file_levels: [fast, best]` tier in both balanced and full
profiles.  Files > 100 MB now skip Default/High levels entirely.

### A4: Full Profile Hardening
Updated `profile_full.yaml`: timeout 600 → 14400 s, added
`large_file_levels`, `very_large_file_levels`, `large_file_iterations: 3`.

### B1: MSN Loghub2_2k Mismatch Investigation
7/14 loghub log files show SSR ≠ MSN compression ratios.  Investigation
confirmed MSN genuinely improves ratio on structured logs (up to +10% on
Thunderbird).  MSN has value for logs despite 78% throughput penalty.
No code change — informational finding.

### C1: YAML Inline-Comment Parser Bug
`_parse_yaml_simple()` in `cpac.py` did not strip inline `#` comments from
values.  A value like `[".gz"]  # comment` failed the `endswith("]")`
check and was stored as a raw string instead of being parsed as a list.
This broke `exclude_extensions`, `cpac_levels`, `large_file_levels`, and
`very_large_file_levels` in all profiles.

**Fix**: Added bracket-aware inline comment stripping (lines 290–302 of
`cpac.py`).  Iterates through characters tracking bracket depth; when `#`
is found at depth 0 preceded by a space, the value is truncated before the
comment.

### D1: AGENTS.md — Benchmark Naming Convention
Added documentation: "full benchmark" = `--profile full`,
"balanced benchmark" = default profile, "quick benchmark" = `--profile quick`.

### Benchmark Results — Full Profile (776/776 OK)
Ran `benchmark-all --profile full` after all fixes.  776/776 files OK,
0 timeouts, 0 failures.  NASA corpus showed 4 files (2 raw logs + 2 .gz)
because the YAML parser bug prevented `.gz` exclusion — fixed by C1.

### Files Modified
- `AGENTS.md` — benchmark naming convention
- `benches/cpac/corpora/corpus_nasa_logs.yaml` — `exclude_extensions: [".gz"]`
- `benches/cpac/profiles/profile_balanced.yaml` — timeout, large/very-large tiers
- `benches/cpac/profiles/profile_full.yaml` — timeout, large/very-large tiers
- `scripts/cpac.py` — `exclude_extensions` support, large/very-large file
  level tiering, YAML inline-comment parser fix

### Validation
- Presubmit: `shell.ps1 presubmit` ✓
- Full benchmark: 776/776 OK, 0 timeouts ✓

---

## Session 29 — 2026-03-12 (v0.2.0 Release Readiness Sprint)

### Focus
Feature completion and release preparation for v0.2.0. New crates, CLI
subcommands, hardware detection, documentation updates, CI additions, and
version bump.

### A1: Remove publish-crates from release.yml
Deleted the `publish-crates` job — crates are not published to crates.io.

### B1: Inline Descriptor Compression
Extended `serialize_dag_descriptor()` / `deserialize_dag_descriptor()` in
`cpac-dag/src/dag.rs` to zstd-compress metadata that exceeds the u16 length
prefix. High bit of transform ID used as compression flag. Removed the u16
bail-out in `normalize.rs:317`. Added `zstd` dependency to `cpac-dag`.

### B2: Extended Default Baselines
Added `extended_baselines()` to `cpac-lab/src/bench.rs` returning zstd-12,
zstd-19, brotli-11 alongside matched baselines. Updated
`bench_directory_with_baselines` test assertion (46→52).

### C1-C4: Transcode Compression
New `cpac-transcode` crate with CPTC wire format for lossless image
compression: byte-plane split → delta encode → zstd. CLI `--transcode` flag
falls back to normal CPAC for non-image files. 10 tests.

### D1-D4: Closed-Loop Auto-Analysis
New `cpac-lab/src/auto_analyze.rs` with `auto_analyze()`, `format_report()`,
`generate_yaml_config()`. CLI subcommand `auto-analyze` (alias `aa`) with
`--output`, `--quick`, `--write-config` flags. Wired into `scripts/cpac.py`.

### E1-E4: Hardware Acceleration
Feature flags in `cpac-engine/Cargo.toml`: `accel-qat`, `accel-iaa`,
`accel-gpu`, `accel-sve2`. Runtime detection in `accel.rs` for QAT devices,
IAA CPUID, GPU runtime (CUDA/nvcuda), with `format_accelerators()` helper.
Documented in `docs/HARDWARE_ACCEL.md`.

### A2: README Benchmarks
Updated benchmark table with Phase 1–6 results (loghub2_2k 16.63×, nasa_logs
8.56×, silesia 4.30×). Updated feature list (12 backends, 539+ tests,
transcode, auto-analysis, hardware accel).

### A3: Python Bindings CI
Added `python-bindings` job to `.github/workflows/ci.yml`: Ubuntu, maturin
build, smoke-test import.

### F1: Version Bump
Workspace version 0.1.0→0.2.0 in `Cargo.toml`, `pyproject.toml`, README
badge and footer. `VERSION` constant uses `env!("CARGO_PKG_VERSION")`.

### F2: CHANGELOG.md
Added full v0.2.0 release notes covering all sessions 20–29.

### Files Modified
- `.github/workflows/release.yml` — removed publish-crates
- `.github/workflows/ci.yml` — python-bindings job
- `Cargo.toml` — workspace version, cpac-transcode member + deps, image dep
- `python/cpac-py/pyproject.toml` — version 0.2.0
- `README.md` — benchmarks, feature list, version badge
- `CHANGELOG.md` — v0.2.0 release notes
- `cpac-dag/src/dag.rs` — inline descriptor compression
- `cpac-dag/Cargo.toml` — zstd dep
- `cpac-transforms/src/normalize.rs` — removed u16 bail-out
- `cpac-lab/src/bench.rs` — extended_baselines()
- `cpac-lab/src/auto_analyze.rs` — new: auto-analysis engine
- `cpac-lab/src/lib.rs` — module registration
- `cpac-lab/Cargo.toml` — cpac-engine dep
- `cpac-transcode/src/lib.rs` — new: CPTC wire format
- `cpac-transcode/Cargo.toml` — new crate
- `cpac-engine/src/accel.rs` — runtime detection improvements
- `cpac-engine/Cargo.toml` — feature flags
- `cpac-cli/src/main.rs` — --transcode, auto-analyze subcommand
- `cpac-cli/Cargo.toml` — cpac-transcode dep
- `scripts/cpac.py` — auto-analyze subcommand
- `docs/HARDWARE_ACCEL.md` — new

### Validation
- Build: `shell.ps1 build` ✓
- Tests: 539+ pass (0 failures) ✓
- Clippy: `shell.ps1 clippy` (0 warnings) ✓

---

## Session 28 — 2026-03-12 (Security Fixes + CI Hardening)

### Focus
Resolve all Dependabot and CodeQL code-scanning alerts on the repository.

### Dependabot Alerts (3 moderate — ml-dsa)
All three ml-dsa vulnerabilities were already patched — the dependency was at
`0.1.0-rc.7`, newer than all three patched versions (rc.2, rc.4, rc.5).
Dependabot couldn't verify this because `Cargo.lock` was gitignored.

**Fix**: Removed `Cargo.lock` from `.gitignore` and committed the lockfile.
Dependabot can now resolve versions and auto-close the alerts.

Patched vulnerabilities:
- Timing side-channel in ML-DSA decomposition (patched in rc.2)
- UseHint off-by-two error when r0 equals zero (patched in rc.5)
- Signature verification accepts repeated hint indices (patched in rc.4)

### CodeQL Code-Scanning Alerts (9 findings — missing workflow permissions)
All 9 alerts were for missing `permissions` blocks in GitHub Actions workflows.

- `ci.yml` — Added top-level `permissions: contents: read` (least privilege)
- `release.yml` — Already had `permissions: contents: write` on develop

### Files Modified
- `.gitignore` — Removed `Cargo.lock` exclusion
- `Cargo.lock` — Committed to repository (5,140 lines)
- `.github/workflows/ci.yml` — Added permissions block

---

## Session 27 — 2026-03-12 (Benchmarking + Profile Tuning)

### Focus
Full corpus benchmarking of all 6 ratio-improvement phases.  Fix profile
timeouts on large Silesia files, run targeted retry, disk cleanup.

### Benchmark Results — Balanced Profile (773/777 OK)
Ran `benchmark-all` with `profile_balanced.yaml`.  4 Silesia files timed out
(nci, samba, webster, mozilla) at the default 900 s timeout.

Key corpus averages (best ratio per file):
- loghub2_2k: 16.63× (Brotli@11 most common best backend; Zstd Best 15.25×)
- nasa_logs: 8.56×
- canterbury: 5.84×
- silesia (excl. timed-out): 4.30×
- calgary: 4.03×
- enwik8: 3.75×
- cloud_configs: 3.63×
- kodak: 1.08× (near-incompressible images)

### Silesia Retry (12/12 OK)
Created `profile_silesia_retry.yaml` with `timeout: 3600` and
`large_file_threshold: 15 MB`.  All 12 Silesia files completed:
- nci: 20.68×
- samba: 5.74×
- webster: 4.94×
- mozilla: 3.83×

### Profile Changes
- `profile_balanced.yaml` — timeout 900 → 3600 s, large_file_threshold 50 → 15 MB
- `profile_silesia_retry.yaml` — new profile targeting Silesia corpus only

### Disk Cleanup
Removed `target/debug` (~33 GB freed, ~30.8 GB now free).

### Files Modified
- `benches/cpac/profiles/profile_balanced.yaml` — timeout + threshold
- `benches/cpac/profiles/profile_silesia_retry.yaml` — new

---

## Session 26 — 2026-03-11 (Phases 3–6: Dictionary, Conditioned BWT, Backend Selection, CAS Bridge)

### Focus
Implement remaining 4 phases of the Compression Ratio Improvement Plan in a
single session.  All 4 phases pass presubmit (build + test + clippy).

### Phase 3 — Auto-Dictionary for Parallel Blocks

1. **CPBL v3 wire format** — Extends v2 with a shared zstd dictionary:
   `dict_len(4B)` + `dict_data` in the CPBL header.  V1/v2 remain readable.

2. **Dictionary training** — `compress_parallel()` collects the first N blocks
   (min 3, max 8, max 64 KB total dict size) and trains a zstd dictionary via
   `cpac-dict`.  Dict is stored once in the CPBL header and applied to all
   blocks via `compress_with_dict()` / `decompress_with_dict()`.

3. **Dependency** — Added `cpac-dict` to `cpac-engine/Cargo.toml`.

### Phase 4 — Conditioned BWT Composition

1. **New transform** — `ConditionedBwtTransform` (ID = 26) in
   `cpac-transforms/src/conditioned_bwt.rs`.  Partitions input via
   `cpac_conditioning::partition()`, applies BWT + MTF + RLE0 per qualifying
   stream.  Reassembles with a length-prefixed partition table.

2. **Registry** — Registered in `TransformRegistry::with_builtins()` in
   `cpac-dag/src/registry.rs` (now 26 transforms total).

### Phase 5 — Per-Block Backend Selection

1. **Fix** — Replaced hardcoded `Track::Track2` with per-block track derived
   from `block_config.cached_ssr` in `compress_parallel()`.  Each block now
   runs `auto_select_backend()` using its own SSR analysis rather than a
   single file-level decision.

### Phase 6 — CAS Bridge for MSN Fields

1. **TypedColumns** — New struct `TypedColumns` + `MsnResult::typed_columns()`
   in `cpac-msn/src/lib.rs`.  Exposes MSN-extracted fields as typed columns
   (numeric, string, timestamp, boolean) for downstream CAS analysis.

2. **CAS constraint bridge** — `compress_parallel()` calls
   `typed_columns()` on MSN results, feeds columns into CAS constraint
   inference, and applies per-column transforms when the cost model accepts.

### Files Modified
- `cpac-engine/src/parallel.rs` — CPBL v3 dict, per-block backend, CAS bridge
- `cpac-engine/src/lib.rs` — dict-aware compress path wiring
- `cpac-engine/Cargo.toml` — `cpac-dict` dependency
- `cpac-transforms/src/conditioned_bwt.rs` — new: ConditionedBwtTransform
- `cpac-transforms/src/lib.rs` — module registration
- `cpac-dag/src/registry.rs` — registered transform ID 26
- `cpac-msn/src/lib.rs` — TypedColumns, typed_columns()

### Validation
- Build: `shell.ps1 build` ✓
- Tests: full workspace (all suites) ✓
- Clippy: `shell.ps1 clippy` (0 warnings) ✓

---

## Session 25 — 2026-03-11 (Phase 2: MSN Cross-Block Metadata Deduplication)

### Focus
Implement Phase 2 of the Compression Ratio Improvement Plan: store MSN
metadata once in the CPBL header instead of duplicating it in every parallel
block frame.

### Implementation

1. **Type system** — Added `msn_metadata_external: bool` to `CompressConfig`
   and `msn_applied: bool` to `CompressResult` in `cpac-types/src/lib.rs`.
   Updated all `CompressResult` construction sites (engine + streaming).

2. **Engine compress()** — When `msn_metadata_external=true` and MSN applies,
   the per-block frame is CP v1 (no inline metadata), `original_size` is set
   to the residual length, and `msn_applied=true` signals the caller.

3. **CPBL v2 wire format** — New format in `parallel.rs` adds:
   `shared_meta_len(4B)` after the v1 header, plus `block_flags(1B×N)` and
   `shared_metadata` between the block size table and payloads.  V1 emitted
   when no MSN metadata (backward compatible).

4. **Compress path** — MSN probe in `compress_parallel()` now sets
   `msn_metadata_external=true` on the block config, collects per-block
   `msn_applied` flags, and writes CPBL v2 with shared metadata.

5. **Decompress path** — `decompress_parallel()` accepts both v1 and v2.
   For v2, decodes shared metadata once, then reconstructs MSN-flagged
   blocks via `metadata.with_residual()` + `cpac_msn::reconstruct()`.

6. **Block size cap** — When MSN is enabled, block size is capped at
   `MAX_DOMAIN_EXTRACT_SIZE` (8 MB) so per-block MSN extraction stays
   within domain handler limits.  Probe sample also truncated.

### Files Modified
- `cpac-types/src/lib.rs` — New config/result fields
- `cpac-engine/src/lib.rs` — External MSN path in compress()
- `cpac-engine/src/parallel.rs` — CPBL v2 format, compress + decompress
- `cpac-streaming/src/lib.rs` — Updated CompressResult construction
- `cpac-engine/tests/phase2_msn_dedup.rs` — New: 4 roundtrip tests
  (JSON v2, YAML v1, binary v1, XML v2)

### Validation
- Build: `shell.ps1 build` ✓
- Tests: full workspace (all suites including new Phase 2 tests) ✓
- Clippy: `shell.ps1 clippy` (0 warnings) ✓

### Key Discovery
Adaptive block sizing could produce blocks larger than MSN domain handlers
accept (BLOCK_SIZE_LARGE=32 MB > MAX_DOMAIN_EXTRACT_SIZE=8 MB).  The MSN
extraction silently returned `not_applied` on oversized blocks, causing the
parallel path to emit CPBL v1 even when MSN would have succeeded.  Fixed by
capping block size at the domain limit when MSN is enabled.

---

## Session 24 — 2026-03-11 (Phase 1: Fix Parallel Smart Transform Roundtrip)

### Focus
Execute Phase 1 of the Compression Ratio Improvement Plan: enable smart
transforms (primarily BWT) on the parallel compression path.

### Investigation Findings

1. **Original bug no longer reproduces** — The "corrupted output" reported in
   Sessions 21/22 was caused by an earlier pipeline issue that has since been
   fixed by other session changes.  The `skip_expensive_transforms = true`
   guard in `compress_parallel()` prevented the bug from manifesting but also
   killed all ratio improvement from transforms.

2. **BWT roundtrips correctly at block sizes** — Tested BWT on 4 MB and 17 MB
   blocks (single-stream and parallel) with full roundtrip verification.
   BWT metadata is only 4 bytes (the original index), well within the u16
   DAG descriptor limit.

3. **Normalize u16 hypothesis (H4) confirmed but moot** — The normalize
   transform generates hundreds of KB to MB of metadata on large blocks
   (one diff per whitespace removal).  The u16 guard at normalize.rs:317
   correctly bails out, and the `smart_preprocess` cost check would also
   reject it because uncompressed metadata overhead exceeds savings.  A
   future phase can add inline descriptor compression to make normalize
   viable.

### Fix Applied
Removed `block_config.skip_expensive_transforms = true` from
`compress_parallel()` in `cpac-engine/src/parallel.rs`.  BWT now runs on
parallel sub-blocks where the analyzer recommends it (≥ 16 MB blocks,
ascii_ratio > 0.85, entropy < 5.5).

### Files Modified
- `cpac-engine/src/parallel.rs` — Removed skip_expensive_transforms override
- `cpac-engine/tests/phase1_bwt_parallel.rs` — New: 2 roundtrip tests at
  17 MB block size (plain text + JSON) verifying smart transforms work
- `docs/ROADMAP.md` — Updated known issues: marked parallel roundtrip as RESOLVED

### Validation
- Build: `shell.ps1 build` ✓
- Tests: full workspace (95 cpac-msn + 77 cpac-engine + all integration) ✓
- Clippy: `shell.ps1 clippy` (0 warnings) ✓
- Phase 1 investigation tests: 2 new tests pass at 17 MB block size ✓

### Expected Impact
+15–45% compression ratio on large text files (≥32 MB) that trigger the
parallel path.  Verified on synthetic test data; real-world corpus
benchmarks pending.

---

## Session 23 — 2026-03-11 (MSN Large-File Regression Fix + Ratio Improvement Plan)

### Focus
Fix the Silesia large-file MSN regression (double-copy on passthrough, XML O(N×K)
blowup, no size limits on domain extractors). Investigate compression ratio
improvement opportunities and non-Rust component impact.

### MSN Regression Fix (3 root causes)

1. **Double-copy on passthrough** — `MsnResult::passthrough(data)` cloned all
   data, then the engine's bypass path cloned again (2× wasted allocation for
   non-matching files). Fix: added `MsnResult::not_applied()` zero-copy sentinel.

2. **No size limits** — All 19 domain extractors ran `extract()` on arbitrarily
   large buffers. Fix: added `MSN_MAX_EXTRACT_SIZE` (16 MB) top-level guard,
   `MAX_DOMAIN_EXTRACT_SIZE` (8 MB) per-domain guard, XML-specific 2 MB guard.

3. **XML extraction O(N×tags)** — 4× `String::replace()` per tag on full string,
   then savings gate rejected the result (all work wasted). Fix: 2 MB size guard
   short-circuits before expensive work.

### Files Modified (19 files)
- `cpac-msn/src/lib.rs` — `not_applied()`, `MSN_MAX_EXTRACT_SIZE`, `MAX_DOMAIN_EXTRACT_SIZE`
- `cpac-engine/src/lib.rs` — replaced `passthrough(data)` with `not_applied()`
- `cpac-msn/src/domains/text/{xml,json,csv,yaml}.rs` — per-domain size guards
- `cpac-msn/src/domains/logs/{syslog,apache,http,java,json_log,bgl,healthapp,proxifier,hpc,w3c,openstack}.rs` — per-domain size guards
- `cpac-msn/src/domains/binary/avro.rs` — size guard + `CpacError` import fix
- `cpac-msn/tests/msgpack_plain_text.rs` — updated for `not_applied()` contract

### Ratio Improvement Plan Created
Formal 6-phase plan: "CPAC Compression Ratio Improvement Plan"
- Phase 1: Fix parallel smart transform roundtrip (P0, +15–45% on large text)
- Phase 2: MSN cross-block metadata deduplication (P1, +0.5–2%)
- Phase 3: Auto-dictionary for parallel blocks (P1, +3–8%)
- Phase 4: Conditioning + BWT composition (P2, +2–10% hypothesis)
- Phase 5: Per-block backend selection (P2, +1–5% on heterogeneous)
- Phase 6: CAS bridge for MSN fields (P3, +5–20% on structured data)

### Non-Rust Component Assessment
Identified 6 statically linked C/C++ entropy codecs (zstd, lz4, xz, lzham,
lizard, zlib-ng) + 2 pure Rust codecs (brotli, snappy). None are pipeline
bottlenecks — FFI overhead is negligible. Python (`cpac.py`) is build-only.
Actual bottlenecks are in pure Rust (smart_preprocess trials, BWT screening,
MSN string operations).

### Validation
- Build: `shell.ps1 build` ✓
- Tests: `cargo test -p cpac-msn` (95 pass) ✓
- Tests: `cargo test -p cpac-engine` (77 + all integration suites) ✓
- Clippy: `shell.ps1 clippy` (0 warnings) ✓

---

## Session 22 — 2026-03-10 (Bug Fix Planning + Session Save)

### Focus
Document the parallel + smart transforms roundtrip bug for handoff to a clean
session. Deep-dive into the compress/decompress parallel architecture to
formulate root cause hypotheses.

### Key Analysis

#### Architecture Trace
Traced the full parallel compress/decompress pipeline:
- `compress_parallel()` splits data into blocks, each block independently runs
  the full CPAC pipeline (SSR → MSN → smart transforms → entropy → frame)
- Each compressed block is a self-contained CPAC frame with its own DAG descriptor
- `decompress_parallel()` extracts blocks, decompresses each independently,
  concatenates results
- Individual transforms (BWT chain, normalize) roundtrip correctly even at 5MB

#### Root Cause Hypotheses (Ranked)
1. **H4 (HIGH)**: Normalize transform metadata overflow — on ~2.5MB text blocks,
   whitespace positions metadata could reach ~2MB, exceeding the per-step `u16`
   length prefix in DAG descriptor wire format. `smart_preprocess` checks total
   descriptor size but may not catch per-step overflow.
2. **H2 (MEDIUM)**: DAG descriptor serialization overflow/truncation at u16 boundary
3. **H5 (MEDIUM)**: Frame original_size vs post-transform size mismatch
4. **H1 (LOW)**: Block boundary splitting transform-sensitive patterns
5. **H3 (RULED OUT for test)**: MSN cached metadata — test uses default
   `enable_msn: false`

#### Investigation Plan
1. Capture exact error from failing test (size mismatch vs content mismatch)
2. Isolate which transform (normalize vs bwt_chain) causes the failure
3. Check `serialize_dag_descriptor` per-step metadata u16 handling
4. Check normalize metadata size on ~2.5MB text blocks
5. Fix root cause
6. Validate all tests pass + clippy clean

### Plan Created
Formal plan document created: "Fix Parallel + Smart Transforms Roundtrip Bug"
with full architecture trace, 5 hypotheses, 6 investigation steps, post-fix
benchmark plan, and all key file references with line numbers.

### No Code Changes
This session was analysis and documentation only.

---

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
