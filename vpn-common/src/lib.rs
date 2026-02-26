use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use hkdf::Hkdf;
use sha2::{Digest, Sha256};
use serde::{Deserialize, Serialize};
use x25519_dalek::{PublicKey, SharedSecret, StaticSecret};

// ── Network constants ────────────────────────────────────────────────────────

pub const VPN_SERVER_IP: &str = "10.0.0.1";
pub const VPN_NETMASK: &str = "255.255.255.0";
pub const VPN_SUBNET: &str = "10.0.0.0";
pub const VPN_SUBNET_CIDR: &str = "10.0.0.0/24";
pub const DEFAULT_UDP_PORT: u16 = 51820;
pub const DEFAULT_API_PORT: u16 = 8080;
pub const DEFAULT_PROXY_PORT: u16 = 8388;

// ── API types ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterRequest {
    pub public_key: String,
    pub psk: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterResponse {
    pub server_public_key: String,
    pub assigned_ip: String,
    pub udp_port: u16,
    pub proxy_port: u16,
    pub subnet: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct PeerInfo {
    pub vpn_ip: String,
    pub endpoint: String,
    pub bytes_in: u64,
    pub bytes_out: u64,
    pub speed_in_bps: u64,
    pub speed_out_bps: u64,
    pub limit_bps: u64,
    pub connected_secs: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub running: bool,
    pub peer_count: usize,
    pub server_vpn_ip: String,
    pub public_ip: String,
    pub udp_port: u16,
    pub proxy_port: u16,
    pub uptime_secs: u64,
    pub total_bytes_in: u64,
    pub total_bytes_out: u64,
}

/// Body for PUT /api/peers/:ip/limit
#[derive(Debug, Serialize, Deserialize)]
pub struct LimitRequest {
    /// Bandwidth limit in Mbps (0 = unlimited)
    pub limit_mbps: f64,
}

// ── Proxy protocol wire format ────────────────────────────────────────────────
//
// TCP proxy connection (VLESS-style) over an encrypted framed stream:
//
// Handshake step 1 (client → server):
//   [32 B: client X25519 ephemeral public key]
//
// Handshake step 2 (server → client):
//   [32 B: server X25519 public key]
//
// First encrypted frame (client → server) — "connect header":
//   [16 B: SHA256(psk)[0..16]  — authentication tag]
//   [1 B:  addr_type: 1=IPv4, 3=hostname, 4=IPv6]
//   [N B:  address  (IPv4=4B, hostname=1Blen+NB, IPv6=16B)]
//   [2 B:  port big-endian]
//
// First encrypted frame (server → client) — status:
//   [1 B: 0x00 = success, 0x01 = auth fail, 0x02 = connect fail]
//
// Subsequent frames (bidirectional):
//   [2 B: payload_len BE]
//   [12 B: nonce]
//   [payload_len B: ciphertext + 16 B tag]
// ─────────────────────────────────────────────────────────────────────────────

pub const PROXY_FRAME_MAX: usize = 65535;

/// Compute the 16-byte auth token from a PSK.
pub fn psk_auth_token(psk: &str) -> [u8; 16] {
    let hash = Sha256::digest(psk.as_bytes());
    let mut token = [0u8; 16];
    token.copy_from_slice(&hash[..16]);
    token
}

// ── Crypto ───────────────────────────────────────────────────────────────────

pub struct VpnCrypto {
    cipher: ChaCha20Poly1305,
}

impl VpnCrypto {
    pub fn new(my_secret: &StaticSecret, peer_public: &PublicKey) -> Self {
        let shared = my_secret.diffie_hellman(peer_public);
        Self::from_shared_secret(&shared)
    }

    pub fn from_shared_secret(shared: &SharedSecret) -> Self {
        let hk = Hkdf::<Sha256>::new(Some(b"lowkey-vpn-v1-salt"), shared.as_bytes());
        let mut key = [0u8; 32];
        hk.expand(b"chacha20poly1305-key", &mut key)
            .expect("HKDF expand failed");
        let cipher = ChaCha20Poly1305::new_from_slice(&key).expect("bad key length");
        VpnCrypto { cipher }
    }

    /// Encrypt `data`; returns `nonce(12B) || ciphertext`.
    pub fn encrypt(&self, data: &[u8]) -> Vec<u8> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut rand::rngs::OsRng);
        let ct = self.cipher.encrypt(&nonce, data).expect("encryption failed");
        let mut out = Vec::with_capacity(12 + ct.len());
        out.extend_from_slice(nonce.as_slice());
        out.extend_from_slice(&ct);
        out
    }

    /// Decrypt `nonce(12B) || ciphertext`; returns plaintext or `None`.
    pub fn decrypt(&self, data: &[u8]) -> Option<Vec<u8>> {
        if data.len() < 13 {
            return None;
        }
        let nonce = Nonce::from_slice(&data[..12]);
        self.cipher.decrypt(nonce, &data[12..]).ok()
    }
}

/// Framed async crypto stream helpers (for TCP proxy):
/// Frame = [2B len BE] + [12B nonce] + [len B ciphertext]
pub struct FramedCrypto {
    cipher: ChaCha20Poly1305,
}

impl FramedCrypto {
    pub fn new(secret: &StaticSecret, peer_pub: &PublicKey) -> Self {
        let shared = secret.diffie_hellman(peer_pub);
        let hk = Hkdf::<Sha256>::new(Some(b"lowkey-proxy-v1"), shared.as_bytes());
        let mut key = [0u8; 32];
        hk.expand(b"proxy-session-key", &mut key).expect("HKDF");
        FramedCrypto {
            cipher: ChaCha20Poly1305::new_from_slice(&key).unwrap(),
        }
    }

    /// Encode one frame: [2B len] [12B nonce] [ciphertext]
    pub fn encode(&self, plaintext: &[u8]) -> Vec<u8> {
        let nonce = ChaCha20Poly1305::generate_nonce(&mut rand::rngs::OsRng);
        let ct = self.cipher.encrypt(&nonce, plaintext).expect("encrypt");
        let len = ct.len() as u16;
        let mut out = Vec::with_capacity(2 + 12 + ct.len());
        out.extend_from_slice(&len.to_be_bytes());
        out.extend_from_slice(nonce.as_slice());
        out.extend_from_slice(&ct);
        out
    }

    /// Decode a frame from buf: reads exactly `2 + 12 + len` bytes.
    /// Returns (plaintext, bytes_consumed).
    pub fn decode(&self, buf: &[u8]) -> Option<(Vec<u8>, usize)> {
        if buf.len() < 14 {
            return None;
        }
        let len = u16::from_be_bytes([buf[0], buf[1]]) as usize;
        let total = 2 + 12 + len;
        if buf.len() < total {
            return None;
        }
        let nonce = Nonce::from_slice(&buf[2..14]);
        let plain = self.cipher.decrypt(nonce, &buf[14..total]).ok()?;
        Some((plain, total))
    }
}

// ── Packet helpers ───────────────────────────────────────────────────────────

pub fn parse_dest_ipv4(pkt: &[u8]) -> Option<std::net::Ipv4Addr> {
    if pkt.len() < 20 || (pkt[0] >> 4) != 4 {
        return None;
    }
    Some(std::net::Ipv4Addr::new(pkt[16], pkt[17], pkt[18], pkt[19]))
}

// ── Hex helpers ──────────────────────────────────────────────────────────────

pub fn to_hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{:02x}", x)).collect()
}

pub fn from_hex(s: &str) -> Option<Vec<u8>> {
    if s.len() % 2 != 0 {
        return None;
    }
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect()
}
