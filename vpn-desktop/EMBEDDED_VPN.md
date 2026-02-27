# Embedded Hysteria2 VPN

The desktop client ships with a **built-in Hysteria2 QUIC proxy** — no external
binary is required.  When the user clicks "Connect", the app:

1. Calls the API to register the peer and obtain a one-time password (`psk`)
2. Establishes an authenticated QUIC connection directly to the server
3. Starts a local **SOCKS5 proxy** at `127.0.0.1:10808`

All traffic routed through the SOCKS5 proxy is tunnelled over an encrypted QUIC
stream to the Hysteria2 server, which forwards it to the destination.

---

## Architecture

```
App / Browser
    │
    ▼  SOCKS5 (127.0.0.1:10808)
┌────────────────────────────────┐
│  Desktop App (Tauri)           │
│  ┌──────────────────────────┐  │
│  │ src-tauri/src/hysteria.rs│  │  ← embedded client
│  │  - QUIC (quinn 0.11)     │  │
│  │  - HTTP/3 auth (h3)      │  │
│  │  - SOCKS5 listener       │  │
│  └──────────────────────────┘  │
└────────────────────────────────┘
    │
    ▼  QUIC / TLS 1.3  (port 8388)
┌──────────────────────┐
│  Hysteria2 Server    │
│  vpn-server/         │
└──────────────────────┘
    │
    ▼  TCP / UDP  (target destination)
```

---

## Protocol

Hysteria2 runs over **QUIC** (RFC 9000) with **TLS 1.3** and ALPN `"h3"`.

### Authentication

After the QUIC handshake, the client authenticates with a single HTTP/3 request:

```
POST /auth HTTP/3
hysteria-auth: <password>
hysteria-cc-rx: 0
hysteria-padding: 00
```

A response with HTTP status **233** means authentication succeeded.
Any other status is treated as an authentication failure.

### TCP Proxying

Each target TCP connection opens a new QUIC bidirectional stream:

1. Client writes a `TcpRequest` frame (destination address + port)
2. Server replies with a `TcpResponse` frame (ok/error)
3. Raw data flows bidirectionally until either side closes

The wire format is defined in `vpn-common/src/hysteria.rs`.

---

## Source Files

| File | Purpose |
|------|---------|
| `src-tauri/src/hysteria.rs` | Embedded Hysteria2 client (QUIC + SOCKS5) |
| `src-tauri/src/vpn.rs` | VPN lifecycle (connect / disconnect) |
| `src-tauri/src/lib.rs` | Tauri commands (`toggle_vpn`, `vpn_status`) |
| `vpn-common/src/hysteria.rs` | Shared protocol types (TcpRequest, TcpResponse) |

---

## Building with a Fixed Server IP

At build time you can **embed the VPN server IP** so users don't have to
configure it manually in the app settings.

### Linux / macOS

```bash
./build.sh --desktop
# The script will prompt:
# VPN server IP: 1.2.3.4
```

Or pass it non-interactively:

```bash
LOWKEY_SERVER_IP=1.2.3.4 VITE_API_URL=http://1.2.3.4:8080 ./build.sh --desktop
```

### Windows (PowerShell)

```powershell
.\build.ps1 -Component desktop
# The script will prompt:
# VPN server IP: 1.2.3.4
```

Or pass it via environment:

```powershell
$env:LOWKEY_SERVER_IP = "1.2.3.4"
$env:VITE_API_URL = "http://1.2.3.4:8080"
.\build.ps1 -Component desktop
```

### What gets baked in

| Variable | Usage |
|----------|-------|
| `LOWKEY_SERVER_IP` | Rust compile-time constant via `option_env!()` — returned by the `get_baked_server_ip` Tauri command |
| `VITE_API_URL` | Injected into the frontend bundle by Vite — used as the default API URL in the store |

If left blank the app defaults to `http://localhost:8080` and the user can
change the server URL in the login screen settings.

---

## Configuring Apps to Use the Proxy

The embedded VPN operates as a **SOCKS5 proxy**, not a system-wide VPN tunnel.
Apps must be configured to use `127.0.0.1:10808` as their proxy.

### Browser (Firefox)

Settings → Network Settings → Manual proxy configuration:
- SOCKS Host: `127.0.0.1`, Port: `10808`, SOCKS v5
- Check "Proxy DNS when using SOCKS v5"

### Browser (Chrome / Edge)

Use a browser extension like **Proxy SwitchyOmega** and configure SOCKS5 at
`127.0.0.1:10808`.

### System-wide (Linux)

```bash
export ALL_PROXY=socks5://127.0.0.1:10808
export HTTPS_PROXY=socks5://127.0.0.1:10808
export HTTP_PROXY=socks5://127.0.0.1:10808
```

Or use `proxychains-ng` to wrap any application:

```bash
proxychains curl https://example.com
```

### System-wide (Windows)

Settings → Network & Internet → Proxy → Manual proxy setup:
- Address: `127.0.0.1`, Port: `10808`

Or in PowerShell:

```powershell
netsh winhttp set proxy 127.0.0.1:10808
```

---

## Security Notes

- TLS certificate verification is **skipped** for the Hysteria2 connection
  (the server uses a self-signed certificate).  Traffic is still encrypted with
  TLS 1.3; only the server *identity* is not verified.
- The SOCKS5 proxy listens on `127.0.0.1` only — it is not accessible from
  other machines on the network.
- The authentication password (`psk`) is generated per-session by the server
  and transmitted over HTTPS from the API.
