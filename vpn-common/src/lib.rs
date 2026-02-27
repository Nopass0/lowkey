//! # vpn-common
//!
//! Shared types, crypto primitives and wire-format helpers used by both
//! `vpn-server` and `vpn-client`.
//!
//! ## Crypto overview
//!
//! All VPN tunnels use the following key-agreement and encryption chain:
//!
//! ```text
//! X25519 DH  →  HKDF-SHA256  →  ChaCha20-Poly1305 (AEAD)
//! ```
//!
//! There are two distinct crypto contexts:
//! * [`VpnCrypto`]  — UDP tunnel (packet-level encryption)
//! * [`FramedCrypto`] — TCP proxy (stream-framing encryption)
//!
//! ## Wire formats
//!
//! ### UDP packet (client → server)
//! ```text
//! [4 B : client VPN IP]  [12 B : nonce]  [N B : ciphertext+tag]
//! ```
//!
//! ### UDP packet (server → client)
//! ```text
//! [12 B : nonce]  [N B : ciphertext+tag]
//! ```
//!
//! ### TCP proxy frame (bidirectional after handshake)
//! ```text
//! [2 B : payload_len BE]  [12 B : nonce]  [payload_len B : ciphertext+tag]
//! ```

use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use hkdf::Hkdf;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use x25519_dalek::{PublicKey, SharedSecret, StaticSecret};

/// Hysteria2 protocol types: wire formats, Salamander obfuscation, address
/// encoding.  See [`hysteria`] for the full API.
pub mod hysteria;

// ── Network constants ─────────────────────────────────────────────────────────

/// VPN server-side TUN IP (gateway for all clients).
pub const VPN_SERVER_IP: &str = "10.66.0.1";

/// Netmask for the /24 VPN subnet.
pub const VPN_NETMASK: &str = "255.255.255.0";

/// Base of the VPN subnet (without CIDR suffix).
pub const VPN_SUBNET: &str = "10.66.0.0";

/// VPN subnet in CIDR notation — used for iptables rules.
pub const VPN_SUBNET_CIDR: &str = "10.66.0.0/24";

/// First three octets of the VPN subnet for dynamic client IP allocation.
pub const VPN_SUBNET_OCTETS: [u8; 3] = [10, 66, 0];

/// Default UDP tunnel port.
pub const DEFAULT_UDP_PORT: u16 = 51820;

/// Default HTTP API port.
pub const DEFAULT_API_PORT: u16 = 8080;

/// Default TCP proxy port (VLESS-style encrypted proxy).
pub const DEFAULT_PROXY_PORT: u16 = 8388;

// ── API request / response types ─────────────────────────────────────────────

/// Client → server: register a new VPN peer.
///
/// The client generates an ephemeral X25519 key pair each session and sends
/// its public half here.  The `psk` field is a legacy plain-text pre-shared
/// key kept for backward compatibility with older clients.
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterRequest {
    /// Hex-encoded X25519 client public key (32 bytes → 64 hex chars).
    pub public_key: String,
    /// Legacy PSK (empty string to skip check).
    pub psk: String,
}

/// Server → client: response to a successful peer registration.
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterResponse {
    /// Hex-encoded X25519 server public key.  Client uses this to complete
    /// the DH handshake and derive the session encryption key.
    pub server_public_key: String,
    /// Assigned VPN IP for this session (e.g. `"10.0.0.5"`).
    pub assigned_ip: String,
    /// UDP tunnel port the client should send encrypted packets to.
    pub udp_port: u16,
    /// TCP proxy port for SOCKS5/VLESS-style clients.
    pub proxy_port: u16,
    /// VPN subnet in CIDR form (client should add a route for this).
    pub subnet: String,
}

/// Snapshot of one connected peer's traffic statistics.
#[derive(Debug, Serialize, Deserialize)]
pub struct PeerInfo {
    /// VPN IP assigned to this peer.
    pub vpn_ip: String,
    /// Last known UDP endpoint (`ip:port`) or `"pending"`.
    pub endpoint: String,
    /// Cumulative bytes received from this peer since connect.
    pub bytes_in: u64,
    /// Cumulative bytes sent to this peer since connect.
    pub bytes_out: u64,
    /// Instantaneous download speed (bytes/s, updated once per second).
    pub speed_in_bps: u64,
    /// Instantaneous upload speed (bytes/s, updated once per second).
    pub speed_out_bps: u64,
    /// Active bandwidth cap in bytes/s (0 = unlimited).
    pub limit_bps: u64,
    /// Seconds elapsed since this peer registered.
    pub connected_secs: u64,
}

/// Response from `GET /api/status`.
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    /// Always `true` if the server is reachable.
    pub running: bool,
    /// Number of currently registered peers.
    pub peer_count: usize,
    /// Server-side VPN gateway IP.
    pub server_vpn_ip: String,
    /// Detected public IP (used by clients to connect).
    pub public_ip: String,
    /// Active UDP tunnel port.
    pub udp_port: u16,
    /// Active TCP proxy port.
    pub proxy_port: u16,
    /// Server uptime in seconds.
    pub uptime_secs: u64,
    /// All-time bytes received through the tunnel.
    pub total_bytes_in: u64,
    /// All-time bytes sent through the tunnel.
    pub total_bytes_out: u64,
}

/// Body for `PUT /api/peers/:ip/limit` — set per-peer bandwidth cap.
#[derive(Debug, Serialize, Deserialize)]
pub struct LimitRequest {
    /// Bandwidth limit in **Mbit/s** (0 = unlimited).
    pub limit_mbps: f64,
}

// ── Proxy protocol wire format ────────────────────────────────────────────────
//
// The TCP proxy uses an X25519 + FramedCrypto handshake before any data flows:
//
//   Client  →  Server : [32 B: client ephemeral X25519 pubkey]
//   Server  →  Client : [32 B: server X25519 pubkey]
//   (both sides derive the same shared key via DH + HKDF)
//
//   Client  →  Server : first encrypted frame = "connect header"
//     [16 B: SHA256(psk)[0..16]]          — auth token
//     [1 B:  addr_type]                   — 1=IPv4, 3=hostname, 4=IPv6
//     [N B:  address]                     — 4B / (1B len + NB) / 16B
//     [2 B:  port BE]
//
//   Server  →  Client : first encrypted frame = status byte
//     0x00 = success   0x01 = auth fail   0x02 = connect fail
//
//   After that both sides exchange FramedCrypto frames indefinitely.

/// Maximum payload length allowed in a single TCP proxy frame (bytes).
pub const PROXY_FRAME_MAX: usize = 65535;

/// Derive a 16-byte auth token from a plain-text PSK.
///
/// The token is the first 16 bytes of `SHA256(psk)`.  It is sent inside the
/// first encrypted frame of the proxy handshake so the server can verify the
/// client knows the correct PSK without revealing it in plaintext.
pub fn psk_auth_token(psk: &str) -> [u8; 16] {
    let hash = Sha256::digest(psk.as_bytes());
    let mut token = [0u8; 16];
    token.copy_from_slice(&hash[..16]);
    token
}

// ── UDP tunnel crypto ─────────────────────────────────────────────────────────

/// Symmetric AEAD crypto context for UDP-tunnel packets.
///
/// A single `VpnCrypto` instance is shared for the lifetime of one VPN session
/// (peer registration → disconnection).  Keys are derived from an X25519
/// Diffie-Hellman shared secret via HKDF-SHA256.
///
/// # Encryption format
/// ```text
/// encrypt(plaintext) → [12 B nonce] ++ [N+16 B ciphertext+tag]
/// ```
pub struct VpnCrypto {
    /// Underlying ChaCha20-Poly1305 AEAD cipher.
    cipher: ChaCha20Poly1305,
}

impl VpnCrypto {
    /// Create from a local secret and a remote public key.
    ///
    /// Performs X25519 DH then calls [`Self::from_shared_secret`].
    pub fn new(my_secret: &StaticSecret, peer_public: &PublicKey) -> Self {
        let shared = my_secret.diffie_hellman(peer_public);
        Self::from_shared_secret(&shared)
    }

    /// Derive a `VpnCrypto` directly from an already-computed shared secret.
    ///
    /// Key derivation:
    /// ```text
    /// HKDF-SHA256(salt = "lowkey-vpn-v1-salt", ikm = shared_secret)
    ///             → 32-byte ChaCha20-Poly1305 key
    /// ```
    pub fn from_shared_secret(shared: &SharedSecret) -> Self {
        let hk = Hkdf::<Sha256>::new(Some(b"lowkey-vpn-v1-salt"), shared.as_bytes());
        let mut key = [0u8; 32];
        hk.expand(b"chacha20poly1305-key", &mut key)
            .expect("HKDF expand failed");
        let cipher = ChaCha20Poly1305::new_from_slice(&key).expect("bad key length");
        VpnCrypto { cipher }
    }

    /// Encrypt `data` and prepend a random 12-byte nonce.
    ///
    /// Returns `nonce(12 B) || ciphertext+tag(N+16 B)`.
    pub fn encrypt(&self, data: &[u8]) -> Vec<u8> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut rand::rngs::OsRng);
        let ct = self
            .cipher
            .encrypt(&nonce, data)
            .expect("encryption failed");
        let mut out = Vec::with_capacity(12 + ct.len());
        out.extend_from_slice(nonce.as_slice());
        out.extend_from_slice(&ct);
        out
    }

    /// Decrypt `nonce(12 B) || ciphertext+tag`.
    ///
    /// Returns the plaintext on success or `None` if the MAC check fails or the
    /// input is too short.
    pub fn decrypt(&self, data: &[u8]) -> Option<Vec<u8>> {
        if data.len() < 13 {
            return None;
        }
        let nonce = Nonce::from_slice(&data[..12]);
        self.cipher.decrypt(nonce, &data[12..]).ok()
    }
}

// ── TCP proxy stream crypto ───────────────────────────────────────────────────

/// Framed AEAD crypto for the TCP proxy stream.
///
/// Frames are length-prefixed so they can be reassembled from a byte stream:
///
/// ```text
/// [2 B : ciphertext_len BE]  [12 B : nonce]  [ciphertext_len B : ciphertext+tag]
/// ```
///
/// Both server and client share the same `FramedCrypto` instance derived from
/// the X25519 handshake that opens every proxy connection.
pub struct FramedCrypto {
    /// Underlying ChaCha20-Poly1305 AEAD cipher.
    cipher: ChaCha20Poly1305,
}

impl FramedCrypto {
    /// Derive a `FramedCrypto` from a local secret and a remote public key.
    ///
    /// Key derivation:
    /// ```text
    /// HKDF-SHA256(salt = "lowkey-proxy-v1", ikm = X25519(secret, peer_pub))
    ///             → 32-byte ChaCha20-Poly1305 key
    /// ```
    pub fn new(secret: &StaticSecret, peer_pub: &PublicKey) -> Self {
        let shared = secret.diffie_hellman(peer_pub);
        let hk = Hkdf::<Sha256>::new(Some(b"lowkey-proxy-v1"), shared.as_bytes());
        let mut key = [0u8; 32];
        hk.expand(b"proxy-session-key", &mut key).expect("HKDF");
        FramedCrypto {
            cipher: ChaCha20Poly1305::new_from_slice(&key).unwrap(),
        }
    }

    /// Encode one plaintext message as a single encrypted frame.
    ///
    /// Output layout: `[2 B len] [12 B nonce] [ciphertext+tag]`.
    /// The `len` field covers only the ciphertext (not the nonce).
    pub fn encode(&self, plaintext: &[u8]) -> Vec<u8> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut rand::rngs::OsRng);
        let ct = self.cipher.encrypt(&nonce, plaintext).expect("encrypt");
        // len = ciphertext length (which already includes the 16-byte AEAD tag)
        let len = ct.len() as u16;
        let mut out = Vec::with_capacity(2 + 12 + ct.len());
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(nonce.as_slice());
        out.extend_from_slice(&ct);
        out
    }

    /// Try to decode the first complete frame from `buf`.
    ///
    /// Returns `(plaintext, bytes_consumed)` if a complete frame is available,
    /// or `None` if more data is needed (partial frame in buffer).
    pub fn decode(&self, buf: &[u8]) -> Option<(Vec<u8>, usize)> {
        // Need at least 2B (len) + 12B (nonce) = 14 B header
        if buf.len() < 14 {
            return None;
        }
        let len = u16::from_be_bytes([buf[0], buf[1]]) as usize;
        let total = 2 + 12 + len; // header + nonce + ciphertext
        if buf.len() < total {
            return None; // incomplete frame
        }
        let nonce = Nonce::from_slice(&buf[2..14]);
        let plain = self.cipher.decrypt(nonce, &buf[14..total]).ok()?;
        Some((plain, total))
    }
}

// ── IP packet helpers ─────────────────────────────────────────────────────────

/// Extract the IPv4 destination address from a raw IP packet.
///
/// Returns `None` if the packet is too short or not an IPv4 packet
/// (version nibble ≠ 4).  The destination address lives at bytes 16–19
/// of the IPv4 header.
pub fn parse_dest_ipv4(pkt: &[u8]) -> Option<std::net::Ipv4Addr> {
    if pkt.len() < 20 || (pkt[0] >> 4) != 4 {
        return None;
    }
    Some(std::net::Ipv4Addr::new(pkt[16], pkt[17], pkt[18], pkt[19]))
}

// ── Hex encoding helpers ──────────────────────────────────────────────────────

/// Encode a byte slice as a lowercase hex string.
pub fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

/// Decode a hex string into bytes.
///
/// Returns `None` if the string has an odd length or contains non-hex
/// characters.
pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}
