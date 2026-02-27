#!/usr/bin/env bash
# =============================================================================
# Lowkey VPN — Development Environment Launcher
# =============================================================================
#
# Usage:
#   ./dev.sh              → start server + web in watch mode (requires .env)
#   ./dev.sh --server     → start only the VPN server
#   ./dev.sh --web        → start only the Next.js dev server
#   ./dev.sh --desktop    → start only the Tauri desktop dev session
#   ./dev.sh --check      → check all dependencies without starting anything
#   ./dev.sh --setup      → install all development dependencies
#
# Requires:
#   - Rust toolchain (rustup)
#   - Node.js 18+ and npm
#   - PostgreSQL running locally (or DATABASE_URL in .env)
#   - .env file in project root (copy from .env.example)
#
# =============================================================================

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ENV_FILE="$SCRIPT_DIR/.env"

RED='\033[0;31m'
GRN='\033[0;32m'
YEL='\033[1;33m'
BLU='\033[0;34m'
CYN='\033[0;36m'
RST='\033[0m'

info()    { echo -e "${BLU}[INFO]${RST}  $*"; }
ok()      { echo -e "${GRN}[ OK ]${RST}  $*"; }
warn()    { echo -e "${YEL}[WARN]${RST}  $*"; }
err()     { echo -e "${RED}[ERR ]${RST}  $*" >&2; }
section() { echo -e "\n${CYN}══ $* ══${RST}"; }

# ── Dependency checks ─────────────────────────────────────────────────────────

check_deps() {
    section "Checking development dependencies"
    local all_ok=true

    if command -v cargo &>/dev/null; then
        ok "Rust/cargo $(cargo --version 2>&1 | awk '{print $2}')"
    else
        err "Rust not found. Install from https://rustup.rs"
        all_ok=false
    fi

    if command -v node &>/dev/null; then
        local nver
        nver=$(node --version 2>&1)
        local nmaj
        nmaj=$(echo "$nver" | tr -d 'v' | cut -d. -f1)
        if [[ "$nmaj" -ge 18 ]]; then
            ok "Node.js $nver"
        else
            warn "Node.js $nver found, but >= 18 recommended"
        fi
    else
        err "Node.js not found. Install from https://nodejs.org"
        all_ok=false
    fi

    if command -v npm &>/dev/null; then
        ok "npm $(npm --version)"
    else
        err "npm not found"
        all_ok=false
    fi

    if command -v psql &>/dev/null; then
        ok "PostgreSQL client $(psql --version | awk '{print $3}')"
    else
        warn "psql not found — make sure DATABASE_URL points to a running PostgreSQL instance"
    fi

    if [[ -f "$ENV_FILE" ]]; then
        ok ".env file found at $ENV_FILE"
    else
        warn ".env not found. Copy .env.example to .env and fill in your values."
        warn "  cp .env.example .env && nano .env"
    fi

    $all_ok && ok "All required dependencies are present." || {
        err "Some dependencies are missing. Run  ./dev.sh --setup  or install them manually."
        return 1
    }
}

# ── Install all development dependencies ─────────────────────────────────────

setup_dev() {
    section "Installing development dependencies"

    # Rust
    if ! command -v cargo &>/dev/null; then
        info "Installing Rust via rustup..."
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y
        source "$HOME/.cargo/env"
    else
        info "Rust already installed, updating..."
        rustup update stable
    fi
    ok "Rust ready."

    # Node via nvm or direct
    if ! command -v node &>/dev/null; then
        if command -v nvm &>/dev/null; then
            nvm install --lts && nvm use --lts
        else
            warn "Node.js not found. Install from https://nodejs.org (>= 18)"
        fi
    fi

    # Web dependencies
    section "Installing web dependencies"
    cd "$SCRIPT_DIR/web"
    npm install --legacy-peer-deps
    ok "Web deps installed."

    # Desktop dependencies
    section "Installing desktop dependencies"
    cd "$SCRIPT_DIR/vpn-desktop"
    npm install --legacy-peer-deps
    ok "Desktop deps installed."

    # Create .env if missing
    if [[ ! -f "$ENV_FILE" ]]; then
        cp "$SCRIPT_DIR/.env.example" "$ENV_FILE"
        warn ".env created from .env.example — please edit it before starting the server."
        warn "  nano $ENV_FILE"
    fi

    ok "Development setup complete."
    echo ""
    echo -e "  Next steps:"
    echo -e "  1. Edit ${YEL}.env${RST} with your database URL, JWT secret and VPN PSK"
    echo -e "  2. Run ${GRN}./dev.sh${RST} to start the server + web in development mode"
}

# ── Load .env ─────────────────────────────────────────────────────────────────

load_env() {
    if [[ -f "$ENV_FILE" ]]; then
        set -o allexport
        # shellcheck disable=SC1090
        source "$ENV_FILE"
        set +o allexport
        ok "Loaded $ENV_FILE"
    else
        err ".env not found at $ENV_FILE"
        err "Copy .env.example to .env and fill in your values."
        exit 1
    fi
}

# ── Start VPN server (cargo run --release) ────────────────────────────────────

start_server() {
    section "Starting VPN server (dev mode)"
    cd "$SCRIPT_DIR"
    load_env

    if [[ $EUID -ne 0 ]]; then
        warn "VPN server needs root privileges for TUN device creation."
        warn "Re-running with sudo..."
        exec sudo -E env PATH="$PATH" \
            DATABASE_URL="${DATABASE_URL:-}" \
            JWT_SECRET="${JWT_SECRET:-}" \
            VPN_PSK="${VPN_PSK:-}" \
            API_PORT="${API_PORT:-8080}" \
            UDP_PORT="${UDP_PORT:-51820}" \
            PROXY_PORT="${PROXY_PORT:-8388}" \
            cargo run -p vpn-server
    fi

    info "Server will listen on:"
    info "  HTTP API  → http://0.0.0.0:${API_PORT:-8080}"
    info "  UDP VPN   → udp/0.0.0.0:${UDP_PORT:-51820}"
    info "  TCP proxy → tcp/0.0.0.0:${PROXY_PORT:-8388}"

    cargo run -p vpn-server
}

# ── Start Next.js web in dev mode ─────────────────────────────────────────────

start_web() {
    section "Starting Next.js web (dev mode)"
    cd "$SCRIPT_DIR/web"

    if [[ ! -d node_modules ]]; then
        info "Installing web dependencies..."
        npm install --legacy-peer-deps
    fi

    export NEXT_PUBLIC_API_URL="${NEXT_PUBLIC_API_URL:-http://localhost:${API_PORT:-8080}}"
    info "Web running at http://localhost:3000"
    info "API proxied to $NEXT_PUBLIC_API_URL"
    npm run dev
}

# ── Start Tauri desktop in dev mode ───────────────────────────────────────────

start_desktop() {
    section "Starting Tauri desktop (dev mode)"
    cd "$SCRIPT_DIR/vpn-desktop"

    if [[ ! -d node_modules ]]; then
        info "Installing desktop dependencies..."
        npm install --legacy-peer-deps
    fi

    info "Starting Tauri in development mode..."
    npm run tauri:dev
}

# ── Run server + web concurrently ─────────────────────────────────────────────

start_all() {
    section "Starting all services (server + web)"

    if command -v tmux &>/dev/null; then
        info "Using tmux for split-pane view."
        tmux new-session -d -s lowkey-dev -n server \
            "cd '$SCRIPT_DIR' && bash dev.sh --server; read"
        tmux new-window -t lowkey-dev -n web \
            "cd '$SCRIPT_DIR' && bash dev.sh --web; read"
        tmux select-window -t lowkey-dev:web
        info "Attached to tmux session 'lowkey-dev'. Switch panes with Ctrl-B + n/p."
        tmux attach-session -t lowkey-dev
    else
        warn "tmux not found — starting server in background, web in foreground."
        bash "$SCRIPT_DIR/dev.sh" --server &
        SERVER_PID=$!
        trap "kill $SERVER_PID 2>/dev/null; exit" INT TERM
        sleep 2
        bash "$SCRIPT_DIR/dev.sh" --web
        kill $SERVER_PID 2>/dev/null || true
    fi
}

# ── Argument parsing ──────────────────────────────────────────────────────────

if [[ $# -eq 0 ]]; then
    start_all
    exit 0
fi

case "$1" in
    --server)  start_server  ;;
    --web)     start_web     ;;
    --desktop) start_desktop ;;
    --check)   check_deps    ;;
    --setup)   setup_dev     ;;
    --help|-h)
        echo "Usage: $0 [--server] [--web] [--desktop] [--check] [--setup]"
        echo "  (no flag)  Start server + web in development mode"
        echo "  --server   Start only the Rust VPN server"
        echo "  --web      Start only the Next.js dev server"
        echo "  --desktop  Start only the Tauri desktop app"
        echo "  --check    Verify all development dependencies"
        echo "  --setup    Install all development dependencies"
        ;;
    *)
        err "Unknown option: $1"
        echo "Run $0 --help for usage."
        exit 1
        ;;
esac
