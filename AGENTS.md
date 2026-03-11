# CPAC Agent Guide

This is the CPAC compression engine repository. Read this file first when
onboarding to the codebase. See also `WARP.md` for Warp-specific project rules.

## Workspace Overview

16-crate Cargo workspace under `crates/`. No circular dependencies.

**Core pipeline** (in compression order):
- `cpac-types` — `CpacError`, `CompressConfig`, `ResourceConfig`, shared enums
- `cpac-ssr` — SSR analysis: entropy, ASCII ratio, track selection, domain hints
- `cpac-transforms` — 11 transforms + SIMD kernels (`simd.rs`), preprocess orchestrator
- `cpac-dag` — `TransformDAG`, `TransformRegistry`, `ProfileCache`, DAG serialization
- `cpac-entropy` — Zstd/Brotli/Raw backends, auto-select by entropy
- `cpac-frame` — CP wire format encode/decode (12-byte header)
- `cpac-engine` — `compress()`/`decompress()`, `host.rs`, `parallel.rs`, `bench.rs`

**CLI:**
- `cpac-cli` — clap-based CLI, `config.rs` for TOML config, all subcommands

**Extensions:**
- `cpac-crypto` — AEAD, KDF, key exchange, `pqc` feature (ML-KEM-768, ML-DSA-65), `hybrid.rs`
- `cpac-streaming` — CS streaming frame, progress, `mmap.rs` (memmap2), adaptive block sizing
- `cpac-msn` — Multi-Scale Normalization: domain detection + semantic field extraction (JSON/CSV/XML/YAML/logs/binary)
- `cpac-domains` — CSV/JSON/XML/YAML/log domain handlers
- `cpac-cas` — constraint inference, DoF extraction, cost model
- `cpac-archive` — CPAR multi-file archive format
- `cpac-dict` — Zstd dictionary training
- `cpac-ffi` — C/C++ FFI bindings with cbindgen headers

## Key Entry Points

- **Compress/decompress API**: `cpac-engine/src/lib.rs` — `compress()`, `decompress()`
- **Parallel API**: `cpac-engine/src/parallel.rs` — `compress_parallel()`, `decompress_parallel()`
- **CLI main**: `cpac-cli/src/main.rs`
- **Host detection**: `cpac-engine/src/host.rs` — `detect_host()`, `auto_resource_config()`
- **SIMD dispatch**: `cpac-transforms/src/simd.rs` — `*_fast()` functions
- **Hybrid encryption**: `cpac-crypto/src/hybrid.rs`
- **Mmap I/O**: `cpac-streaming/src/mmap.rs`

## Shell Execution Rule — HARD RULE

**CRITICAL**: Agents must NEVER execute cargo, rustup, python, or any project
command directly from the terminal. ALL commands must be dispatched through
the unified shell entry points:

- **Windows (PowerShell)**: `.\shell.ps1 <command> [args...]`
- **Linux / macOS (bash)**: `./shell.sh <command> [args...]`

If no command is given, the shell drops into an interactive Python venv shell.

Available commands (via `scripts/cpac.py`):

| Command | Description |
|---|---|
| `build` | `cargo build --workspace` |
| `test` | `cargo test --workspace` |
| `clippy` | `cargo clippy --workspace -- -D warnings` |
| `fmt` | `cargo fmt --all -- --check` |
| `check` | clippy + fmt + test |
| `bench` | run a single benchmark file |
| `benchmark-all` | full corpus benchmark suite |
| `benchmark-external` | compare CPAC vs zstd/brotli/lz4/gzip/xz/snappy on a corpus |
| `criterion` | Criterion benchmarks |
| `pgo-build` | PGO-optimised release build |
| `download-corpus` | fetch benchmark corpus files |
| `setup` | install Rust toolchain components |
| `info` | show environment info |
| `analyze` | analyze a file's compression characteristics |
| `compress` | compress a file |
| `decompress` | decompress a file |

Examples:
```powershell
.\shell.ps1 build              # build workspace
.\shell.ps1 test               # run tests
.\shell.ps1 clippy             # lint
.\shell.ps1 bench dickens      # benchmark one file
.\shell.ps1 benchmark-all      # full benchmark suite
.\shell.ps1                    # interactive venv shell
```

```bash
./shell.sh build
./shell.sh test
./shell.sh check
```

## Scripting Policy — HARD RULE

**CRITICAL**: All new automation, tooling, and capability scripts **MUST** be
added as commands in `scripts/cpac.py`. Do NOT create platform-specific scripts
(`.ps1`, `.sh`, `.bat`, `.cmd`) for project capabilities.

The **only** permitted platform-specific shell files are the two entry-point
shims that already exist:

- `shell.ps1` — Windows PowerShell entry point (delegates to `cpac.py`)
- `shell.sh` — Linux/macOS bash entry point (delegates to `cpac.py`)

**Why**: `cpac.py` is the single cross-platform source of truth for all build,
test, benchmark, and utility commands. Platform-specific scripts create
maintenance burden, divergent behavior, and confusion about which script is
canonical. Python runs everywhere the project does.

**If you need a new command**: add a `cmd_<name>()` function to `cpac.py`,
wire it into `argparse` and the `dispatch` dict, then invoke via
`shell.ps1 <name>` or `shell.sh <name>`.

## Common CLI Operations

Quick reference for frequently-used benchmark and development commands.
All must go through `shell.ps1` / `shell.sh` (see Shell Execution Rule).

### Benchmarking

```powershell
# Quick benchmark (1 iteration, all 12 backends + matched baselines + Track 1+2)
cargo run --release -p cpac-cli -- benchmark <file> --quick

# Balanced benchmark (5 iterations)
cargo run --release -p cpac-cli -- benchmark <file>

# Full benchmark (50 iterations, high-precision)
cargo run --release -p cpac-cli -- benchmark <file> --full

# Skip standalone baselines (CPAC pipeline only)
cargo run --release -p cpac-cli -- benchmark <file> --quick --skip-baselines

# JSON output for machine parsing
cargo run --release -p cpac-cli -- benchmark <file> --quick --json

# Discovery mode (forced Track 1 vs Track 2 comparison)
cargo run --release -p cpac-cli -- benchmark <file> --quick --discovery

# Specific backends only
cargo run --release -p cpac-cli -- benchmark <file> --quick --backends zstd,brotli,lz4

# Specific levels
cargo run --release -p cpac-cli -- benchmark <file> --quick --levels ultrafast,default,best

# Full presubmit check (build + test + clippy + fmt)
.\shell.ps1 check
```

IMPORTANT: Always use `--release` for benchmark runs. Debug-mode throughput
numbers are 5-50x slower and misleading.

### Development Workflow

```powershell
.\shell.ps1 check          # full presubmit: build + test + clippy + fmt
.\shell.ps1 build          # cargo build --workspace
.\shell.ps1 test           # cargo test --workspace
.\shell.ps1 clippy         # cargo clippy --workspace -- -D warnings
.\shell.ps1 fmt            # cargo fmt --all -- --check
cargo fmt --all            # auto-fix formatting
cargo test -p cpac-engine  # test a single crate
cargo test -p cpac-entropy -- roundtrip_lzham  # run one specific test
```

### Compression / Decompression

```powershell
cargo run --release -p cpac-cli -- compress <file> --backend zstd --level best
cargo run --release -p cpac-cli -- decompress <file>.cpac
cargo run --release -p cpac-cli -- compress <file> --enable-msn --smart
cargo run --release -p cpac-cli -- info <file>.cpac
cargo run --release -p cpac-cli -- info --host
```

## CPAC Pipeline vs Standalone Backend Performance

### What the Data Shows (release-mode, 500 KB JSONL, Quick benchmark)

**Compression ratio**: CPAC pipeline achieves the **same ratio** as the
standalone backend in every case. The SSR analyzer routes data to the correct
backend and the framing overhead is negligible (<0.1% size increase).

**Compression throughput**: CPAC pipeline is **slower** than standalone
backends due to pipeline overhead (SSR analysis, transform evaluation,
wire-format framing). The overhead varies by backend speed:
- Slow backends (LZMA, XZ, LZHAM): ~1.2-1.4x slower (pipeline cost small vs codec cost)
- Medium backends (Brotli, Lizard): ~1.5-3.7x slower
- Fast backends (Zstd, Gzip, zlib-ng, OpenZl): ~5-9x slower
- Ultra-fast backends (LZ4, Snappy): ~12-74x slower (pipeline dominates)

**Decompression throughput**: CPAC is 5-80% slower depending on backend,
with fast decompressors (LZ4, Lizard) showing the largest relative overhead
because frame parsing cost becomes significant vs near-memcpy decompression.

### Why CPAC Exists Despite Pipeline Overhead

CPAC does NOT aim to beat individual backends on raw throughput. The value
proposition is the **integrated pipeline**:

1. **Adaptive backend selection** — SSR automatically picks the best backend
   for each data block (e.g., Zstd for binary, Brotli for text, Raw for
   incompressible data). Users don't need to guess.
2. **Format-aware MSN extraction** — Semantic field extraction for JSON, CSV,
   XML, logs that can improve ratio on structured data.
3. **Transform pipeline** — BWT, delta coding, dictionary dedup applied when
   they help (corpus-dependent; not all data benefits).
4. **Integrated encryption** — AEAD + PQC hybrid encryption in the same
   pipeline, no separate tooling.
5. **Streaming / parallel** — Bounded-memory streaming, block-parallel
   compression, mmap I/O.
6. **Cross-engine compatibility** — Same wire format readable by Rust and
   Python engines.

### When CPAC Adds Ratio Improvement

The quick single-file benchmarks above show equal ratios because the test
data doesn't trigger transforms. CPAC can improve ratio when:
- **MSN is enabled** on structured data (JSON/CSV logs) — field extraction
  reduces redundancy before backend compression.
- **Transform pipeline** kicks in — BWT on text, delta on time-series,
  dictionary dedup on repetitive corpora.
- **Multi-file archives** with dedup — cross-file content-addressable dedup
  in CPAR format.

### Honest Assessment

On simple single-file compression of typical data, a standalone `zstd` or
`brotli` call will be faster than CPAC with the same backend. CPAC's value
is in scenarios where the full pipeline matters: heterogeneous data,
automated backend selection, encryption, streaming, or structured data
where MSN/transforms provide ratio lift.

## Coding Conventions

- **Error handling**: `CpacError` enum (thiserror). No `unwrap()` in library crates.
- **`#[must_use]`** on public functions returning `Result`.
- **Doc comments** on all public items.
- **Unit tests** in each crate (`#[cfg(test)] mod tests`).
- **Integration tests** in `tests/` directory of `cpac-engine`.
- **NO SYNTHETIC DATA — HARD RULE**: This applies to **tests AND benchmarks**. Never generate, create, or use synthetic/fake data for any test or benchmark purpose. Benchmark results derived from synthetic data are invalid and must be deleted. All benchmarks must run exclusively against the official corpus files downloaded via the corpus configs in `benches/cpac/corpora/`. Never create files under `.work/benchmarks/bench-corpus/` or any ad-hoc corpus directory — this is explicitly prohibited.
- **CORPUS LOCALITY — HARD RULE**: Benchmark corpus files must live in `cpac/.work/benchdata/` inside **this** repository. Never reference, symlink, junction, or use corpus files from other repositories (e.g. `cpac-engine-python/.work/`). Download corpora with `shell.ps1 download-corpus`. The `.work/` directory must be a real directory, not a junction or symlink.
- **Copyright header** on every `.rs` file:
  ```
  // Copyright (c) 2026 BitConcepts, LLC
  // SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
  ```
- **Commit messages**: include `Co-Authored-By: Oz <oz-agent@warp.dev>` when AI-assisted.

## Agent Output Limits — HARD RULE

**CRITICAL**: AI agents must never allow unbounded test/build output to flood the
context window. Violating this has caused session crashes (403 errors).

1. **Never run `cargo test --workspace` raw.** Always limit output:
   - Preferred: run per-crate (`cargo test -p cpac-engine`) to isolate failures.
   - If workspace-wide is needed: use `cargo test --workspace -- --format terse 2>&1 | Select-Object -Last 30` (PowerShell) or `| tail -30` (bash) to capture only the summary.
   - Alternative: redirect full output to `.work/temp/test-output.txt` and then read only the last N lines.
2. **Never use `--nocapture` on workspace-wide test runs.** Test println!/dbg! output is captured by default — only use `--nocapture` on a single targeted test.
3. **Clippy/build warnings**: pipe through `Select-Object -Last 50` or equivalent.
4. **Benchmark output**: always redirect to `.work/benchmarks/`, never stream raw to the agent context.
5. **If a command might produce >4 KB of output**, either redirect to a file or use summarization/tail strategies.

## Known Gotchas (Rust 1.93 / Clippy)

- Use `.is_multiple_of()` instead of `% == 0`
- Use `if let` instead of single-arm `match`
- Use `.unwrap_or_default()` instead of manual if-let-else patterns
- `#[derive(Default)]` instead of manual impl when all fields are zero/false
- `unwrap_or_else(|_| panic!(...))` instead of `expect(&format!(...))`
- `#[allow(clippy::too_many_arguments)]` for functions with > 7 args

### sysinfo 0.33

- Needs `features = ["system"]` (no `cpu` feature exists)
- `physical_core_count()` is an instance method, not static

### AVX-512 intrinsics

- Use `*const __m512i` / `*mut __m512i` pointer types (NOT `*const i32`)
- `_mm512_cmpgt_epi8_mask` + `_mm512_movm_epi8` for sign detection
- Detection via `is_x86_feature_detected!("avx512f")` works on stable Rust

## Adding a New Transform

1. Create `cpac-transforms/src/my_transform.rs` with encode/decode functions
2. Add `pub mod my_transform;` to `cpac-transforms/src/lib.rs`
3. Implement `TransformNode` trait (see `cpac-transforms/src/traits.rs`)
4. Register in `TransformRegistry::with_builtins()` in `cpac-dag/src/registry.rs`
5. Optionally add SIMD kernel in `cpac-transforms/src/simd.rs`
6. Add tests, run `cargo test --workspace && cargo clippy --workspace -- -D warnings`

## Adding a New CLI Subcommand

1. Add variant to `Commands` enum in `cpac-cli/src/main.rs`
2. Add `cmd_*` handler function
3. Wire in the `main()` match arm
4. Test with `cargo build -p cpac-cli && cargo run -p cpac-cli -- <subcommand> --help`

## Wire Format Compatibility

The Rust engine must produce frames decompressible by the Python engine
and vice-versa. Magic bytes, version numbers, backend IDs, and transform
IDs must match. See `docs/SPEC.md`.

## Safe Defaults

- **Threads**: physical core count (not logical/HT)
- **Memory**: 25% of system RAM, clamped to 256 MB – 8 GB
- **Parallel threshold**: 256 KiB minimum before engaging block-parallel
- **Mmap threshold**: 64 MiB minimum for auto memory-mapping
- **Block size**: 1 MiB default for parallel and streaming

## Benchmark Reporting Rule — HARD RULE

**CRITICAL**: Whenever benchmark results are reported (in docs, summaries,
or conversation), the report **MUST** include:

1. **All compared compressors** — list every codec tested (cpac, zstd, brotli,
   lz4, gzip, xz, snappy, lzma, and any others present in the run).
2. **Where each compressor is strong** — briefly note the use-cases or data
   types where each external codec excels (e.g., "zstd: best speed/ratio
   balance for general data", "brotli: highest ratio on text/web", "lz4:
   fastest throughput for real-time ingest", "xz: highest ratio for archival",
   "snappy: lowest latency for hot-path data").
3. **How CPAC enhances each** — explain what CPAC adds on top (preprocessing
   transforms like BWT/delta/MSN, adaptive backend selection, CDC dedup, PQC
   encryption, streaming/mmap, parallel blocks).
4. **Where CPAC shines** — highlight CPAC's unique strengths (adaptive
   multi-backend routing, format-aware MSN extraction, integrated pipeline
   from analysis to compression to encryption, cross-engine compatibility).

This rule applies to benchmark-all, benchmark-external, and any ad-hoc
benchmark comparisons generated by agents.

## File Organization Rules

### Repository Root Policy

**CRITICAL**: The repository root must stay clean. Only the following are permitted:

- **Build configuration**: `Cargo.toml`, `Cargo.lock`, `rust-toolchain.toml`, `clippy.toml`
- **Documentation**: `README.md`, `LICENSE`, `SECURITY.md`, `AGENTS.md`, `WARP.md`
- **Shell entry points**: `shell.ps1` (Windows), `shell.sh` (Linux/macOS)
- **Dependencies**: `requirements.txt`
- **Directories**: `crates/`, `docs/`, `fuzz/`, `benches/`, `python/`, `scripts/`, `target/`, `.work/`
- **Hidden files**: `.git/`, `.gitignore`, `.gitattributes`, `.github/`

### Temporary/Generated Files → `.work/`

All temporary, cache, and generated files **MUST** go into `.work/` subdirectories:

- **Benchmarks**: `.work/benchmarks/` — corpus files, results CSVs/Markdown, analysis reports
- **Cache**: `.work/cache/` — downloaded datasets, precompiled profiles, temp build artifacts
- **Temp**: `.work/temp/` — scratch files, logs, intermediate data

**Prohibited in root**:
- `bench-corpus/`, `bench-results*`, `*_REPORT.md`, `*_RECOMMENDATIONS.md`
- Build artifacts: `*.o`, `*.rlib`, `*.so`, `*.dll`, `*.dylib`
- Compressed test files: `*.cpac`, `*.cpar`
- Log files, PGO profiles (except in `pgo-data/` if needed)

The `.gitignore` enforces these rules. Agents must never create files in the root that are not explicitly permitted above.

### Documentation Structure

#### CPAC Repository (`cpac/docs/`)

Implementation-specific documentation for the Rust engine:

- `ARCHITECTURE.md` — crate structure, pipeline flow, internal APIs
- `SPEC.md` — wire format specification (CP/CS/TP/CPBL frames)
- `cpac-overview.md` — high-level technical explainer
- Session reports and development logs → `.work/temp/` (not committed)

#### CPSC Core Repository (`cpsc-core/docs/`)

Normative specifications and legal documentation:

- `specification/` — CPSC formal spec, mathematical foundations
- `patents/` — patent-related documentation
- `public/` — public-facing technical materials
- `GLOSSARY.md`, `LEGAL-FAQ.md`, `LEDGER.md`

**Rule**: Implementation details belong in `cpac/docs/`. Normative/legal/specification content belongs in `cpsc-core/docs/`.

### Code Generation and Scripts

- Benchmark runner: use `cpac-cli bench` or `cpac-engine/src/bench.rs` API
- Results output: always to `.work/benchmarks/`, never root
- CI/CD scripts: in `.github/workflows/` or `scripts/`
- Build scripts: `build.rs` in crate roots, NOT repo root

### Enforcement

1. Agents **must check** `.gitignore` before creating files in root
2. Any file not matching the permitted list → move to `.work/` appropriate subfolder
3. Periodic cleanup: `git status --ignored` to find violations
4. PRs that add root clutter will be rejected
