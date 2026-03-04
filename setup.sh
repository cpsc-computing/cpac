#!/usr/bin/env bash
# CPAC Rust Engine — Linux/Mac Setup
set -euo pipefail

echo "=== CPAC Rust Engine Setup (Linux/Mac) ==="

if ! command -v rustup &>/dev/null; then
    echo "Installing Rust via rustup..."
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
    source "$HOME/.cargo/env"
else
    echo "rustup found: $(rustup --version)"
fi

echo ""
echo "Rust toolchain:"
rustc --version
cargo --version

rustup component add rustfmt clippy 2>/dev/null || true

echo ""
echo "Building workspace..."
cd "$(dirname "$0")"
cargo build --workspace

echo ""
echo "Setup complete! Run: source ./env.sh"
