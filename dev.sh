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

source_nvm() {
    export NVM_DIR="${NVM_DIR:-$HOME/.nvm}"

    if [[ -s "$NVM_DIR/nvm.sh" ]]; then
        # shellcheck disable=SC1090
        source "$NVM_DIR/nvm.sh"
        return 0
    fi

    return 1
}

ensure_latest_node() {
    if ! source_nvm; then
        info "nvm not found, installing it to manage Node.js versions..."
        curl -fsSL https://raw.githubusercontent.com/nvm-sh/nvm/v0.40.3/install.sh | bash
        if ! source_nvm; then
            err "Failed to load nvm after installation."
            return 1
        fi
    fi

    info "Installing latest stable Node.js via nvm (this may take a moment)..."
    nvm install node >/dev/null
    nvm alias default node >/dev/null
    nvm use default >/dev/null

    local nver
    nver=$(node --version 2>&1)
    ok "Using Node.js $nver (default)"
}

kill_port_listeners() {
    local proto="$1"
    local port="$2"
    local label="$3"
    local pids=""

    if command -v lsof &>/dev/null; then
        if [[ "$proto" == "tcp" ]]; then
            pids=$(lsof -t -iTCP:"$port" -sTCP:LISTEN 2>/dev/null || true)
        else
            pids=$(lsof -t -iUDP:"$port" 2>/dev/null || true)
        fi
        pids=$(echo "$pids" | sort -u | tr '\n' ' ')
    fi

    if [[ -z "${pids// /}" ]] && command -v ss &>/dev/null; then
        if [[ "$proto" == "tcp" ]]; then
            pids=$(ss -tlnp "sport = :$port" 2>/dev/null | awk 'NR>1 && /pid=/{match($0,/pid=([0-9]+)/,a); if(a[1]) print a[1]}' | sort -u | tr '\n' ' ' || true)
        else
            pids=$(ss -ulnp "sport = :$port" 2>/dev/null | awk 'NR>1 && /pid=/{match($0,/pid=([0-9]+)/,a); if(a[1]) print a[1]}' | sort -u | tr '\n' ' ' || true)
        fi
    fi

    if [[ -z "${pids// /}" ]] && command -v fuser &>/dev/null; then
        if [[ "$proto" == "tcp" ]]; then
            pids=$(fuser -n tcp "$port" 2>/dev/null | tr ' ' '\n' | sort -u | tr '\n' ' ' || true)
        else
            pids=$(fuser -n udp "$port" 2>/dev/null | tr ' ' '\n' | sort -u | tr '\n' ' ' || true)
        fi
    fi

    pids="${pids// /}"
    if [[ -n "$pids" ]]; then
        pids=$(echo "$pids" | tr ' ' '\n' | grep -v '^$' | tr '\n' ' ')
    fi

    if [[ -n "${pids// /}" ]]; then
        warn "Port $port/$proto ($label) is busy. Stopping processes: $pids"
        # shellcheck disable=SC2086
        kill -TERM $pids 2>/dev/null || true
        sleep 1
        # shellcheck disable=SC2086
        kill -KILL $pids 2>/dev/null || true
        ok "Freed $port/$proto for $label"
    fi
}

free_dev_ports() {
    local target="${1:-all}"
    local api_port="${API_PORT:-8080}"
    local udp_port="${UDP_PORT:-51820}"
    local proxy_port="${PROXY_PORT:-8388}"
    local web_port="${WEB_PORT:-3000}"

    info "Ensuring required ports are free before startup..."

    case "$target" in
        server)
            kill_port_listeners tcp "$api_port" "HTTP API"
            kill_port_listeners udp "$udp_port" "UDP VPN"
            kill_port_listeners tcp "$proxy_port" "TCP proxy"
            ;;
        web)
            kill_port_listeners tcp "$web_port" "Next.js web"
            ;;
        all)
            kill_port_listeners tcp "$api_port" "HTTP API"
            kill_port_listeners udp "$udp_port" "UDP VPN"
            kill_port_listeners tcp "$proxy_port" "TCP proxy"
            kill_port_listeners tcp "$web_port" "Next.js web"
            ;;
        *)
            err "Unknown free_dev_ports target: $target"
            return 1
            ;;
    esac
}

ensure_npm_deps() {
    local target_dir="$1"

    if [[ -d "$target_dir/node_modules" ]]; then
        info "Dependencies already installed in $(basename "$target_dir"), skipping install."
        return 0
    fi

    install_npm_deps "$target_dir"
}

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
        if [[ "$nmaj" -ge 20 ]]; then
            ok "Node.js $nver"
        else
            warn "Node.js $nver found, but >= 20 is required for the current web stack"
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

install_npm_deps() {
    local target_dir="$1"
    cd "$target_dir"

    if [[ -f package-lock.json ]]; then
        info "Installing dependencies via npm ci in $(basename "$target_dir")..."
        if npm ci --legacy-peer-deps; then
            return 0
        fi
        warn "npm ci failed in $(basename "$target_dir"), retrying with npm install..."
    else
        warn "package-lock.json not found in $(basename "$target_dir"), using npm install..."
    fi

    npm install --legacy-peer-deps
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

    # Node via nvm (latest default)
    ensure_latest_node

    # Web dependencies
    section "Installing web dependencies"
    install_npm_deps "$SCRIPT_DIR/web"
    ok "Web deps installed."

    # Desktop dependencies
    section "Installing desktop dependencies"
    install_npm_deps "$SCRIPT_DIR/vpn-desktop"
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
    free_dev_ports server

    if [[ $EUID -ne 0 ]]; then
        warn "Running server without root privileges."
        warn "If your setup requires TUN creation, restart with sudo:  sudo ./dev.sh --server"
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

    ensure_latest_node
    free_dev_ports web

    ensure_npm_deps "$SCRIPT_DIR/web"

    export NEXT_PUBLIC_API_URL="${NEXT_PUBLIC_API_URL:-http://localhost:${API_PORT:-8080}}"
    info "Web running at http://localhost:${WEB_PORT:-3000}"
    info "API proxied to $NEXT_PUBLIC_API_URL"
    npm run dev -- --port "${WEB_PORT:-3000}"
}

# ── Start Tauri desktop in dev mode ───────────────────────────────────────────

start_desktop() {
    section "Starting Tauri desktop (dev mode)"
    cd "$SCRIPT_DIR/vpn-desktop"

    ensure_latest_node

    ensure_npm_deps "$SCRIPT_DIR/vpn-desktop"

    info "Starting Tauri in development mode..."
    npm run tauri:dev
}

# ── Run server + web concurrently ─────────────────────────────────────────────

start_all() {
    section "Starting all services (server + web)"

    # Load env and free all ports ONCE before spawning sub-processes
    load_env
    free_dev_ports all

    # Start server in background (sub-shell loads its own env)
    (
        cd "$SCRIPT_DIR"
        info "Server will listen on:"
        info "  HTTP API  → http://0.0.0.0:${API_PORT:-8080}"
        info "  UDP VPN   → udp/0.0.0.0:${UDP_PORT:-51820}"
        info "  TCP proxy → tcp/0.0.0.0:${PROXY_PORT:-8388}"
        cargo run -p vpn-server
    ) &
    SERVER_PID=$!

    # Start web in background
    (
        cd "$SCRIPT_DIR/web"
        ensure_latest_node
        ensure_npm_deps "$SCRIPT_DIR/web"
        export NEXT_PUBLIC_API_URL="${NEXT_PUBLIC_API_URL:-http://localhost:${API_PORT:-8080}}"
        info "Web running at http://localhost:${WEB_PORT:-3000}"
        npm run dev -- --port "${WEB_PORT:-3000}"
    ) &
    WEB_PID=$!

    info "Started server (PID: $SERVER_PID) and web (PID: $WEB_PID)."
    info "Press Ctrl+C to stop both services."

    cleanup() {
        warn "Shutting down all services..."
        kill "$SERVER_PID" "$WEB_PID" 2>/dev/null || true
        # Kill any child processes of the sub-shells too
        pkill -TERM -P "$SERVER_PID" 2>/dev/null || true
        pkill -TERM -P "$WEB_PID" 2>/dev/null || true
        wait "$SERVER_PID" "$WEB_PID" 2>/dev/null || true
        exit 0
    }
    trap cleanup INT TERM

    # Wait for both — if either exits, kill the other and exit
    wait "$SERVER_PID" || true
    warn "Server process stopped. Shutting down web..."
    kill "$WEB_PID" 2>/dev/null || true
    wait "$WEB_PID" 2>/dev/null || true
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
