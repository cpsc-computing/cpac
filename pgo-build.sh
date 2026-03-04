#!/usr/bin/env bash
# PGO (Profile-Guided Optimization) build for CPAC.
#
# Usage:  ./pgo-build.sh
# Output: target/release/cpac (PGO-optimized binary)
#
# Requires: rustup component add llvm-tools-preview
set -euo pipefail

WORKSPACE="$(cd "$(dirname "$0")" && pwd)"
PROFDATA_DIR="$WORKSPACE/target/pgo-profiles"

echo "=== Step 1: Instrumented build ==="
RUSTFLAGS="-Cprofile-generate=$PROFDATA_DIR" \
    cargo build --release --manifest-path="$WORKSPACE/Cargo.toml" -p cpac-cli

echo "=== Step 2: Gather profiles (run tests as workload) ==="
RUSTFLAGS="-Cprofile-generate=$PROFDATA_DIR" \
    cargo test --release --manifest-path="$WORKSPACE/Cargo.toml" --workspace

echo "=== Step 3: Merge profiles ==="
LLVM_PROFDATA="$(rustc --print sysroot)/lib/rustlib/$(rustc -vV | sed -n 's|host: ||p')/bin/llvm-profdata"
if [ ! -f "$LLVM_PROFDATA" ]; then
    # Fallback: try PATH
    LLVM_PROFDATA="llvm-profdata"
fi
"$LLVM_PROFDATA" merge -o "$PROFDATA_DIR/merged.profdata" "$PROFDATA_DIR"

echo "=== Step 4: PGO-optimized build ==="
RUSTFLAGS="-Cprofile-use=$PROFDATA_DIR/merged.profdata -Cllvm-args=-pgo-warn-missing-function" \
    cargo build --release --manifest-path="$WORKSPACE/Cargo.toml" -p cpac-cli

echo "=== Done! Binary at target/release/cpac ==="
ls -lh "$WORKSPACE/target/release/cpac" 2>/dev/null || ls -lh "$WORKSPACE/target/release/cpac.exe" 2>/dev/null || true
