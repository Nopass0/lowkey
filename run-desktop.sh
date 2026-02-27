#!/usr/bin/env bash
# ─── Lowkey VPN — Desktop Client Runner ────────────────────────────────────
# Usage:
#   ./run-desktop.sh          → start Tauri app in development mode
#   ./run-desktop.sh --build  → build release Tauri bundle

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DESKTOP_DIR="$SCRIPT_DIR/vpn-desktop"

RED='\033[0;31m'; GRN='\033[0;32m'; BLU='\033[0;34m'; RST='\033[0m'
info() { echo -e "${BLU}[INFO]${RST}  $*"; }
ok()   { echo -e "${GRN}[ OK ]${RST}  $*"; }
err()  { echo -e "${RED}[ERR ]${RST}  $*" >&2; }

if [[ ! -d "$DESKTOP_DIR" ]]; then
    err "vpn-desktop/ not found at $DESKTOP_DIR"
    exit 1
fi

# Check for required system libraries
if ! ldconfig -p 2>/dev/null | grep -q libwebkit2gtk; then
    warn() { echo -e "\033[1;33m[WARN]\033[0m  $*"; }
    warn "webkit2gtk not found. Install with:"
    warn "  Ubuntu/Debian: sudo apt install libwebkit2gtk-4.1-dev libgtk-3-dev"
fi

cd "$DESKTOP_DIR"

if [[ ! -d node_modules ]]; then
    info "Installing npm dependencies..."
    npm ci --legacy-peer-deps
fi

case "${1:-}" in
    --build)
        ok "Building Tauri release bundle..."
        npm run tauri build 2>/dev/null || npx tauri build
        ok "Bundle output: vpn-desktop/src-tauri/target/release/bundle/"
        ;;
    *)
        ok "Starting Tauri dev mode..."
        npm run tauri dev 2>/dev/null || npx tauri dev
        ;;
esac
