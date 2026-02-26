#!/usr/bin/env bash
# =============================================================================
# Lowkey VPN Client — Launch Script
# =============================================================================
#
# Simple script to start the VPN client after initial setup is done.
# Run client-setup.sh first to register, get a subscription and configure.
#
# Usage:
#   sudo ./client-run.sh              # TUN mode (routes all traffic)
#   ./client-run.sh --socks5          # SOCKS5 proxy on 127.0.0.1:1080
#   ./client-run.sh --split           # TUN mode, split-tunnel (VPN subnet only)
#   sudo ./client-run.sh --stop       # Stop a detached background session
#   sudo ./client-run.sh --background # TUN mode, detached (nohup)
# =============================================================================

set -euo pipefail

# ── Colour helpers ────────────────────────────────────────────────────────────
GRN='\033[0;32m'; YEL='\033[1;33m'; RED='\033[0;31m'
BLU='\033[0;34m'; CYN='\033[0;36m'; RST='\033[0m'
info()  { echo -e "${BLU}[INFO]${RST}  $*"; }
ok()    { echo -e "${GRN}[ OK ]${RST}  $*"; }
warn()  { echo -e "${YEL}[WARN]${RST}  $*"; }
error() { echo -e "${RED}[ERR ]${RST}  $*" >&2; }

# ── Paths ─────────────────────────────────────────────────────────────────────
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BINARY="$SCRIPT_DIR/vpn-client/target/release/vpn-client"

REAL_USER="${SUDO_USER:-$USER}"
REAL_HOME=$(getent passwd "$REAL_USER" | cut -d: -f6)
CONF_DIR="$REAL_HOME/.config/lowkey"
CONF_FILE="$CONF_DIR/client.conf"
SESSION_FILE="$CONF_DIR/session.json"
PID_FILE="$SCRIPT_DIR/vpn-client.pid"
LOG_FILE="$SCRIPT_DIR/vpn-client.log"

# ── Parse flags ───────────────────────────────────────────────────────────────
SOCKS5=false
SPLIT=false
BG=false
STOP=false

for arg in "$@"; do
    case "$arg" in
        --socks5)     SOCKS5=true ;;
        --split)      SPLIT=true  ;;
        --background) BG=true     ;;
        --stop)       STOP=true   ;;
        --help|-h)
            echo "Usage: $0 [--socks5] [--split] [--background] [--stop]"
            echo "  (no flags)    TUN full-tunnel mode (root required)"
            echo "  --socks5      SOCKS5 proxy on 127.0.0.1:1080 (no root)"
            echo "  --split       TUN split-tunnel: only 10.0.0.0/24 via VPN"
            echo "  --background  Run detached (TUN mode, logs to $LOG_FILE)"
            echo "  --stop        Stop a background VPN session"
            exit 0 ;;
    esac
done

# ── Stop mode ─────────────────────────────────────────────────────────────────
if [[ "$STOP" == "true" ]]; then
    if [[ -f "$PID_FILE" ]]; then
        PID=$(cat "$PID_FILE")
        if kill -0 "$PID" 2>/dev/null; then
            kill "$PID"
            rm -f "$PID_FILE"
            ok "VPN client (PID $PID) stopped."
        else
            warn "PID $PID is no longer running."
            rm -f "$PID_FILE"
        fi
    else
        warn "No PID file found — is the client running in background?"
    fi
    exit 0
fi

# ── Sanity checks ─────────────────────────────────────────────────────────────
if [[ ! -x "$BINARY" ]]; then
    error "Client binary not found: $BINARY"
    error "Run ./client-setup.sh first to build the client."
    exit 1
fi

if [[ ! -f "$SESSION_FILE" ]]; then
    error "No session file found at $SESSION_FILE"
    error "Run ./client-setup.sh first to register and log in."
    exit 1
fi

if [[ ! -f "$CONF_FILE" ]]; then
    error "No config file found at $CONF_FILE"
    error "Run ./client-setup.sh first to configure the client."
    exit 1
fi

# Load saved config
# shellcheck source=/dev/null
source "$CONF_FILE"

# ── Root check for TUN mode ───────────────────────────────────────────────────
if [[ "$SOCKS5" == "false" && $EUID -ne 0 ]]; then
    error "TUN mode requires root. Use: sudo $0"
    error "Or use SOCKS5 mode (no root): $0 --socks5"
    exit 1
fi

# ── Build the command ─────────────────────────────────────────────────────────
CMD=("$BINARY" connect \
    --server "${SERVER_ADDR}" \
    --api-port "${API_PORT:-8080}")

if [[ "$SOCKS5" == "true" ]]; then
    CMD+=(
        --proxy-port "${PROXY_PORT:-8388}"
        --mode socks5
        --socks-port "${SOCKS_PORT:-1080}"
    )
else
    CMD+=(
        --udp-port "${UDP_PORT:-51820}"
        --mode tun
    )
    [[ "$SPLIT" == "true" ]] && CMD+=(--split-tunnel)
fi

# ── Print banner ──────────────────────────────────────────────────────────────
echo ""
echo -e "${GRN}╔══════════════════════════════════════════════════════╗${RST}"
echo -e "${GRN}║            Lowkey VPN Client                         ║${RST}"
echo -e "${GRN}╠══════════════════════════════════════════════════════╣${RST}"
echo -e "${GRN}║${RST}  Server  : ${YEL}${SERVER_ADDR}${RST}"
if [[ "$SOCKS5" == "true" ]]; then
echo -e "${GRN}║${RST}  Mode    : SOCKS5  →  ${YEL}127.0.0.1:${SOCKS_PORT:-1080}${RST}"
echo -e "${GRN}║${RST}           Set system proxy to SOCKS5 127.0.0.1:${SOCKS_PORT:-1080}"
else
echo -e "${GRN}║${RST}  Mode    : TUN  (${SPLIT:+split-tunnel}${SPLIT:-full-tunnel})"
echo -e "${GRN}║${RST}  UDP     : ${UDP_PORT:-51820}"
fi
echo -e "${GRN}╠══════════════════════════════════════════════════════╣${RST}"
if [[ "$BG" == "true" ]]; then
echo -e "${GRN}║${RST}  Running : detached  (logs → vpn-client.log)"
echo -e "${GRN}║${RST}  Stop    : sudo $0 --stop"
else
echo -e "${GRN}║${RST}  Press Ctrl-C to disconnect."
fi
echo -e "${GRN}╚══════════════════════════════════════════════════════╝${RST}"
echo ""

# ── Launch ────────────────────────────────────────────────────────────────────
if [[ "$BG" == "true" ]]; then
    # Stop any existing background session
    if [[ -f "$PID_FILE" ]]; then
        OLD_PID=$(cat "$PID_FILE")
        if kill -0 "$OLD_PID" 2>/dev/null; then
            info "Stopping previous session (PID $OLD_PID)..."
            kill "$OLD_PID"
            sleep 1
        fi
        rm -f "$PID_FILE"
    fi

    nohup "${CMD[@]}" >> "$LOG_FILE" 2>&1 &
    echo $! > "$PID_FILE"
    ok "VPN client started in background (PID $(cat "$PID_FILE"))."
    info "Tail logs: tail -f $LOG_FILE"
    info "Stop with: sudo $0 --stop"
else
    # Foreground — exec replaces this shell so Ctrl-C goes straight to the client
    exec "${CMD[@]}"
fi
