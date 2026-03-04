# CPAC — Warp Project Rules

## Identity
- Repository: `BitConcepts/cpac`
- Tool name: **CPAC** (Constraint-Projected Adaptive Compression)
- License: CPAC Research & Evaluation License v1.0

## Build Commands
```bash
cargo build --workspace           # debug build
cargo build --release             # release (fat LTO)
cargo test --workspace            # all tests (~220+)
cargo clippy --workspace -- -D warnings  # lint (must pass)
cargo fmt --all -- --check        # format check
cargo bench -p cpac-engine        # criterion benchmarks
```

## Windows PATH
Always set before any cargo command in PowerShell:
```powershell
$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
```

## Presubmit Checklist
Before committing, all of the following must pass:
1. `cargo build --workspace`
2. `cargo test --workspace` (ignore pre-existing fuzz_equivalent failure)
3. `cargo clippy --workspace -- -D warnings`
4. `cargo fmt --all -- --check`

## Commit Conventions
- Conventional commit style preferred: `feat:`, `fix:`, `refactor:`, `docs:`, `bench:`
- Always include `Co-Authored-By: Oz <oz-agent@warp.dev>` on AI-assisted commits
- Never commit unless the user explicitly asks

## Rust Version
- Minimum: 1.75 stable
- Tested: 1.93 stable (x86_64-pc-windows-msvc)

## Code Style
- Copyright header on every `.rs` file
- `CpacError` for all errors, no `unwrap()` in library crates
- `#[must_use]` on compress/decompress and other Result-returning public APIs
- Doc comments on all public items
- Unit tests in `#[cfg(test)] mod tests` within each source file

## Architecture Rules
- No circular crate dependencies
- `cpac-types` is the leaf dependency (no internal deps)
- `cpac-engine` is the top-level API crate
- `cpac-cli` depends on engine + extensions, never the reverse
- Feature gates for heavyweight optional deps (e.g., `pqc` on `cpac-crypto`)

## File Extensions
- `.cpac` — compressed file
- `.cpar` — multi-file archive
- `.cpac-enc` — password-encrypted file
- `.cpac-pqe` — hybrid PQC-encrypted file
- `.cpac-sig` — ML-DSA-65 signature
- `.cpac-pub` / `.cpac-sec` — public/secret key files

## Git Branch
- Main development: `main`
- Feature branches: `feature/<name>`
- Current: `feature/rust-port` (will merge to `main` when repo moves)

## Key Files to Load
When starting work on this codebase, load:
1. `AGENTS.md` — codebase overview, conventions, gotchas
2. `WARP.md` — this file (project rules)
3. `LEDGER.md` — session history
4. `docs/SPEC.md` — wire format reference
5. `docs/ARCHITECTURE.md` — system design
