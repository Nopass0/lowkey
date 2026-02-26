#!/usr/bin/env bash
# =============================================================================
# Lowkey VPN Client — Quick Setup & Connect Script
# =============================================================================
#
# Usage:
#   chmod +x client-setup.sh
#   sudo ./client-setup.sh             # first run: build + register + connect
#   sudo ./client-setup.sh --connect   # reconnect with saved config
#   sudo ./client-setup.sh --build     # rebuild only
#   sudo ./client-setup.sh --status    # show account & subscription info
#   ./client-setup.sh --socks5         # connect in SOCKS5 mode (no root needed)
#
# What this script does:
#   1. Detect the OS and install required system packages.
#   2. Install the Rust toolchain (if not already installed).
#   3. Build vpn-client in release mode.
#   4. Prompt for server address, login and password.
#   5. Register a new account OR log in to an existing one.
#   6. Apply a promo / trial code to activate a free subscription.
#   7. Show the subscription status and plan info.
#   8. Connect to the VPN (TUN mode by default, SOCKS5 with --socks5).
#
# Config is saved to ~/.config/lowkey/client.conf so subsequent runs don't
# repeat all prompts.
# =============================================================================

set -euo pipefail

# ── Colour helpers ────────────────────────────────────────────────────────────
RED='\033[0;31m'
GRN='\033[0;32m'
YEL='\033[1;33m'
BLU='\033[0;34m'
CYN='\033[0;36m'
MAG='\033[0;35m'
RST='\033[0m'

info()    { echo -e "${BLU}[INFO]${RST}  $*"; }
ok()      { echo -e "${GRN}[ OK ]${RST}  $*"; }
warn()    { echo -e "${YEL}[WARN]${RST}  $*"; }
error()   { echo -e "${RED}[ERR ]${RST}  $*" >&2; }
section() { echo -e "\n${CYN}══════════════════════════════════════════${RST}"; \
             echo -e "${CYN}  $*${RST}"; \
             echo -e "${CYN}══════════════════════════════════════════${RST}"; }

# ── Locate directories ────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLIENT_DIR="$SCRIPT_DIR/vpn-client"
BINARY="$CLIENT_DIR/target/release/vpn-client"

# Config / session stored in ~/.config/lowkey/
REAL_USER="${SUDO_USER:-$USER}"
REAL_HOME=$(getent passwd "$REAL_USER" | cut -d: -f6)
CONF_DIR="$REAL_HOME/.config/lowkey"
CONF_FILE="$CONF_DIR/client.conf"
SESSION_FILE="$CONF_DIR/session.json"

# ── Parse CLI flags ───────────────────────────────────────────────────────────
MODE="setup"   # setup | connect | build | status | socks5
for arg in "$@"; do
    case "$arg" in
        --connect) MODE="connect"  ;;
        --build)   MODE="build"    ;;
        --status)  MODE="status"   ;;
        --socks5)  MODE="socks5"   ;;
        --help|-h)
            echo "Usage: $0 [--connect|--build|--status|--socks5]"
            echo "  (no flag)   Full setup: build + register/login + connect"
            echo "  --connect   Reconnect using saved credentials (TUN mode)"
            echo "  --socks5    Connect via SOCKS5 proxy (port 1080, no root needed)"
            echo "  --status    Show current account and subscription info"
            echo "  --build     Rebuild the client binary only"
            exit 0 ;;
    esac
done

# =============================================================================
# HELPERS — thin wrappers around curl for the VPN API
# =============================================================================

# api_post <server> <port> <path> <json_body>
# Returns the HTTP response body (exits on HTTP error ≥ 400)
api_post() {
    local server="$1" port="$2" path="$3" body="$4"
    local url="http://${server}:${port}${path}"
    local out http_code
    out=$(curl -s -w '\n%{http_code}' \
        -H 'Content-Type: application/json' \
        -d "$body" "$url" 2>/dev/null)
    http_code=$(echo "$out" | tail -1)
    body_out=$(echo "$out" | head -n -1)
    if [[ "$http_code" -ge 400 ]]; then
        echo -e "${RED}HTTP $http_code${RST}: $body_out" >&2
        return 1
    fi
    echo "$body_out"
}

# api_post_auth <server> <port> <path> <token> <json_body>
api_post_auth() {
    local server="$1" port="$2" path="$3" token="$4" body="$5"
    local url="http://${server}:${port}${path}"
    local out http_code
    out=$(curl -s -w '\n%{http_code}' \
        -H 'Content-Type: application/json' \
        -H "Authorization: Bearer $token" \
        -d "$body" "$url" 2>/dev/null)
    http_code=$(echo "$out" | tail -1)
    body_out=$(echo "$out" | head -n -1)
    if [[ "$http_code" -ge 400 ]]; then
        echo -e "${RED}HTTP $http_code${RST}: $body_out" >&2
        return 1
    fi
    echo "$body_out"
}

# api_get_auth <server> <port> <path> <token>
api_get_auth() {
    local server="$1" port="$2" path="$3" token="$4"
    local url="http://${server}:${port}${path}"
    local out http_code
    out=$(curl -s -w '\n%{http_code}' \
        -H "Authorization: Bearer $token" \
        "$url" 2>/dev/null)
    http_code=$(echo "$out" | tail -1)
    body_out=$(echo "$out" | head -n -1)
    if [[ "$http_code" -ge 400 ]]; then
        echo -e "${RED}HTTP $http_code${RST}: $body_out" >&2
        return 1
    fi
    echo "$body_out"
}

# json_field <json_string> <field_name>
# Extracts a top-level JSON field value using sed (no jq required)
json_field() {
    local json="$1" field="$2"
    echo "$json" | sed -n "s/.*\"$field\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p" | head -1
}

# json_field_num <json_string> <field_name>
json_field_num() {
    local json="$1" field="$2"
    echo "$json" | sed -n "s/.*\"$field\"[[:space:]]*:[[:space:]]*\([0-9.]*\).*/\1/p" | head -1
}

# ── Load saved config ─────────────────────────────────────────────────────────
load_conf() {
    if [[ -f "$CONF_FILE" ]]; then
        # shellcheck source=/dev/null
        source "$CONF_FILE"
    fi
}

save_conf() {
    mkdir -p "$CONF_DIR"
    cat > "$CONF_FILE" <<CONF
# Lowkey VPN Client — saved configuration
SERVER_ADDR="${SERVER_ADDR:-}"
API_PORT="${API_PORT:-8080}"
UDP_PORT="${UDP_PORT:-51820}"
PROXY_PORT="${PROXY_PORT:-8388}"
SOCKS_PORT="${SOCKS_PORT:-1080}"
CONF
    chmod 600 "$CONF_FILE"
}

load_conf

# =============================================================================
# 1. PRIVILEGE CHECK
# =============================================================================
section "Privilege check"

if [[ "$MODE" == "socks5" || "$MODE" == "status" || "$MODE" == "build" ]]; then
    # SOCKS5, status and build don't need root
    info "Mode '$MODE' — root not required."
elif [[ $EUID -ne 0 ]]; then
    warn "TUN mode requires root. Re-run with: sudo $0 $*"
    warn "Or use SOCKS5 mode (no root): $0 --socks5"
    exit 1
fi

# =============================================================================
# 2. SYSTEM DEPENDENCIES
# =============================================================================
section "System dependencies"

install_packages_apt() {
    apt-get update -qq
    apt-get install -y --no-install-recommends \
        build-essential curl git ca-certificates \
        iproute2 iptables
}

install_packages_yum() {
    yum install -y gcc make curl git ca-certificates iproute iptables
}

install_packages_dnf() {
    dnf install -y gcc make curl git ca-certificates iproute iptables
}

if [[ $EUID -eq 0 ]]; then
    if command -v apt-get &>/dev/null; then
        info "Detected Debian/Ubuntu — installing packages."
        install_packages_apt
    elif command -v dnf &>/dev/null; then
        info "Detected Fedora/RHEL — installing packages."
        install_packages_dnf
    elif command -v yum &>/dev/null; then
        info "Detected CentOS/RHEL — installing packages."
        install_packages_yum
    else
        warn "Unknown package manager — skipping system package installation."
    fi
    ok "System packages ready."
else
    info "Not running as root — skipping package installation."
fi

# =============================================================================
# 3. RUST TOOLCHAIN
# =============================================================================
section "Rust toolchain"

CARGO_BIN="$REAL_HOME/.cargo/bin/cargo"

if [[ -x "$CARGO_BIN" ]]; then
    ok "Rust found at $CARGO_BIN."
elif command -v cargo &>/dev/null; then
    CARGO_BIN=$(command -v cargo)
    ok "Rust found at $CARGO_BIN."
else
    info "Installing Rust via rustup for user $REAL_USER..."
    if [[ $EUID -eq 0 && "$REAL_USER" != "root" ]]; then
        su - "$REAL_USER" -c 'curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path'
    else
        curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path
    fi
    CARGO_BIN="$REAL_HOME/.cargo/bin/cargo"
    ok "Rust installed."
fi

export PATH="$REAL_HOME/.cargo/bin:$PATH"

# =============================================================================
# 4. BUILD
# =============================================================================
section "Building vpn-client (release)"

BUILD_CMD="$CARGO_BIN build --release"
info "Running: $BUILD_CMD"
info "(first build may take a few minutes)"

if [[ $EUID -eq 0 && "$REAL_USER" != "root" ]]; then
    su - "$REAL_USER" -c "cd '$CLIENT_DIR' && PATH='$REAL_HOME/.cargo/bin:$PATH' $BUILD_CMD"
else
    (cd "$CLIENT_DIR" && eval "$BUILD_CMD")
fi

ok "Build complete: $BINARY"

if [[ "$MODE" == "build" ]]; then
    ok "Build-only mode — done."
    exit 0
fi

# =============================================================================
# 5. CONFIGURE SERVER CONNECTION
# =============================================================================
section "Server configuration"

echo ""
echo -e "${YEL}Enter the VPN server details.${RST}"
echo -e "${YEL}Press Enter to keep existing/default values.${RST}"
echo ""

# Server address
CUR_SERVER="${SERVER_ADDR:-}"
if [[ -n "$CUR_SERVER" ]]; then
    read -rp "  Server IP or hostname [${CUR_SERVER}]: " INPUT_SERVER
    SERVER_ADDR="${INPUT_SERVER:-$CUR_SERVER}"
else
    while true; do
        read -rp "  Server IP or hostname (required): " SERVER_ADDR
        [[ -n "$SERVER_ADDR" ]] && break
        error "Server address is required."
    done
fi

# Check server is reachable
info "Checking server connectivity at $SERVER_ADDR..."
STATUS_RESP=$(curl -s --max-time 5 \
    "http://${SERVER_ADDR}:${API_PORT:-8080}/api/status" 2>/dev/null || true)

if [[ -z "$STATUS_RESP" ]]; then
    warn "Could not reach http://${SERVER_ADDR}:${API_PORT:-8080}/api/status"
    read -rp "  API port [${API_PORT:-8080}]: " INPUT_API_PORT
    API_PORT="${INPUT_API_PORT:-${API_PORT:-8080}}"

    STATUS_RESP=$(curl -s --max-time 5 \
        "http://${SERVER_ADDR}:${API_PORT}/api/status" 2>/dev/null || true)
    if [[ -z "$STATUS_RESP" ]]; then
        error "Still cannot reach the server. Check the IP/port and firewall rules."
        exit 1
    fi
else
    API_PORT="${API_PORT:-8080}"
fi

ok "Server is reachable."

# Read the advertised ports from the status response
ADVERTISED_UDP=$(json_field_num "$STATUS_RESP" "udp_port")
ADVERTISED_PROXY=$(json_field_num "$STATUS_RESP" "proxy_port")
UDP_PORT="${UDP_PORT:-${ADVERTISED_UDP:-51820}}"
PROXY_PORT="${PROXY_PORT:-${ADVERTISED_PROXY:-8388}}"
SOCKS_PORT="${SOCKS_PORT:-1080}"

info "UDP port  : $UDP_PORT"
info "Proxy port: $PROXY_PORT"

save_conf

# =============================================================================
# 6. ACCOUNT — REGISTER OR LOGIN
# =============================================================================
section "Account setup"

# Check if a valid session already exists
EXISTING_TOKEN=""
if [[ -f "$SESSION_FILE" ]]; then
    EXISTING_TOKEN=$(python3 -c "import json,sys; d=json.load(open('$SESSION_FILE')); print(d.get('token',''))" 2>/dev/null || \
                     grep -o '"token":"[^"]*"' "$SESSION_FILE" | sed 's/"token":"//;s/"//' 2>/dev/null || true)
fi

if [[ -n "$EXISTING_TOKEN" ]]; then
    # Validate the existing token with /auth/me
    ME_RESP=$(api_get_auth "$SERVER_ADDR" "$API_PORT" "/auth/me" "$EXISTING_TOKEN" 2>/dev/null || true)
    if [[ -n "$ME_RESP" ]] && echo "$ME_RESP" | grep -q '"login"'; then
        EXISTING_LOGIN=$(json_field "$ME_RESP" "login")
        ok "Already logged in as '${EXISTING_LOGIN}'."
        TOKEN="$EXISTING_TOKEN"
        ACCOUNT_RESP="$ME_RESP"
        LOGGED_IN=true
    else
        warn "Saved session token is invalid or expired — please log in again."
        LOGGED_IN=false
    fi
else
    LOGGED_IN=false
fi

if [[ "$LOGGED_IN" == "false" ]]; then
    echo ""
    echo -e "${BLU}Do you have an existing account on this server?${RST}"
    read -rp "  [y/N] " HAS_ACCOUNT

    echo ""
    read -rp "  Login (username): " LOGIN_NAME
    read -rsp "  Password: " LOGIN_PASS
    echo ""

    if [[ "$HAS_ACCOUNT" =~ ^[Yy]$ ]]; then
        # ── Login ─────────────────────────────────────────────────────────────
        info "Logging in as '$LOGIN_NAME'..."
        AUTH_RESP=$(api_post "$SERVER_ADDR" "$API_PORT" "/auth/login" \
            "{\"login\":\"$LOGIN_NAME\",\"password\":\"$LOGIN_PASS\"}")
        TOKEN=$(json_field "$AUTH_RESP" "token")
        if [[ -z "$TOKEN" ]]; then
            error "Login failed. Check your credentials and try again."
            exit 1
        fi
        ok "Logged in as '$LOGIN_NAME'."
    else
        # ── Register ──────────────────────────────────────────────────────────
        info "Creating account '$LOGIN_NAME'..."
        AUTH_RESP=$(api_post "$SERVER_ADDR" "$API_PORT" "/auth/register" \
            "{\"login\":\"$LOGIN_NAME\",\"password\":\"$LOGIN_PASS\"}")
        TOKEN=$(json_field "$AUTH_RESP" "token")
        if [[ -z "$TOKEN" ]]; then
            error "Registration failed."
            exit 1
        fi
        ok "Account '${LOGIN_NAME}' created successfully."
    fi

    # Save session for the vpn-client binary
    mkdir -p "$CONF_DIR"
    printf '{"token":"%s","server":"%s","api_port":%s}' \
        "$TOKEN" "$SERVER_ADDR" "$API_PORT" > "$SESSION_FILE"
    chmod 600 "$SESSION_FILE"
    ok "Session saved to $SESSION_FILE"

    ACCOUNT_RESP=$(api_get_auth "$SERVER_ADDR" "$API_PORT" "/auth/me" "$TOKEN")
fi

# =============================================================================
# 7. SUBSCRIPTION CHECK & TRIAL ACTIVATION
# =============================================================================
section "Subscription"

SUB_RESP=$(api_get_auth "$SERVER_ADDR" "$API_PORT" "/subscription/status" "$TOKEN" 2>/dev/null || true)
SUB_STATUS=$(json_field "$SUB_RESP" "sub_status")
SUB_EXPIRES=$(json_field "$SUB_RESP" "sub_expires_at")

echo ""
echo -e "${BLU}Current subscription status:${RST} ${YEL}${SUB_STATUS:-unknown}${RST}"
[[ -n "$SUB_EXPIRES" ]] && echo -e "${BLU}Expires:${RST} $SUB_EXPIRES"
echo ""

if [[ "$SUB_STATUS" != "active" ]]; then
    echo -e "${YEL}No active subscription.${RST}"
    echo ""

    # ── Try a promo / trial code ───────────────────────────────────────────
    echo -e "${BLU}Available options:${RST}"
    echo -e "  ${GRN}1)${RST} Apply a promo or trial code"
    echo -e "  ${GRN}2)${RST} View subscription plans and buy"
    echo -e "  ${GRN}3)${RST} Skip (connect will fail without a subscription)"
    echo ""
    read -rp "  Choice [1]: " SUB_CHOICE
    SUB_CHOICE="${SUB_CHOICE:-1}"

    case "$SUB_CHOICE" in
        1)
            echo ""
            echo -e "${YEL}If the server admin ran server-setup.sh, try the code:${RST} ${GRN}TRIAL30${RST}"
            read -rp "  Enter promo code: " PROMO_CODE
            PROMO_CODE="${PROMO_CODE:-TRIAL30}"

            PROMO_RESP=$(api_post_auth "$SERVER_ADDR" "$API_PORT" "/promo/apply" \
                "$TOKEN" "{\"code\":\"$PROMO_CODE\"}" 2>/dev/null || true)

            if echo "$PROMO_RESP" | grep -qi '"message"'; then
                MSG=$(json_field "$PROMO_RESP" "message")
                NEW_EXP=$(json_field "$PROMO_RESP" "sub_expires_at")
                ok "Promo applied: $MSG"
                [[ -n "$NEW_EXP" ]] && info "Subscription active until: $NEW_EXP"
            else
                warn "Could not apply promo code."
                echo "  Response: $PROMO_RESP"
                echo ""
                echo -e "${YEL}You can still try to connect — the server will reject if no sub is active.${RST}"
            fi
            ;;
        2)
            echo ""
            PLANS_RESP=$(curl -s "http://${SERVER_ADDR}:${API_PORT}/subscription/plans" 2>/dev/null || true)
            if [[ -n "$PLANS_RESP" ]]; then
                echo -e "${BLU}Available plans:${RST}"
                echo "$PLANS_RESP" | sed 's/},{/}\n{/g' | while IFS= read -r line; do
                    PL_ID=$(json_field "$line" "id")
                    PL_NAME=$(json_field "$line" "name")
                    PL_PRICE=$(json_field_num "$line" "price_rub")
                    PL_DAYS=$(json_field_num "$line" "duration_days")
                    [[ -n "$PL_ID" ]] && printf "  %-10s %-35s %s ₽ / %s days\n" \
                        "$PL_ID" "$PL_NAME" "$PL_PRICE" "$PL_DAYS"
                done
            fi
            echo ""
            read -rp "  Plan ID to buy [standard]: " BUY_PLAN
            BUY_PLAN="${BUY_PLAN:-standard}"
            BUY_RESP=$(api_post_auth "$SERVER_ADDR" "$API_PORT" "/subscription/buy" \
                "$TOKEN" "{\"plan_id\":\"$BUY_PLAN\"}" 2>/dev/null || true)
            if echo "$BUY_RESP" | grep -qi 'expires'; then
                ok "Subscription activated!"
                echo "$BUY_RESP"
            else
                warn "Purchase may have failed (balance too low?)."
                echo "  Response: $BUY_RESP"
            fi
            ;;
        3)
            warn "Skipping subscription setup."
            ;;
    esac
fi

# ── Final subscription status ─────────────────────────────────────────────────
SUB_RESP=$(api_get_auth "$SERVER_ADDR" "$API_PORT" "/subscription/status" "$TOKEN" 2>/dev/null || true)
SUB_STATUS=$(json_field "$SUB_RESP" "sub_status")
SUB_SPEED=$(json_field_num "$SUB_RESP" "sub_speed_mbps")
SUB_EXPIRES=$(json_field "$SUB_RESP" "sub_expires_at")

echo ""
echo -e "${GRN}╔══════════════════════════════════════════════════════╗${RST}"
echo -e "${GRN}║               Account Summary                        ║${RST}"
echo -e "${GRN}╠══════════════════════════════════════════════════════╣${RST}"
echo -e "${GRN}║${RST}  Server        : $SERVER_ADDR:$API_PORT"
echo -e "${GRN}║${RST}  Subscription  : ${YEL}${SUB_STATUS:-unknown}${RST}"
[[ -n "$SUB_EXPIRES" ]] && \
echo -e "${GRN}║${RST}  Expires       : $SUB_EXPIRES"
if [[ "${SUB_SPEED:-0}" == "0" ]]; then
    echo -e "${GRN}║${RST}  Speed limit   : unlimited"
else
    echo -e "${GRN}║${RST}  Speed limit   : ${SUB_SPEED} Mbit/s"
fi
echo -e "${GRN}╚══════════════════════════════════════════════════════╝${RST}"
echo ""

if [[ "$MODE" == "status" ]]; then
    ok "Status check complete."
    exit 0
fi

# =============================================================================
# 8. CONNECT
# =============================================================================
section "Connecting to VPN"

if [[ "$SUB_STATUS" != "active" ]]; then
    warn "Subscription is not active. The server will likely reject the connection."
    read -rp "  Continue anyway? [y/N] " FORCE_CONNECT
    [[ ! "$FORCE_CONNECT" =~ ^[Yy]$ ]] && exit 0
fi

# Ensure binary is owned / executable by the real user
chmod +x "$BINARY"

if [[ "$MODE" == "socks5" ]]; then
    # ── SOCKS5 mode ───────────────────────────────────────────────────────────
    read -rp "  Local SOCKS5 port [${SOCKS_PORT}]: " INPUT_SOCKS
    SOCKS_PORT="${INPUT_SOCKS:-$SOCKS_PORT}"
    save_conf

    echo ""
    echo -e "${GRN}Starting SOCKS5 proxy on 127.0.0.1:${SOCKS_PORT}${RST}"
    echo -e "  Set system proxy → ${YEL}SOCKS5 127.0.0.1:${SOCKS_PORT}${RST}"
    echo -e "  Press Ctrl-C to disconnect."
    echo ""

    exec "$BINARY" connect \
        --server "$SERVER_ADDR" \
        --api-port "$API_PORT" \
        --proxy-port "$PROXY_PORT" \
        --mode socks5 \
        --socks-port "$SOCKS_PORT"
else
    # ── TUN mode (requires root) ───────────────────────────────────────────────
    echo ""
    echo -e "${GRN}Starting TUN VPN (full-tunnel, all traffic routed through VPN)${RST}"
    echo -e "  VPN server  : ${YEL}$SERVER_ADDR${RST}"
    echo -e "  UDP port    : ${YEL}$UDP_PORT${RST}"
    echo -e "  Press Ctrl-C to disconnect and restore routing."
    echo ""

    # Ask about split tunnel
    read -rp "  Use split-tunnel? (only VPN subnet 10.0.0.0/24 is routed) [y/N]: " SPLIT
    SPLIT_ARG=""
    [[ "$SPLIT" =~ ^[Yy]$ ]] && SPLIT_ARG="--split-tunnel"

    exec "$BINARY" connect \
        --server "$SERVER_ADDR" \
        --api-port "$API_PORT" \
        --udp-port "$UDP_PORT" \
        --mode tun \
        $SPLIT_ARG
fi
