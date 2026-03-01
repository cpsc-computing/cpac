# CPAC Agent Guide

This is the CPAC compression engine repository. Read this file first when
onboarding to the codebase. See also `WARP.md` for Warp-specific project
rules and `LEDGER.md` for development history.

## Workspace Overview

13-crate Cargo workspace under `crates/`. No circular dependencies.

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
- `cpac-domains` — CSV/JSON/XML/YAML/log domain handlers
- `cpac-cas` — constraint inference, DoF extraction, cost model
- `cpac-archive` — CPAR multi-file archive format

## Key Entry Points

- **Compress/decompress API**: `cpac-engine/src/lib.rs` — `compress()`, `decompress()`
- **Parallel API**: `cpac-engine/src/parallel.rs` — `compress_parallel()`, `decompress_parallel()`
- **CLI main**: `cpac-cli/src/main.rs`
- **Host detection**: `cpac-engine/src/host.rs` — `detect_host()`, `auto_resource_config()`
- **SIMD dispatch**: `cpac-transforms/src/simd.rs` — `*_fast()` functions
- **Hybrid encryption**: `cpac-crypto/src/hybrid.rs`
- **Mmap I/O**: `cpac-streaming/src/mmap.rs`

## Build & Test

```bash
cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
cargo fmt --all -- --check
```

### Windows PATH fix (PowerShell)

If cargo is not found, run first:
```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
```

### Benchmarks

```bash
cargo bench -p cpac-engine                    # all bench suites
cargo bench -p cpac-engine --bench compress   # pipeline + backends
cargo bench -p cpac-engine --bench simd       # SIMD vs scalar
cargo bench -p cpac-engine --bench dag        # DAG compile/execute
```

## Coding Conventions

- **Error handling**: `CpacError` enum (thiserror). No `unwrap()` in library crates.
- **`#[must_use]`** on public functions returning `Result`.
- **Doc comments** on all public items.
- **Unit tests** in each crate (`#[cfg(test)] mod tests`).
- **Integration tests** in `tests/` directory of `cpac-engine`.
- **Copyright header** on every `.rs` file:
  ```
  // Copyright (c) 2026 BitConcepts, LLC
  // SPDX-License-Identifier: LicenseRef-CPAC-Research-Evaluation-1.0
  ```
- **Commit messages**: include `Co-Authored-By: Oz <oz-agent@warp.dev>` when AI-assisted.

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
