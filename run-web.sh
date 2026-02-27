#!/usr/bin/env bash
# ─── Lowkey VPN — Web App Runner ───────────────────────────────────────────
# Usage:
#   ./run-web.sh              → install deps & start dev server
#   ./run-web.sh --prod       → start production (requires npm run build first)
#   ./run-web.sh --build-prod → build + start production

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
WEB_DIR="$SCRIPT_DIR/web"

RED='\033[0;31m'; GRN='\033[0;32m'; BLU='\033[0;34m'; RST='\033[0m'
info() { echo -e "${BLU}[INFO]${RST}  $*"; }
ok()   { echo -e "${GRN}[ OK ]${RST}  $*"; }
err()  { echo -e "${RED}[ERR ]${RST}  $*" >&2; }

ensure_js_tooling() {
    if ! command -v node &>/dev/null; then
        err "Node.js is required but not installed (need Node.js 18+)"
        exit 1
    fi
    if ! command -v npm &>/dev/null; then
        err "npm is required but not installed"
        exit 1
    fi
}

install_web_deps() {
    if [[ -f package-lock.json ]]; then
        info "Installing dependencies via npm ci..."
        if npm ci --legacy-peer-deps; then
            return 0
        fi
        info "npm ci failed, retrying with npm install..."
    else
        info "package-lock.json not found, using npm install..."
    fi
    npm install --legacy-peer-deps
}

if [[ ! -d "$WEB_DIR" ]]; then
    err "web/ directory not found at $WEB_DIR"
    exit 1
fi

cd "$WEB_DIR"

# Load env if exists
if [[ -f "$SCRIPT_DIR/.env" ]]; then
    set -o allexport
    source "$SCRIPT_DIR/.env"
    set +o allexport
fi

# Set NEXT_PUBLIC_API_URL from API_PORT if not set
if [[ -z "${NEXT_PUBLIC_API_URL:-}" ]]; then
    API_PORT="${API_PORT:-8080}"
    export NEXT_PUBLIC_API_URL="http://localhost:$API_PORT"
    info "NEXT_PUBLIC_API_URL=$NEXT_PUBLIC_API_URL"
fi

ensure_js_tooling
install_web_deps

MODE="${1:-}"

case "$MODE" in
    --prod)
        if [[ ! -d .next/standalone ]]; then
            err ".next/standalone not found. Run: ./run-web.sh --build-prod"
            exit 1
        fi
        ok "Starting production web server on port 3000..."
        PORT=3000 node .next/standalone/server.js
        ;;
    --build-prod)
        info "Building production Next.js app..."
        npm run build
        ok "Starting production web server on port 3000..."
        PORT=3000 node .next/standalone/server.js
        ;;
    *)
        ok "Starting development server on port 3000..."
        npm run dev
        ;;
esac
