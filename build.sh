#!/usr/bin/env bash
# ─── Lowkey VPN build script ────────────────────────────────────────────────
# Usage:
#   ./build.sh           → release builds for native platform
#   ./build.sh --windows → cross-compile for Windows x86_64 (needs mingw-w64)
#   ./build.sh --all     → both Linux and Windows

set -euo pipefail

LINUX_TARGET="x86_64-unknown-linux-gnu"
WINDOWS_TARGET="x86_64-pc-windows-gnu"

build_linux() {
    echo "=== Building for Linux ($LINUX_TARGET) ==="
    cargo build --release --target "$LINUX_TARGET"
    mkdir -p dist/linux
    cp "target/$LINUX_TARGET/release/vpn-server" dist/linux/
    cp "target/$LINUX_TARGET/release/vpn-client" dist/linux/
    echo "→ dist/linux/vpn-server"
    echo "→ dist/linux/vpn-client"
}

build_windows() {
    echo "=== Building for Windows ($WINDOWS_TARGET) ==="
    if ! command -v x86_64-w64-mingw32-gcc &>/dev/null; then
        echo "ERROR: mingw-w64 not found."
        echo "  Ubuntu/Debian: sudo apt install gcc-mingw-w64-x86-64"
        exit 1
    fi
    rustup target add "$WINDOWS_TARGET"
    # vpn-client only — server doesn't run on Windows
    cargo build --release --target "$WINDOWS_TARGET" -p vpn-client
    mkdir -p dist/windows
    cp "target/$WINDOWS_TARGET/release/vpn-client.exe" dist/windows/
    echo "→ dist/windows/vpn-client.exe"
}

case "${1:-}" in
    --windows) build_windows ;;
    --all)     build_linux; build_windows ;;
    *)         build_linux ;;
esac

echo ""
echo "Build complete."
