# Lowkey VPN

A minimal, self-contained VPN written in Rust.

```
┌──────────────┐  X25519 + ChaCha20-Poly1305   ┌──────────────────────────────┐
│  vpn-client  │ ◄──────────── UDP ───────────► │         vpn-server           │
│  (any host)  │                                │  TUN 10.0.0.1  iptables NAT  │
└──────────────┘                                │  HTTP API :8080              │
                                                └──────────────────────────────┘
```

## Workspace layout

| Crate | Description |
|-------|-------------|
| `vpn-common` | Shared types, crypto primitives, wire format |
| `vpn-server` | VPN server — TUN device, UDP tunnel, REST API |
| `vpn-client` | VPN client — connects to server, routes traffic |

---

## Protocol overview

### Key exchange

1. Client generates an ephemeral **X25519** key pair.
2. Client calls `POST /api/peers/register` with its public key + PSK.
3. Server responds with its public key and an assigned VPN IP (`10.0.0.x`).
4. Both sides compute the **X25519 shared secret**, derive a **32-byte symmetric
   key** via HKDF-SHA-256, and use **ChaCha20-Poly1305** for all tunnel traffic.

### UDP wire format

**Client → Server**
```
[ 4 B: client VPN IP (network byte order) ]
[12 B: random nonce                       ]
[ N B: ciphertext + 16 B Poly1305 tag     ]
```
The 4-byte prefix lets the server route packets without decrypting first.

**Server → Client**
```
[12 B: random nonce                   ]
[ N B: ciphertext + 16 B Poly1305 tag ]
```

### Traffic flow (full tunnel mode)

```
App → kernel → tun0 → vpn-client → [encrypt] → UDP → vpn-server
                                                        │
                                                   [decrypt]
                                                        │
                                                      tun0 → kernel IP stack
                                                        │
                                               iptables MASQUERADE
                                                        │
                                                  public internet
```

---

## Building

```bash
cargo build --release
```

Produces:
- `target/release/vpn-server`
- `target/release/vpn-client`

---

## Running

### Server

> Requires **root** (to create a TUN device and configure iptables).

```bash
# Minimum
VPN_PSK=mysecretkey sudo ./vpn-server

# Custom ports
sudo ./vpn-server --api-port 8080 --udp-port 51820 --psk mysecretkey
```

The server will:
1. Create `tun0` with IP `10.0.0.1/24`.
2. Enable IP forwarding (`/proc/sys/net/ipv4/ip_forward`).
3. Add an `iptables` MASQUERADE rule for the VPN subnet.
4. Listen for client registrations on the HTTP API.
5. Accept encrypted tunnel traffic on the UDP port.

### Client

> Requires **root** (to create a TUN device and modify the routing table).

```bash
# Full tunnel — all traffic goes through the VPN
sudo ./vpn-client connect --server <SERVER_IP> --psk mysecretkey

# Split tunnel — only 10.0.0.0/24 routes through the VPN
sudo ./vpn-client connect --server <SERVER_IP> --psk mysecretkey --split-tunnel

# Override ports if the server uses non-default values
sudo ./vpn-client connect --server <SERVER_IP> --psk mysecretkey \
    --api-port 8080 --udp-port 51820
```

Press **Ctrl-C** to disconnect. The client restores the original routing table
before exiting.

---

## REST API

| Method | Path | Description |
|--------|------|-------------|
| `GET` | `/api/status` | Server status and peer count |
| `GET` | `/api/peers` | List connected peers |
| `POST` | `/api/peers/register` | Register a new peer |
| `DELETE` | `/api/peers/:ip` | Remove a peer by VPN IP |

### `POST /api/peers/register`

Request:
```json
{
  "public_key": "<hex-encoded X25519 public key>",
  "psk": "mysecretkey"
}
```

Response:
```json
{
  "server_public_key": "<hex>",
  "assigned_ip": "10.0.0.2",
  "udp_port": 51820,
  "subnet": "10.0.0.0/24"
}
```

---

## Security notes

- Authentication uses a **pre-shared key** (PSK) passed at registration time.
- Traffic is encrypted with **ChaCha20-Poly1305** (authenticated encryption).
- Keys are derived via **X25519 Diffie-Hellman + HKDF-SHA-256**.
- Each packet uses a **fresh random 96-bit nonce**; there is no nonce reuse.
- The server generates a **new X25519 key pair on every restart** (ephemeral
  server key). For persistent server keys, save `server_secret` to disk.
- No perfect forward secrecy beyond the ephemeral server key — for PFS use
  per-session client keys (which the client already does by design).
