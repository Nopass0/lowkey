#!/usr/bin/env bash
# =============================================================================
# Lowkey VPN Server — Quick Setup & Run Script
# =============================================================================
#
# Usage:
#   chmod +x server-setup.sh
#   sudo ./server-setup.sh          # first run: installs deps + creates .env
#   sudo ./server-setup.sh --run    # skip prompts, load .env, run the server
#   sudo ./server-setup.sh --build  # just rebuild, don't run
#
# What this script does:
#   1. Detect the Linux distribution and install required system packages.
#   2. Install the Rust toolchain (if not already installed).
#   3. Prompt for configuration and write a .env file (skipped if it exists).
#   4. Optionally set up a local PostgreSQL database.
#   5. Build the server in release mode.
#   6. Start the server (detached via nohup, or in the foreground).
#
# The server binary is written to:
#   vpn-server/target/release/vpn-server
#
# Logs (when running detached) are written to:
#   vpn-server.log
# =============================================================================

set -euo pipefail

# ── Colour helpers ────────────────────────────────────────────────────────────
RED='\033[0;31m'
GRN='\033[0;32m'
YEL='\033[1;33m'
BLU='\033[0;34m'
CYN='\033[0;36m'
RST='\033[0m'

info()    { echo -e "${BLU}[INFO]${RST}  $*"; }
ok()      { echo -e "${GRN}[ OK ]${RST}  $*"; }
warn()    { echo -e "${YEL}[WARN]${RST}  $*"; }
error()   { echo -e "${RED}[ERR ]${RST}  $*" >&2; }
section() { echo -e "\n${CYN}══════════════════════════════════════════${RST}"; \
             echo -e "${CYN}  $*${RST}"; \
             echo -e "${CYN}══════════════════════════════════════════${RST}"; }

# ── Locate script / project root ──────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
SERVER_DIR="$SCRIPT_DIR/vpn-server"
ENV_FILE="$SCRIPT_DIR/.env"
# Cargo workspace puts the binary in the workspace root target/, not the crate's target/
BINARY="$SCRIPT_DIR/target/release/vpn-server"
PID_FILE="$SCRIPT_DIR/vpn-server.pid"
WEB_PID_FILE="$SCRIPT_DIR/lowkey-web.pid"
WEB_LOG_FILE="$SCRIPT_DIR/lowkey-web.log"

# ── Parse CLI flags ───────────────────────────────────────────────────────────
MODE="setup"   # setup | run | build | stop | status
for arg in "$@"; do
    case "$arg" in
        --run)   MODE="run"   ;;
        --build) MODE="build" ;;
        --stop)  MODE="stop"  ;;
        --status) MODE="status" ;;
        --help|-h)
            echo "Usage: $0 [--run|--build|--stop|--status]"
            echo "  (no flag)  Full setup: install deps, configure, build, run"
            echo "  --build    Rebuild the server binary only"
            echo "  --run      Load .env and start the server (no prompts)"
            echo "  --stop     Stop a running detached server"
            echo "  --status   Show server health + connected peers"
            exit 0 ;;
    esac
done

# ── Stop mode ─────────────────────────────────────────────────────────────────
if [[ "$MODE" == "stop" ]]; then
    if [[ -f "$PID_FILE" ]]; then
        PID=$(cat "$PID_FILE")
        if kill -0 "$PID" 2>/dev/null; then
            kill "$PID"
            rm -f "$PID_FILE"
            ok "Server (PID $PID) stopped."
        else
            warn "PID $PID is not running. Removing stale pid file."
            rm -f "$PID_FILE"
        fi
    else
        warn "No PID file found at $PID_FILE. Is the server running?"
    fi

    if [[ -f "$WEB_PID_FILE" ]]; then
        WEB_PID=$(cat "$WEB_PID_FILE")
        if kill -0 "$WEB_PID" 2>/dev/null; then
            kill "$WEB_PID"
            ok "Web server (PID $WEB_PID) stopped."
        fi
        rm -f "$WEB_PID_FILE"
    fi
    exit 0
fi



# ── Status mode ───────────────────────────────────────────────────────────────
if [[ "$MODE" == "status" ]]; then
    if [[ -f "$ENV_FILE" ]]; then
        set -o allexport
        source "$ENV_FILE"
        set +o allexport
    fi

    API_PORT_V="${API_PORT:-8080}"
    section "Server status"
    if [[ -f "$PID_FILE" ]]; then
        PID=$(cat "$PID_FILE")
        if kill -0 "$PID" 2>/dev/null; then
            ok "vpn-server process is running (PID $PID)."
        else
            warn "PID file exists but process $PID is not running."
        fi
    else
        warn "No PID file found at $PID_FILE."
    fi

    info "HTTP: GET /api/status"
    curl -fsS "http://127.0.0.1:${API_PORT_V}/api/status" || true
    echo ""
    echo ""
    info "Peers: GET /api/peers"
    curl -fsS "http://127.0.0.1:${API_PORT_V}/api/peers" || true
    echo ""
    echo ""
    info "Realtime monitor: watch -n 1 'curl -fsS http://127.0.0.1:${API_PORT_V}/api/peers | jq . 2>/dev/null || curl -fsS http://127.0.0.1:${API_PORT_V}/api/peers'"
    exit 0
fi

# =============================================================================
# 1. ROOT CHECK
# =============================================================================
section "Privilege check"
if [[ $EUID -ne 0 ]]; then
    error "This script must be run as root (needed for TUN device + iptables)."
    error "Re-run with: sudo $0 $*"
    exit 1
fi
ok "Running as root."

# =============================================================================
# 2. SYSTEM DEPENDENCIES
# =============================================================================
section "System dependencies"

install_packages_apt() {
    apt-get update -qq
    apt-get install -y --no-install-recommends \
        build-essential curl git ca-certificates \
        postgresql postgresql-client \
        iproute2 iptables nginx certbot python3-certbot-nginx \
        nodejs npm
    ok "APT packages installed."
}

install_packages_yum() {
    yum install -y \
        gcc make curl git ca-certificates \
        postgresql-server postgresql \
        iproute iptables nginx certbot python3-certbot-nginx nodejs npm
    ok "YUM packages installed."
}

install_packages_dnf() {
    dnf install -y \
        gcc make curl git ca-certificates \
        postgresql-server postgresql \
        iproute iptables nginx certbot python3-certbot-nginx nodejs npm
    ok "DNF packages installed."
}

if command -v apt-get &>/dev/null; then
    info "Detected Debian/Ubuntu — using apt-get."
    install_packages_apt
elif command -v dnf &>/dev/null; then
    info "Detected Fedora/RHEL — using dnf."
    install_packages_dnf
elif command -v yum &>/dev/null; then
    info "Detected CentOS/RHEL — using yum."
    install_packages_yum
else
    warn "Unknown package manager. Skipping system package installation."
    warn "Make sure build-essential, postgresql, iproute2, and iptables are installed."
fi

install_npm_deps() {
    local dir="$1"
    cd "$dir"
    if [[ -f package-lock.json ]]; then
        info "Installing web dependencies via npm ci..."
        if npm ci --legacy-peer-deps; then
            return 0
        fi
        warn "npm ci failed, falling back to npm install..."
    else
        warn "package-lock.json not found in $dir, using npm install..."
    fi
    npm install --legacy-peer-deps
}

build_web_app() {
    if [[ "${ENABLE_WEB:-false}" != "true" ]]; then
        return 0
    fi
    section "Building Next.js web app"
    local web_dir="$SCRIPT_DIR/web"
    if [[ ! -d "$web_dir" ]]; then
        warn "web/ directory not found, skipping web setup."
        return 0
    fi
    if ! command -v node &>/dev/null || ! command -v npm &>/dev/null; then
        warn "Node.js/npm not found, skipping web setup."
        return 0
    fi
    install_npm_deps "$web_dir"
    NEXT_PUBLIC_API_URL="${NEXT_PUBLIC_API_URL:-http://127.0.0.1:${API_PORT:-8080}}" npm run build --prefix "$web_dir"
}

start_web_app() {
    if [[ "${ENABLE_WEB:-false}" != "true" ]]; then
        return 0
    fi
    local web_dir="$SCRIPT_DIR/web"
    local web_port="${WEB_PORT:-3000}"
    if [[ ! -f "$web_dir/.next/standalone/server.js" ]]; then
        warn "Web standalone build not found, skipping web launch."
        return 0
    fi
    if [[ -f "$WEB_PID_FILE" ]]; then
        local old_pid
        old_pid=$(cat "$WEB_PID_FILE")
        kill "$old_pid" 2>/dev/null || true
        rm -f "$WEB_PID_FILE"
    fi
    info "Starting web server on port ${web_port}..."
    nohup env PORT="$web_port" HOSTNAME="0.0.0.0"         NEXT_PUBLIC_API_URL="${NEXT_PUBLIC_API_URL:-http://127.0.0.1:${API_PORT:-8080}}"         node "$web_dir/.next/standalone/server.js" >> "$WEB_LOG_FILE" 2>&1 &
    echo $! > "$WEB_PID_FILE"
    ok "Web server started with PID $(cat "$WEB_PID_FILE")"
}

configure_nginx() {
    if [[ -z "${APP_DOMAIN:-}" ]]; then
        return 0
    fi
    section "Configuring Nginx reverse proxy"
    cat > /etc/nginx/sites-available/lowkey.conf <<NGINXEOF
server {
    listen 80;
    server_name ${APP_DOMAIN};

    location / {
        proxy_pass http://127.0.0.1:${WEB_PORT:-3000};
        proxy_http_version 1.1;
        proxy_set_header Host \$host;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
    }

    location /api/ {
        proxy_pass http://127.0.0.1:${API_PORT:-8080};
        proxy_http_version 1.1;
        proxy_set_header Host \$host;
        proxy_set_header X-Forwarded-For \$proxy_add_x_forwarded_for;
        proxy_set_header X-Forwarded-Proto \$scheme;
    }
}
NGINXEOF

    ln -sfn /etc/nginx/sites-available/lowkey.conf /etc/nginx/sites-enabled/lowkey.conf
    rm -f /etc/nginx/sites-enabled/default
    nginx -t && systemctl restart nginx && systemctl enable nginx >/dev/null 2>&1 || warn "Failed to restart nginx"

    if [[ "${SSL_MODE:-none}" == "letsencrypt" ]]; then
        if command -v certbot &>/dev/null; then
            certbot --nginx -d "${APP_DOMAIN}" --non-interactive --agree-tos -m "${LETSENCRYPT_EMAIL:-admin@${APP_DOMAIN}}" --redirect || warn "Certbot failed"
        else
            warn "certbot not installed; SSL skipped."
        fi
    elif [[ "${SSL_MODE:-existing}" == "existing" && -n "${SSL_CERT_PATH:-}" && -n "${SSL_KEY_PATH:-}" ]]; then
        warn "Existing cert mode selected. Place SSL directives manually in /etc/nginx/sites-available/lowkey.conf"
    fi
}

# =============================================================================
# 3. RUST TOOLCHAIN
# =============================================================================
section "Rust toolchain"

# Try to use the invoking user's Rust installation first (via sudo -u)
REAL_USER="${SUDO_USER:-root}"
REAL_HOME=$(getent passwd "$REAL_USER" | cut -d: -f6)
CARGO_BIN="$REAL_HOME/.cargo/bin/cargo"

if [[ -x "$CARGO_BIN" ]]; then
    ok "Rust already installed for user $REAL_USER."
elif command -v cargo &>/dev/null; then
    CARGO_BIN=$(command -v cargo)
    ok "Rust found at $CARGO_BIN."
else
    info "Installing Rust via rustup for user $REAL_USER..."
    su - "$REAL_USER" -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path'
    CARGO_BIN="$REAL_HOME/.cargo/bin/cargo"
    ok "Rust installed."
fi

export PATH="$REAL_HOME/.cargo/bin:$PATH"

# =============================================================================
# 4. CONFIGURE .env
# =============================================================================
if [[ "$MODE" == "setup" ]]; then
section "Configuration (.env)"

if [[ -f "$ENV_FILE" ]]; then
    warn ".env file already exists at $ENV_FILE"
    read -rp "  Overwrite it? [y/N] " OVERWRITE
    if [[ ! "$OVERWRITE" =~ ^[Yy]$ ]]; then
        info "Keeping existing .env file."
        source "$ENV_FILE" 2>/dev/null || true
    else
        rm -f "$ENV_FILE"
    fi
fi

if [[ ! -f "$ENV_FILE" ]]; then
    echo ""
    echo -e "${YEL}Please answer the following configuration questions.${RST}"
    echo -e "${YEL}Press Enter to accept the default shown in brackets.${RST}"
    echo ""

    # ── Database URL ────────────────────────────────────────────────────────
    echo -e "${BLU}PostgreSQL database URL${RST}"
    echo "  Format: postgres://user:password@host/dbname"
    echo "  Leave blank to set up a local PostgreSQL database automatically."
    read -rp "  DATABASE_URL [auto-create local]: " DB_URL

    if [[ -z "$DB_URL" ]]; then
        info "Setting up a local PostgreSQL database..."

        DB_NAME="lowkey"
        DB_USER="lowkey"
        DB_PASS="$(head -c 24 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 20)"

        # Start PostgreSQL if not running
        if command -v systemctl &>/dev/null && systemctl list-units --type=service | grep -q postgresql; then
            systemctl enable --now postgresql 2>/dev/null || true
        elif command -v pg_ctlcluster &>/dev/null; then
            # Debian-style: find the latest cluster version
            PG_VER=$(pg_lsclusters -h | awk '{print $1}' | sort -V | tail -1)
            pg_ctlcluster "$PG_VER" main start 2>/dev/null || true
        fi

        # Give PostgreSQL a moment to start
        sleep 2

        # Create user and database (ignore errors if they already exist)
        su - postgres -c "psql -tc \"SELECT 1 FROM pg_roles WHERE rolname='$DB_USER'\" | grep -q 1 || \
            psql -c \"CREATE USER $DB_USER WITH PASSWORD '$DB_PASS'\"" 2>/dev/null || true
        su - postgres -c "psql -tc \"SELECT 1 FROM pg_database WHERE datname='$DB_NAME'\" | grep -q 1 || \
            psql -c \"CREATE DATABASE $DB_NAME OWNER $DB_USER\"" 2>/dev/null || true

        DB_URL="postgres://$DB_USER:$DB_PASS@localhost/$DB_NAME"
        ok "Local database created: $DB_URL"
    fi

    # ── JWT secret ──────────────────────────────────────────────────────────
    JWT_DEFAULT="$(head -c 48 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 48)"
    read -rp "  JWT_SECRET [random: ${JWT_DEFAULT:0:12}...]: " JWT_SECRET
    JWT_SECRET="${JWT_SECRET:-$JWT_DEFAULT}"

    # ── Pre-shared key ──────────────────────────────────────────────────────
    PSK_DEFAULT="$(head -c 32 /dev/urandom | base64 | tr -dc 'a-zA-Z0-9' | head -c 32)"
    read -rp "  VPN_PSK (tunnel pre-shared key) [random: ${PSK_DEFAULT:0:8}...]: " VPN_PSK
    VPN_PSK="${VPN_PSK:-$PSK_DEFAULT}"

    # ── Ports ───────────────────────────────────────────────────────────────
    read -rp "  API_PORT (HTTP API) [8080]: " API_PORT
    API_PORT="${API_PORT:-8080}"

    read -rp "  UDP_PORT (VPN tunnel) [51820]: " UDP_PORT
    UDP_PORT="${UDP_PORT:-51820}"

    read -rp "  PROXY_PORT (TCP proxy) [8388]: " PROXY_PORT
    PROXY_PORT="${PROXY_PORT:-8388}"

    # ── Telegram (optional) ─────────────────────────────────────────────────
    echo ""
    echo -e "${BLU}Telegram bot (optional — for admin OTP login)${RST}"
    echo "  Leave blank to skip Telegram integration."
    read -rp "  TG_BOT_TOKEN: " TG_BOT_TOKEN
    read -rp "  TG_ADMIN_CHAT_ID: " TG_ADMIN_CHAT_ID

    # ── Tochka Bank SBP (optional) ───────────────────────────────────────────
    echo ""
    echo -e "${BLU}Tochka Bank SBP API (optional — for real SBP payments)${RST}"
    echo "  Leave blank to use fallback NSPK QR links (no auto-confirmation)."
    read -rp "  TOCHKA_JWT (JWT token): " TOCHKA_JWT
    read -rp "  TOCHKA_MERCHANT_ID: " TOCHKA_MERCHANT_ID
    read -rp "  TOCHKA_LEGAL_ID: " TOCHKA_LEGAL_ID

    echo ""
    echo -e "${BLU}Web/Nginx setup (optional)${RST}"
    read -rp "  Setup and run web dashboard too? [Y/n]: " ENABLE_WEB_IN
    if [[ -z "$ENABLE_WEB_IN" || "$ENABLE_WEB_IN" =~ ^[Yy]$ ]]; then
        ENABLE_WEB=true
    else
        ENABLE_WEB=false
    fi

    if [[ "$ENABLE_WEB" == "true" ]]; then
        read -rp "  WEB_PORT [3000]: " WEB_PORT
        WEB_PORT="${WEB_PORT:-3000}"
        read -rp "  Domain for nginx (leave blank to skip nginx): " APP_DOMAIN
        read -rp "  NEXT_PUBLIC_API_URL [http://127.0.0.1:${API_PORT}]: " NEXT_PUBLIC_API_URL
        NEXT_PUBLIC_API_URL="${NEXT_PUBLIC_API_URL:-http://127.0.0.1:${API_PORT}}"
        if [[ -n "$APP_DOMAIN" ]]; then
            read -rp "  SSL mode [letsencrypt/existing/none] (default: letsencrypt): " SSL_MODE
            SSL_MODE="${SSL_MODE:-letsencrypt}"
            if [[ "$SSL_MODE" == "letsencrypt" ]]; then
                read -rp "  Let's Encrypt email [admin@${APP_DOMAIN}]: " LETSENCRYPT_EMAIL
                LETSENCRYPT_EMAIL="${LETSENCRYPT_EMAIL:-admin@${APP_DOMAIN}}"
            elif [[ "$SSL_MODE" == "existing" ]]; then
                read -rp "  SSL cert path: " SSL_CERT_PATH
                read -rp "  SSL key path: " SSL_KEY_PATH
            fi
        fi
    fi

    # ── Write .env ──────────────────────────────────────────────────────────
    cat > "$ENV_FILE" <<ENVEOF
# Lowkey VPN Server — environment configuration
# Generated by server-setup.sh on $(date -u '+%Y-%m-%d %H:%M:%S UTC')

DATABASE_URL="$DB_URL"

JWT_SECRET="$JWT_SECRET"

VPN_PSK="$VPN_PSK"

# Ports (can also be passed as CLI flags: --api-port, --udp-port, --proxy-port)
API_PORT=$API_PORT
UDP_PORT=$UDP_PORT
PROXY_PORT=$PROXY_PORT

ENABLE_WEB=${ENABLE_WEB:-false}
WEB_PORT=${WEB_PORT:-3000}
APP_DOMAIN="${APP_DOMAIN:-}"
SSL_MODE="${SSL_MODE:-none}"
SSL_CERT_PATH="${SSL_CERT_PATH:-}"
SSL_KEY_PATH="${SSL_KEY_PATH:-}"
LETSENCRYPT_EMAIL="${LETSENCRYPT_EMAIL:-}"
NEXT_PUBLIC_API_URL="${NEXT_PUBLIC_API_URL:-http://127.0.0.1:$API_PORT}"

ENVEOF

    if [[ -n "$TG_BOT_TOKEN" ]]; then
        echo "TG_BOT_TOKEN=\"$TG_BOT_TOKEN\"" >> "$ENV_FILE"
    fi

    if [[ -n "$TG_ADMIN_CHAT_ID" ]]; then
        echo "TG_ADMIN_CHAT_ID=\"$TG_ADMIN_CHAT_ID\"" >> "$ENV_FILE"
    fi

    if [[ -n "$TOCHKA_JWT" ]]; then
        echo "TOCHKA_JWT=\"$TOCHKA_JWT\"" >> "$ENV_FILE"
    fi

    if [[ -n "$TOCHKA_MERCHANT_ID" ]]; then
        echo "TOCHKA_MERCHANT_ID=\"$TOCHKA_MERCHANT_ID\"" >> "$ENV_FILE"
    fi

    if [[ -n "$TOCHKA_LEGAL_ID" ]]; then
        echo "TOCHKA_LEGAL_ID=\"$TOCHKA_LEGAL_ID\"" >> "$ENV_FILE"
    fi

    chmod 600 "$ENV_FILE"
    ok ".env written to $ENV_FILE"
fi   # end: if [[ ! -f "$ENV_FILE" ]]
fi   # end: if [[ "$MODE" == "setup" ]]

# =============================================================================
# 5. BUILD
# =============================================================================
section "Building vpn-server (release)"

# Build from workspace root so Cargo resolves the workspace correctly.
# The binary lands at <workspace>/target/release/vpn-server.
cd "$SCRIPT_DIR"

BUILD_CMD="$CARGO_BIN build --release -p vpn-server"
info "Running: $BUILD_CMD"
info "(this may take several minutes on the first run)"

# Run as the real user if possible (keeps the build cache in their home)
if [[ "$REAL_USER" != "root" ]]; then
    su - "$REAL_USER" -c "cd '$SCRIPT_DIR' && PATH='$REAL_HOME/.cargo/bin:$PATH' $BUILD_CMD"
else
    eval "$BUILD_CMD"
fi

if [[ ! -f "$BINARY" ]]; then
    error "Binary not found after build: $BINARY"
    error "Check the cargo output above for compile errors."
    exit 1
fi
ok "Build complete: $BINARY"

# =============================================================================
# 6. RUN
# =============================================================================
if [[ "$MODE" == "build" ]]; then
    ok "Build-only mode — not starting the server."
    exit 0
fi

section "Starting vpn-server"

# Load environment variables from .env
if [[ -f "$ENV_FILE" ]]; then
    info "Loading $ENV_FILE"
    # Export all VAR=VALUE lines, ignoring comments and blank lines
    set -o allexport
    # shellcheck source=/dev/null
    source "$ENV_FILE"
    set +o allexport
else
    error ".env not found at $ENV_FILE — run without --run to configure first."
    exit 1
fi

# Stop any existing server process
if [[ -f "$PID_FILE" ]]; then
    OLD_PID=$(cat "$PID_FILE")
    if kill -0 "$OLD_PID" 2>/dev/null; then
        info "Stopping previous server (PID $OLD_PID)..."
        kill "$OLD_PID"
        sleep 1
    fi
    rm -f "$PID_FILE"
fi

# =============================================================================
# 5b. OPEN FIREWALL PORTS
# =============================================================================
section "Opening firewall ports"

open_ports() {
    local api_p="${API_PORT:-8080}"
    local udp_p="${UDP_PORT:-51820}"
    local prx_p="${PROXY_PORT:-8388}"
    local ssh_p="${SSH_PORT:-22}"

    if command -v ufw &>/dev/null && ufw status | grep -q "Status: active"; then
        info "ufw detected — opening ports..."
        ufw allow "${ssh_p}/tcp"  comment "SSH management"   2>/dev/null && ok "ufw: ${ssh_p}/tcp open"  || warn "ufw rule for ${ssh_p}/tcp failed (may already exist)"
        ufw allow "${api_p}/tcp"  comment "Lowkey API"       2>/dev/null && ok "ufw: ${api_p}/tcp open"  || warn "ufw rule for ${api_p}/tcp failed (may already exist)"
        ufw allow "${udp_p}/udp"  comment "Lowkey VPN tunnel" 2>/dev/null && ok "ufw: ${udp_p}/udp open"  || warn "ufw rule for ${udp_p}/udp failed"
        ufw allow "${prx_p}/tcp"  comment "Lowkey proxy"     2>/dev/null && ok "ufw: ${prx_p}/tcp open"  || warn "ufw rule for ${prx_p}/tcp failed"
    else
        info "Using iptables INPUT rules (no active ufw found)..."

        # Keep existing SSH sessions alive.
        iptables -C INPUT -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT 2>/dev/null \
            || iptables -I INPUT 1 -m conntrack --ctstate ESTABLISHED,RELATED -j ACCEPT

        for rule_args in \
            "-p tcp --dport ${ssh_p} -j ACCEPT" \
            "-p tcp --dport ${api_p} -j ACCEPT" \
            "-p udp --dport ${udp_p} -j ACCEPT" \
            "-p tcp --dport ${prx_p} -j ACCEPT"
        do
            # Remove duplicate first (idempotent), then insert at position 1
            # shellcheck disable=SC2086
            iptables -D INPUT $rule_args 2>/dev/null || true
            # shellcheck disable=SC2086
            iptables -I INPUT 1 $rule_args
        done
        ok "iptables: ports ${ssh_p}/tcp, ${api_p}/tcp, ${udp_p}/udp, ${prx_p}/tcp opened."

        # Persist rules so they survive a reboot
        if command -v iptables-save &>/dev/null; then
            if command -v netfilter-persistent &>/dev/null; then
                netfilter-persistent save 2>/dev/null || true
            elif [[ -d /etc/iptables ]]; then
                iptables-save > /etc/iptables/rules.v4 2>/dev/null || true
            fi
        fi
    fi
}

open_ports

build_web_app
start_web_app
configure_nginx

# Build the argument list from environment variables
SERVER_ARGS=(
    "--api-port"   "${API_PORT:-8080}"
    "--udp-port"   "${UDP_PORT:-51820}"
    "--proxy-port" "${PROXY_PORT:-8388}"
    "--no-tui"
)

if [[ -n "${TOCHKA_JWT:-}" ]]; then
    SERVER_ARGS+=("--tochka-jwt" "$TOCHKA_JWT")
fi
if [[ -n "${TOCHKA_MERCHANT_ID:-}" ]]; then
    SERVER_ARGS+=("--tochka-merchant-id" "$TOCHKA_MERCHANT_ID")
fi
if [[ -n "${TOCHKA_LEGAL_ID:-}" ]]; then
    SERVER_ARGS+=("--tochka-legal-id" "$TOCHKA_LEGAL_ID")
fi

echo ""
echo -e "${GRN}╔══════════════════════════════════════════════╗${RST}"
echo -e "${GRN}║        Lowkey VPN Server Starting            ║${RST}"
echo -e "${GRN}╠══════════════════════════════════════════════╣${RST}"
echo -e "${GRN}║${RST}  API port  : ${API_PORT:-8080}                            ${GRN}║${RST}"
echo -e "${GRN}║${RST}  UDP port  : ${UDP_PORT:-51820}                          ${GRN}║${RST}"
echo -e "${GRN}║${RST}  Proxy port: ${PROXY_PORT:-8388}                          ${GRN}║${RST}"
echo -e "${GRN}╠══════════════════════════════════════════════╣${RST}"
echo -e "${GRN}║${RST}  Logs → vpn-server.log                       ${GRN}║${RST}"
echo -e "${GRN}║${RST}  Stop → sudo $0 --stop                ${GRN}║${RST}"
echo -e "${GRN}╚══════════════════════════════════════════════╝${RST}"
echo ""

# Launch detached with nohup; record PID for --stop
nohup "$BINARY" "${SERVER_ARGS[@]}" \
    >> "$SCRIPT_DIR/vpn-server.log" 2>&1 &

SERVER_PID=$!
echo "$SERVER_PID" > "$PID_FILE"
ok "Server started with PID $SERVER_PID."
info "Tail logs: tail -f $SCRIPT_DIR/vpn-server.log"

# =============================================================================
# 6b. VERIFY SERVER IS ACTUALLY LISTENING
# =============================================================================
section "Verifying server health"

API_PORT_V="${API_PORT:-8080}"
SERVER_UP=false

info "Waiting up to 15s for the server to accept connections on :${API_PORT_V} ..."
for i in $(seq 1 15); do
    STATUS_HTTP=$(curl -s -o /dev/null -w "%{http_code}" \
        "http://127.0.0.1:${API_PORT_V}/api/status" --max-time 2 2>/dev/null || true)
    if [[ "$STATUS_HTTP" == "200" ]]; then
        SERVER_UP=true
        ok "Server is UP and answering on 127.0.0.1:${API_PORT_V} (HTTP $STATUS_HTTP)."
        break
    fi
    sleep 1
done

if [[ "$SERVER_UP" == "false" ]]; then
    error "Server did NOT start within 15 seconds!"
    error "Last ${API_PORT_V} curl HTTP code: ${STATUS_HTTP:-none}"
    echo ""
    warn "=== Last 30 lines of vpn-server.log ==="
    tail -30 "$SCRIPT_DIR/vpn-server.log" 2>/dev/null || true
    echo ""
    warn "Is the process still alive?"
    ps -p "$SERVER_PID" -o pid,stat,cmd 2>/dev/null || echo "  (process $SERVER_PID not found)"
    echo ""
    warn "Listening sockets on :${API_PORT_V}:"
    ss -tlnp "sport = :${API_PORT_V}" 2>/dev/null || netstat -tlnp 2>/dev/null | grep "${API_PORT_V}" || true
    echo ""
    error "Fix the issues above and re-run: sudo $0 --run"
    exit 1
fi

echo ""
if [[ "${ENABLE_WEB:-false}" == "true" ]]; then
    ok "Web URL (local): http://127.0.0.1:${WEB_PORT:-3000}"
    if [[ -n "${APP_DOMAIN:-}" ]]; then
        ok "Web URL (domain): https://${APP_DOMAIN}"
    fi
fi
warn "------------------------------------------------------------------------"
warn "  CLOUD FIREWALL REMINDER"
warn "------------------------------------------------------------------------"
warn "  iptables rules are now open, but many cloud providers also have a"
warn "  SEPARATE network-level firewall (security groups, VPC firewall rules)."
warn "  Make sure the following ports are allowed INBOUND in your cloud panel:"
warn ""
warn "    ${API_PORT_V}/tcp  -- HTTP API (required for client setup)"
warn "    ${UDP_PORT:-51820}/udp  -- VPN tunnel"
warn "    ${PROXY_PORT:-8388}/tcp  -- TCP proxy / SOCKS5"
warn ""
warn "  Yandex Cloud: VPC -> Security Groups -> add inbound rules"
warn "  Hetzner     : Firewall -> add inbound rules"
warn "  AWS EC2     : Security Group -> Inbound rules"
warn "  DigitalOcean: Networking -> Firewalls -> Inbound rules"
warn "------------------------------------------------------------------------"
echo ""

# =============================================================================
# 7. CREATE TRIAL PROMO CODE
# =============================================================================
section "Creating trial promo code"

TRIAL_CODE="TRIAL30"
TRIAL_CREATED=false

# Insert the trial code directly via psql if available.
# The INSERT ... ON CONFLICT DO NOTHING makes it idempotent.
if command -v psql &>/dev/null && [[ -n "${DATABASE_URL:-}" ]]; then
    psql "$DATABASE_URL" -c "
        INSERT INTO promo_codes
            (code, \"type\", value, extra, max_uses, used_count)
        VALUES
            ('$TRIAL_CODE', 'free_days', 30, 0, 9999, 0)
        ON CONFLICT (code) DO NOTHING;
    " 2>/dev/null && TRIAL_CREATED=true
fi

echo ""
echo -e "${YEL}╔══════════════════════════════════════════════════════╗${RST}"
echo -e "${YEL}║              TRIAL PROMO CODE                        ║${RST}"
echo -e "${YEL}╠══════════════════════════════════════════════════════╣${RST}"
echo -e "${YEL}║${RST}  Code  : ${GRN}$TRIAL_CODE${RST}                               ${YEL}║${RST}"
echo -e "${YEL}║${RST}  Effect: 30 free VPN days                            ${YEL}║${RST}"
echo -e "${YEL}║${RST}  Uses  : unlimited                                   ${YEL}║${RST}"
echo -e "${YEL}╠══════════════════════════════════════════════════════╣${RST}"
echo -e "${YEL}║${RST}  Share this code with users during client setup.     ${YEL}║${RST}"
echo -e "${YEL}║${RST}  Run client-setup.sh on the client machine.          ${YEL}║${RST}"
echo -e "${YEL}╚══════════════════════════════════════════════════════╝${RST}"
echo ""

if [[ "$TRIAL_CREATED" == "false" ]]; then
    warn "Could not auto-insert the promo code via psql."
    warn "Run this SQL manually on the server database:"
    echo "  INSERT INTO promo_codes (code, \"type\", value, extra, max_uses, used_count)"
    echo "  VALUES ('$TRIAL_CODE', 'free_days', 30, 0, 9999, 0) ON CONFLICT DO NOTHING;"
fi
