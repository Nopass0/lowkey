#!/usr/bin/env bash
# ─── Lowkey VPN — Master Build Script ──────────────────────────────────────
# Usage:
#   ./build.sh                  → build all components (release, native)
#   ./build.sh --server         → build Rust VPN server only
#   ./build.sh --web            → build Next.js web app
#   ./build.sh --desktop        → build Tauri desktop client
#   ./build.sh --android        → build Android APK (requires Android SDK)
#   ./build.sh --windows        → cross-compile server/client for Windows
#   ./build.sh --clean          → remove all dist/ output

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DIST_DIR="$SCRIPT_DIR/dist"

RED='\033[0;31m'
GRN='\033[0;32m'
YEL='\033[1;33m'
BLU='\033[0;34m'
CYN='\033[0;36m'
RST='\033[0m'

info()    { echo -e "${BLU}[INFO]${RST}  $*"; }
ok()      { echo -e "${GRN}[ OK ]${RST}  $*"; }
warn()    { echo -e "${YEL}[WARN]${RST}  $*"; }
section() { echo -e "\n${CYN}══ $* ══${RST}"; }

LINUX_TARGET="x86_64-unknown-linux-gnu"
WINDOWS_TARGET="x86_64-pc-windows-gnu"

# ── Build Rust server + client ───────────────────────────────────────────────
build_server() {
    section "Building Rust VPN server & client (Linux)"
    cd "$SCRIPT_DIR"
    cargo build --release --target "$LINUX_TARGET"
    mkdir -p "$DIST_DIR/linux"
    cp "target/$LINUX_TARGET/release/vpn-server" "$DIST_DIR/linux/"
    cp "target/$LINUX_TARGET/release/vpn-client" "$DIST_DIR/linux/" 2>/dev/null || true
    ok "→ dist/linux/vpn-server"
    ok "→ dist/linux/vpn-client"
}

# ── Build Rust for Windows ───────────────────────────────────────────────────
build_windows() {
    section "Building Rust VPN client (Windows)"
    if ! command -v x86_64-w64-mingw32-gcc &>/dev/null; then
        echo -e "${RED}ERROR: mingw-w64 not found.${RST}"
        echo "  Ubuntu/Debian: sudo apt install gcc-mingw-w64-x86-64"
        exit 1
    fi
    rustup target add "$WINDOWS_TARGET"
    cd "$SCRIPT_DIR"
    cargo build --release --target "$WINDOWS_TARGET" -p vpn-client
    mkdir -p "$DIST_DIR/windows"
    cp "target/$WINDOWS_TARGET/release/vpn-client.exe" "$DIST_DIR/windows/"
    ok "→ dist/windows/vpn-client.exe"
}

# ── Build Next.js web app ────────────────────────────────────────────────────
build_web() {
    section "Building Next.js web app"
    WEB_DIR="$SCRIPT_DIR/web"
    if [[ ! -d "$WEB_DIR" ]]; then
        echo -e "${RED}ERROR: web/ directory not found${RST}"
        exit 1
    fi
    cd "$WEB_DIR"
    if [[ ! -d node_modules ]]; then
        info "Installing npm dependencies..."
        npm ci --legacy-peer-deps
    fi
    npm run build
    mkdir -p "$DIST_DIR/web"
    cp -r .next/standalone/. "$DIST_DIR/web/"
    cp -r .next/static "$DIST_DIR/web/.next/static"
    cp -r public "$DIST_DIR/web/public" 2>/dev/null || true
    ok "→ dist/web/ (Next.js standalone)"
    info "Run: node dist/web/server.js"
}

# ── Build Tauri desktop client ───────────────────────────────────────────────
build_desktop() {
    section "Building Tauri desktop client"
    DESKTOP_DIR="$SCRIPT_DIR/vpn-desktop"
    if [[ ! -d "$DESKTOP_DIR" ]]; then
        echo -e "${RED}ERROR: vpn-desktop/ directory not found${RST}"
        exit 1
    fi
    cd "$DESKTOP_DIR"
    if [[ ! -d node_modules ]]; then
        info "Installing npm dependencies..."
        npm ci --legacy-peer-deps
    fi
    npm run tauri build 2>/dev/null || npx tauri build
    mkdir -p "$DIST_DIR/desktop"
    # Copy Linux bundle
    find src-tauri/target/release/bundle -name "*.deb" -o -name "*.AppImage" \
        -o -name "*.rpm" 2>/dev/null | while read -r f; do
        cp "$f" "$DIST_DIR/desktop/"
        ok "→ dist/desktop/$(basename "$f")"
    done
    # Also copy the raw binary
    cp src-tauri/target/release/vpn-desktop "$DIST_DIR/desktop/" 2>/dev/null || true
}

# ── Build Android APK ────────────────────────────────────────────────────────
build_android() {
    section "Building Android APK"
    ANDROID_DIR="$SCRIPT_DIR/android-app"
    if [[ ! -d "$ANDROID_DIR" ]]; then
        echo -e "${RED}ERROR: android-app/ directory not found${RST}"
        exit 1
    fi

    if [[ -z "${ANDROID_HOME:-}" ]] && [[ -z "${ANDROID_SDK_ROOT:-}" ]]; then
        warn "ANDROID_HOME / ANDROID_SDK_ROOT not set."
        warn "Install Android Studio or set ANDROID_HOME to your SDK path."
        warn "Skipping Android build."
        return 0
    fi

    cd "$ANDROID_DIR"
    ./gradlew assembleRelease --no-daemon
    mkdir -p "$DIST_DIR/android"
    find . -name "*.apk" -path "*/release/*" | while read -r f; do
        cp "$f" "$DIST_DIR/android/LowkeyVPN.apk"
        ok "→ dist/android/LowkeyVPN.apk"
    done
}

# ── Clean ────────────────────────────────────────────────────────────────────
do_clean() {
    section "Cleaning dist/"
    rm -rf "$DIST_DIR"
    ok "dist/ removed."
}

# ── Parse arguments ──────────────────────────────────────────────────────────
if [[ $# -eq 0 ]]; then
    build_server
    build_web
    build_desktop
    build_android
    echo ""
    ok "All components built. Output in dist/"
    exit 0
fi

for arg in "$@"; do
    case "$arg" in
        --server)  build_server ;;
        --web)     build_web ;;
        --desktop) build_desktop ;;
        --android) build_android ;;
        --windows) build_windows ;;
        --clean)   do_clean ;;
        --help|-h)
            echo "Usage: $0 [--server] [--web] [--desktop] [--android] [--windows] [--clean]"
            echo "  (no flag)  Build all components"
            echo "  --server   Build Rust VPN server + client (Linux)"
            echo "  --web      Build Next.js web app"
            echo "  --desktop  Build Tauri desktop client"
            echo "  --android  Build Android APK (requires ANDROID_HOME)"
            echo "  --windows  Cross-compile client for Windows"
            echo "  --clean    Remove dist/ directory"
            exit 0 ;;
        *)
            echo -e "${RED}Unknown flag: $arg${RST}"
            echo "Run $0 --help for usage."
            exit 1 ;;
    esac
done
