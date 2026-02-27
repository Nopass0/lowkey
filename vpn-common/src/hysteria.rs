//! # Hysteria2 Protocol — Shared Types and Utilities
//!
//! This module implements the wire-format types and the Salamander obfuscation
//! layer for the Hysteria2 proxy protocol
//! (<https://v2.hysteria.network/docs/developers/Protocol/>).
//!
//! ## Protocol Overview
//!
//! Hysteria2 runs over QUIC (RFC 9000).  After a QUIC connection is
//! established (TLS 1.3), the client authenticates via a single HTTP/3
//! POST request to `/auth`.  On success the server replies with HTTP
//! status **233** and bandwidth negotiation headers.  The same QUIC
//! connection is then reused for:
//!
//! * **TCP proxying** — each target TCP connection opens a new QUIC
//!   bidirectional stream.  The client writes a [`TcpRequest`] header
//!   first; the server responds with a [`TcpResponse`]; then data flows
//!   transparently in both directions.
//!
//! * **UDP relaying** — UDP datagrams are packed into [`UdpMessage`]
//!   structs and sent / received as QUIC unreliable datagrams (RFC 9221).
//!   Large payloads are fragmented across multiple QUIC datagrams.
//!
//! ## Salamander Obfuscation (optional)
//!
//! [`SalamanderObfuscator`] XOR-obfuscates each QUIC UDP packet before it
//! leaves the socket, making the traffic look like random noise to DPI
//! systems.  The algorithm is:
//!
//! ```text
//! salt    = random 8 bytes
//! key     = BLAKE2b-256(salt || password)
//! payload = XOR(quic_packet, cyclic_repeat(key))
//! wire    = salt || payload
//! ```
//!
//! On the receiving side the process is reversed: extract the 8-byte salt
//! from the front, recompute the key, and XOR the remaining bytes.

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use anyhow::{bail, Context, Result};
use blake2::{Blake2b, Digest};
use blake2::digest::typenum::U32;

/// Convenience type alias: BLAKE2b with 256-bit (32-byte) output.
type Blake2b256 = Blake2b<U32>;
use bytes::{Buf, BufMut, Bytes, BytesMut};
use rand::RngCore;

// ─── Address encoding ──────────────────────────────────────────────────────

/// Address type tag as used in both [`TcpRequest`] and [`UdpMessage`].
///
/// | Byte | Meaning |
/// |------|---------|
/// | 0x01 | IPv4 (4 bytes, big-endian) |
/// | 0x02 | Hostname (1-byte length prefix + UTF-8 bytes) |
/// | 0x03 | IPv6 (16 bytes, big-endian) |
#[derive(Debug, Clone, PartialEq)]
pub enum HysteriaAddr {
    /// IPv4 socket address.
    V4(SocketAddr),
    /// IPv6 socket address.
    V6(SocketAddr),
    /// Domain name + port (e.g. `"example.com:443"`).
    Name(String, u16),
}

impl HysteriaAddr {
    /// Encode the address into `buf` using the Hysteria2 wire format.
    pub fn encode(&self, buf: &mut BytesMut) {
        match self {
            HysteriaAddr::V4(sa) => {
                buf.put_u8(0x01);
                if let IpAddr::V4(ip) = sa.ip() {
                    buf.put_slice(&ip.octets());
                }
                buf.put_u16(sa.port());
            }
            HysteriaAddr::V6(sa) => {
                buf.put_u8(0x03);
                if let IpAddr::V6(ip) = sa.ip() {
                    buf.put_slice(&ip.octets());
                }
                buf.put_u16(sa.port());
            }
            HysteriaAddr::Name(host, port) => {
                buf.put_u8(0x02);
                let bytes = host.as_bytes();
                buf.put_u8(bytes.len() as u8);
                buf.put_slice(bytes);
                buf.put_u16(*port);
            }
        }
    }

    /// Decode a [`HysteriaAddr`] from a byte buffer.
    ///
    /// Returns an error if the buffer is too short or the type byte is
    /// unrecognised.
    pub fn decode(buf: &mut impl Buf) -> Result<Self> {
        if buf.remaining() < 1 {
            bail!("address buffer too short");
        }
        let addr_type = buf.get_u8();
        match addr_type {
            0x01 => {
                if buf.remaining() < 6 {
                    bail!("IPv4 address buffer too short");
                }
                let mut ip = [0u8; 4];
                buf.copy_to_slice(&mut ip);
                let port = buf.get_u16();
                Ok(HysteriaAddr::V4(SocketAddr::new(
                    IpAddr::V4(Ipv4Addr::from(ip)),
                    port,
                )))
            }
            0x03 => {
                if buf.remaining() < 18 {
                    bail!("IPv6 address buffer too short");
                }
                let mut ip = [0u8; 16];
                buf.copy_to_slice(&mut ip);
                let port = buf.get_u16();
                Ok(HysteriaAddr::V6(SocketAddr::new(
                    IpAddr::V6(Ipv6Addr::from(ip)),
                    port,
                )))
            }
            0x02 => {
                if buf.remaining() < 1 {
                    bail!("hostname length missing");
                }
                let len = buf.get_u8() as usize;
                if buf.remaining() < len + 2 {
                    bail!("hostname buffer too short");
                }
                let mut host_bytes = vec![0u8; len];
                buf.copy_to_slice(&mut host_bytes);
                let host = String::from_utf8(host_bytes)
                    .context("hostname is not valid UTF-8")?;
                let port = buf.get_u16();
                Ok(HysteriaAddr::Name(host, port))
            }
            other => bail!("unknown address type: {:#04x}", other),
        }
    }

    /// Return the host string (IP or hostname, without port).
    pub fn host(&self) -> String {
        match self {
            HysteriaAddr::V4(sa) => sa.ip().to_string(),
            HysteriaAddr::V6(sa) => sa.ip().to_string(),
            HysteriaAddr::Name(h, _) => h.clone(),
        }
    }

    /// Return the port number.
    pub fn port(&self) -> u16 {
        match self {
            HysteriaAddr::V4(sa) => sa.port(),
            HysteriaAddr::V6(sa) => sa.port(),
            HysteriaAddr::Name(_, p) => *p,
        }
    }
}

// ─── TCP Proxy Wire Format ─────────────────────────────────────────────────

/// **TCPRequest** — the first message a client sends on a new QUIC
/// bidirectional stream when it wants to open a TCP connection through the
/// server.
///
/// Wire layout (big-endian):
/// ```text
/// [HysteriaAddr]  destination address (variable length)
/// [uint16]        padding length
/// [N bytes]       random padding (ignored)
/// ```
#[derive(Debug, Clone)]
pub struct TcpRequest {
    /// Destination the server should connect to.
    pub addr: HysteriaAddr,
}

impl TcpRequest {
    /// Serialize the request into a [`Bytes`] buffer.
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::new();
        self.addr.encode(&mut buf);
        // Zero padding for now (compatible with spec — may add random later)
        buf.put_u16(0u16);
        buf.freeze()
    }

    /// Deserialize a [`TcpRequest`] from raw bytes.
    pub fn decode(data: Bytes) -> Result<Self> {
        let mut cur = data;
        let addr = HysteriaAddr::decode(&mut cur)?;
        // Read and discard padding
        if cur.remaining() >= 2 {
            let pad_len = cur.get_u16() as usize;
            if cur.remaining() >= pad_len {
                cur.advance(pad_len);
            }
        }
        Ok(TcpRequest { addr })
    }
}

/// **TCPResponse** — the server's reply to a [`TcpRequest`].
///
/// Wire layout:
/// ```text
/// [uint8]   status  (0x00 = OK, 0x01 = error)
/// [uint16]  message length
/// [N bytes] message (empty on success, error description on failure)
/// [uint16]  padding length
/// [N bytes] random padding (ignored)
/// ```
#[derive(Debug, Clone)]
pub struct TcpResponse {
    /// `true` if the server successfully opened the connection.
    pub ok: bool,
    /// Human-readable message (empty on success, error text on failure).
    pub message: String,
}

impl TcpResponse {
    /// Create a successful response.
    pub fn success() -> Self {
        TcpResponse { ok: true, message: String::new() }
    }

    /// Create an error response with `msg`.
    pub fn error(msg: impl Into<String>) -> Self {
        TcpResponse { ok: false, message: msg.into() }
    }

    /// Serialize the response into a [`Bytes`] buffer.
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::new();
        buf.put_u8(if self.ok { 0x00 } else { 0x01 });
        let msg = self.message.as_bytes();
        buf.put_u16(msg.len() as u16);
        buf.put_slice(msg);
        buf.put_u16(0u16); // no padding
        buf.freeze()
    }

    /// Deserialize a [`TcpResponse`] from raw bytes.
    pub fn decode(data: Bytes) -> Result<Self> {
        let mut cur = data;
        if cur.remaining() < 3 {
            bail!("TCPResponse too short");
        }
        let status = cur.get_u8();
        let msg_len = cur.get_u16() as usize;
        if cur.remaining() < msg_len {
            bail!("TCPResponse message truncated");
        }
        let mut msg_bytes = vec![0u8; msg_len];
        cur.copy_to_slice(&mut msg_bytes);
        let message = String::from_utf8_lossy(&msg_bytes).into_owned();
        Ok(TcpResponse {
            ok: status == 0x00,
            message,
        })
    }
}

// ─── UDP Relay Wire Format ─────────────────────────────────────────────────

/// **UDPMessage** — wraps a UDP datagram for transport over QUIC unreliable
/// datagrams (RFC 9221).
///
/// Large UDP payloads are split into multiple [`UdpMessage`] fragments all
/// sharing the same `session_id` and `packet_id`, differentiated by
/// `fragment_id` (0-based) and `fragment_count`.
///
/// Wire layout (big-endian):
/// ```text
/// [uint32]        session_id     (per-UDP-session unique ID)
/// [uint16]        packet_id      (per-packet unique ID within a session)
/// [uint8]         fragment_id    (0-based index of this fragment)
/// [uint8]         fragment_count (total number of fragments)
/// [HysteriaAddr]  destination address
/// [bytes]         payload (the UDP data slice for this fragment)
/// ```
#[derive(Debug, Clone)]
pub struct UdpMessage {
    /// Opaque session identifier; all fragments of a datagram share this.
    pub session_id: u32,
    /// Packet counter within the session (used for fragment reassembly).
    pub packet_id: u16,
    /// Zero-based index of this fragment.
    pub fragment_id: u8,
    /// Total number of fragments the full datagram was split into.
    pub fragment_count: u8,
    /// Destination address for the UDP datagram.
    pub addr: HysteriaAddr,
    /// The payload slice carried by this fragment.
    pub data: Bytes,
}

impl UdpMessage {
    /// Maximum QUIC datagram payload that avoids IP fragmentation (1200 B).
    pub const MAX_DATAGRAM_SIZE: usize = 1200;

    /// Serialize this [`UdpMessage`] into a [`Bytes`] buffer.
    pub fn encode(&self) -> Bytes {
        let mut buf = BytesMut::new();
        buf.put_u32(self.session_id);
        buf.put_u16(self.packet_id);
        buf.put_u8(self.fragment_id);
        buf.put_u8(self.fragment_count);
        self.addr.encode(&mut buf);
        buf.put_slice(&self.data);
        buf.freeze()
    }

    /// Deserialize a [`UdpMessage`] from raw bytes.
    pub fn decode(data: Bytes) -> Result<Self> {
        let mut cur = data;
        if cur.remaining() < 8 {
            bail!("UdpMessage header too short");
        }
        let session_id    = cur.get_u32();
        let packet_id     = cur.get_u16();
        let fragment_id   = cur.get_u8();
        let fragment_count = cur.get_u8();
        let addr = HysteriaAddr::decode(&mut cur)?;
        let data = cur.copy_to_bytes(cur.remaining());
        Ok(UdpMessage { session_id, packet_id, fragment_id, fragment_count, addr, data })
    }

    /// Split `payload` into as many [`UdpMessage`] fragments as needed, each
    /// fitting within [`Self::MAX_DATAGRAM_SIZE`] bytes on the wire.
    ///
    /// The address overhead is estimated conservatively so all fragments are
    /// always within the QUIC datagram limit.
    pub fn fragment(
        session_id: u32,
        packet_id: u16,
        addr: HysteriaAddr,
        payload: Bytes,
    ) -> Vec<Self> {
        // Conservatively estimate the address overhead
        let addr_overhead = 20; // 1 type + 16 ip + 2 port + 1 len (worst case)
        let header_size   = 4 + 2 + 1 + 1 + addr_overhead; // session+packet+frag_id+frag_cnt+addr
        let chunk_size    = Self::MAX_DATAGRAM_SIZE.saturating_sub(header_size);
        let chunk_size    = chunk_size.max(64); // always positive

        let chunks: Vec<Bytes> = payload.chunks(chunk_size)
            .map(|c| Bytes::copy_from_slice(c))
            .collect();
        let total = chunks.len() as u8;

        chunks.into_iter().enumerate().map(|(i, chunk)| {
            UdpMessage {
                session_id,
                packet_id,
                fragment_id: i as u8,
                fragment_count: total,
                addr: addr.clone(),
                data: chunk,
            }
        }).collect()
    }
}

// ─── Salamander Obfuscation ────────────────────────────────────────────────

/// **SalamanderObfuscator** applies per-packet XOR obfuscation using a
/// BLAKE2b-256 keystream derived from a random 8-byte salt and a shared
/// password.
///
/// ```text
/// Encoding:
///   salt    = rand_bytes(8)
///   key     = BLAKE2b-256(salt || password)
///   encoded = salt || XOR(plaintext, cyclic(key))
///
/// Decoding:
///   salt    = encoded[0..8]
///   key     = BLAKE2b-256(salt || password)
///   plain   = XOR(encoded[8..], cyclic(key))
/// ```
///
/// This makes each QUIC UDP packet appear as 8 bytes of random salt followed
/// by XOR-encrypted noise, with no detectable QUIC header patterns.
#[derive(Clone)]
pub struct SalamanderObfuscator {
    /// The pre-shared password used for key derivation.
    password: Vec<u8>,
}

impl SalamanderObfuscator {
    /// Create a new obfuscator with the given password.
    pub fn new(password: impl AsRef<[u8]>) -> Self {
        SalamanderObfuscator { password: password.as_ref().to_vec() }
    }

    /// Obfuscate `plain` into a new [`Vec<u8>`].
    ///
    /// Output layout: `[8 B salt] [N B XOR-encrypted payload]`
    pub fn obfuscate(&self, plain: &[u8]) -> Vec<u8> {
        let mut salt = [0u8; 8];
        rand::thread_rng().fill_bytes(&mut salt);
        let key = self.derive_key(&salt);
        let mut out = Vec::with_capacity(8 + plain.len());
        out.extend_from_slice(&salt);
        out.extend(plain.iter().zip(key.iter().cycle()).map(|(b, k)| b ^ k));
        out
    }

    /// Deobfuscate a packet received from the network.
    ///
    /// Returns an error if the buffer is shorter than 8 bytes (no salt).
    pub fn deobfuscate(&self, data: &[u8]) -> Result<Vec<u8>> {
        if data.len() < 8 {
            bail!("obfuscated packet too short (< 8 bytes)");
        }
        let (salt, payload) = data.split_at(8);
        let key = self.derive_key(salt);
        let plain: Vec<u8> = payload.iter().zip(key.iter().cycle()).map(|(b, k)| b ^ k).collect();
        Ok(plain)
    }

    /// Derive a 32-byte key: `BLAKE2b-256(salt || password)`.
    fn derive_key(&self, salt: &[u8]) -> [u8; 32] {
        let mut h = Blake2b256::new();
        h.update(salt);
        h.update(&self.password);
        let result = h.finalize();
        let mut key = [0u8; 32];
        key.copy_from_slice(&result);
        key
    }
}

// ─── Hysteria2 Authentication Headers ────────────────────────────────────

/// HTTP/3 header names used during the Hysteria2 authentication handshake.
///
/// The client sends a `POST /auth HTTP/3` request with these headers.
/// The server responds with HTTP status **233** and includes the TX header.
pub mod auth_headers {
    /// `Hysteria-Auth`: the authentication credential (password or user:pass).
    pub const AUTH:    &str = "hysteria-auth";
    /// `Hysteria-CC-RX`: client's maximum receive rate in bytes/second (`0` = auto).
    pub const CC_RX:   &str = "hysteria-cc-rx";
    /// `Hysteria-CC-TX`: server's maximum transmit rate in bytes/second (response).
    pub const CC_TX:   &str = "hysteria-cc-tx";
    /// `Hysteria-Padding`: random hex string for request body obfuscation.
    pub const PADDING: &str = "hysteria-padding";
    /// `Hysteria-UDP`: `"true"` if the server supports UDP relay.
    pub const UDP:     &str = "hysteria-udp";
}

/// HTTP status code returned by the Hysteria2 server on successful authentication.
///
/// This non-standard 233 code is chosen so that browsers and other HTTP/3
/// clients that accidentally hit the server receive an unusual response and
/// cannot tell the server is a proxy.
pub const AUTH_SUCCESS_STATUS: u16 = 233;

// ─── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addr_roundtrip_ipv4() {
        let addr = HysteriaAddr::V4("1.2.3.4:443".parse().unwrap());
        let mut buf = BytesMut::new();
        addr.encode(&mut buf);
        let decoded = HysteriaAddr::decode(&mut buf.freeze()).unwrap();
        assert_eq!(decoded, addr);
    }

    #[test]
    fn addr_roundtrip_hostname() {
        let addr = HysteriaAddr::Name("example.com".into(), 443);
        let mut buf = BytesMut::new();
        addr.encode(&mut buf);
        let decoded = HysteriaAddr::decode(&mut buf.freeze()).unwrap();
        assert_eq!(decoded, addr);
    }

    #[test]
    fn tcp_request_roundtrip() {
        let req = TcpRequest { addr: HysteriaAddr::Name("google.com".into(), 80) };
        let enc = req.encode();
        let dec = TcpRequest::decode(enc).unwrap();
        assert_eq!(dec.addr.host(), "google.com");
        assert_eq!(dec.addr.port(), 80);
    }

    #[test]
    fn tcp_response_success_roundtrip() {
        let r = TcpResponse::success();
        let enc = r.encode();
        let dec = TcpResponse::decode(enc).unwrap();
        assert!(dec.ok);
    }

    #[test]
    fn tcp_response_error_roundtrip() {
        let r = TcpResponse::error("connection refused");
        let enc = r.encode();
        let dec = TcpResponse::decode(enc).unwrap();
        assert!(!dec.ok);
        assert!(dec.message.contains("refused"));
    }

    #[test]
    fn udp_message_roundtrip() {
        let msg = UdpMessage {
            session_id: 42,
            packet_id: 7,
            fragment_id: 0,
            fragment_count: 1,
            addr: HysteriaAddr::V4("8.8.8.8:53".parse().unwrap()),
            data: Bytes::from_static(b"hello"),
        };
        let enc = msg.encode();
        let dec = UdpMessage::decode(enc).unwrap();
        assert_eq!(dec.session_id, 42);
        assert_eq!(dec.data.as_ref(), b"hello");
    }

    #[test]
    fn salamander_roundtrip() {
        let obfs = SalamanderObfuscator::new("secret_password");
        let plain = b"this is a QUIC packet";
        let obfuscated = obfs.obfuscate(plain);
        assert_ne!(&obfuscated[8..], plain.as_slice());
        let recovered = obfs.deobfuscate(&obfuscated).unwrap();
        assert_eq!(recovered, plain);
    }

    #[test]
    fn salamander_different_salts() {
        let obfs = SalamanderObfuscator::new("pw");
        let plain = b"test data";
        let enc1 = obfs.obfuscate(plain);
        let enc2 = obfs.obfuscate(plain);
        // Salts are random so outputs differ
        assert_ne!(enc1, enc2);
    }
}
