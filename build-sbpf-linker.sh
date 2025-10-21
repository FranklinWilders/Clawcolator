#!/usr/bin/env bash
#
# Build all programs using sbpf-linker (nightly)
#
# This requires:
# - Rust nightly toolchain
# - sbpf-linker installed

set -e

cd "$(dirname "$0")"

echo "========================================="
echo "Building with sbpf-linker (nightly)"
echo "========================================="
echo

# Check for sbpf-linker
if ! command -v sbpf-linker &> /dev/null; then
    echo "Error: sbpf-linker not found"
    echo "Install from: https://github.com/blueshift-gg/upstream-pinocchio-escrow"
    exit 1
fi

# Check for nightly toolchain
if ! rustup toolchain list | grep -q nightly; then
    echo "Installing nightly toolchain..."
    rustup toolchain install nightly
fi

echo "✓ sbpf-linker found: $(which sbpf-linker)"
echo "✓ Using toolchain: nightly"
echo

# Build each program with nightly + sbpf-linker
for program in programs/slab programs/router programs/oracle; do
    echo ">>> Building $program with sbpf-linker..."
    cd "$program"

    # Use nightly and let .cargo/config.toml handle sbpf-linker
    cargo +nightly build-sbf --release

    cd - > /dev/null
    echo
done

echo "========================================="
echo "sbpf-linker builds complete!"
echo "========================================="
echo
echo "Binaries in: ./target/deploy/"
ls -lh target/deploy/*.so 2>/dev/null || echo "  (no .so files found)"
echo
