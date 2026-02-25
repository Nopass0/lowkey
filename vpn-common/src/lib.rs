use chacha20poly1305::{
    aead::{Aead, AeadCore, KeyInit},
    ChaCha20Poly1305, Nonce,
};
use hkdf::Hkdf;
use sha2::Sha256;
use serde::{Deserialize, Serialize};
use x25519_dalek::{PublicKey, SharedSecret, StaticSecret};

// ── Network constants ────────────────────────────────────────────────────────

pub const VPN_SERVER_IP: &str = "10.0.0.1";
pub const VPN_NETMASK: &str = "255.255.255.0";
pub const VPN_SUBNET: &str = "10.0.0.0";
pub const VPN_SUBNET_CIDR: &str = "10.0.0.0/24";
pub const DEFAULT_UDP_PORT: u16 = 51820;
pub const DEFAULT_API_PORT: u16 = 8080;

// ── API types ────────────────────────────────────────────────────────────────

/// Client → Server: register as a new peer
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterRequest {
    /// Hex-encoded X25519 public key
    pub public_key: String,
    /// Pre-shared key for authentication
    pub psk: String,
}

/// Server → Client: registration response
#[derive(Debug, Serialize, Deserialize)]
pub struct RegisterResponse {
    /// Hex-encoded X25519 public key of the server
    pub server_public_key: String,
    /// Assigned VPN IP address (e.g. "10.0.0.2")
    pub assigned_ip: String,
    /// Server's UDP tunnel port
    pub udp_port: u16,
    /// VPN subnet in CIDR notation
    pub subnet: String,
}

/// Info about a connected peer (returned by GET /api/peers)
#[derive(Debug, Serialize, Deserialize)]
pub struct PeerInfo {
    pub vpn_ip: String,
    pub endpoint: String,
}

/// Server status (returned by GET /api/status)
#[derive(Debug, Serialize, Deserialize)]
pub struct StatusResponse {
    pub running: bool,
    pub peer_count: usize,
    pub server_vpn_ip: String,
    pub udp_port: u16,
}

// ── UDP packet wire format ───────────────────────────────────────────────────
//
// Client → Server:
//   [4 bytes: client VPN IP (network byte order)]
//   [12 bytes: ChaCha20-Poly1305 nonce]
//   [N bytes: ciphertext + 16-byte Poly1305 tag]
//
// Server → Client:
//   [12 bytes: ChaCha20-Poly1305 nonce]
//   [N bytes: ciphertext + 16-byte Poly1305 tag]
//
// The 4-byte VPN IP prefix lets the server route packets without decrypting.
// ────────────────────────────────────────────────────────────────────────────

// ── Crypto ───────────────────────────────────────────────────────────────────

/// Symmetric AEAD wrapper derived from an X25519 shared secret.
pub struct VpnCrypto {
    cipher: ChaCha20Poly1305,
}

impl VpnCrypto {
    /// Derive a cipher from two parties' keys using X25519 + HKDF.
    pub fn new(my_secret: &StaticSecret, peer_public: &PublicKey) -> Self {
        let shared = my_secret.diffie_hellman(peer_public);
        Self::from_shared_secret(&shared)
    }

    /// Derive a cipher directly from a pre-computed shared secret.
    pub fn from_shared_secret(shared: &SharedSecret) -> Self {
        let hk = Hkdf::<Sha256>::new(Some(b"lowkey-vpn-v1-salt"), shared.as_bytes());
        let mut key = [0u8; 32];
        hk.expand(b"chacha20poly1305-key", &mut key)
            .expect("HKDF expand failed");
        let cipher = ChaCha20Poly1305::new_from_slice(&key).expect("bad key length");
        VpnCrypto { cipher }
    }

    /// Encrypt `data`; returns `nonce (12 B) || ciphertext`.
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

    /// Decrypt `nonce (12 B) || ciphertext`; returns plaintext or `None` on error.
    pub fn decrypt(&self, data: &[u8]) -> Option<Vec<u8>> {
        if data.len() < 13 {
            return None;
        }
        let nonce = Nonce::from_slice(&data[..12]);
        self.cipher.decrypt(nonce, &data[12..]).ok()
    }
}

// ── Packet helpers ───────────────────────────────────────────────────────────

/// Extract the destination IPv4 address from a raw IPv4 packet.
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
