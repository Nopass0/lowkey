//! Shared server state — all data that is read and mutated by concurrent tasks.
//!
//! The entire state is wrapped in [`Arc<ServerState>`] (alias [`Shared`]) and
//! cloned cheaply whenever a new task or HTTP handler needs access.
//!
//! # Thread-safety model
//! * **Lock-free counters** — [`AtomicU64`] for bytes_in/out, speed snapshots
//!   and bandwidth limits.  These can be read and written without any mutex.
//! * **DashMap** — concurrent hashmap for the peer table and the
//!   endpoint→VPN-IP reverse index.  Multiple readers/writers are safe
//!   simultaneously.
//! * **RwLock** — for the public and local IP strings (rarely written,
//!   frequently read by the dashboard).
//! * **Mutex** — for the per-peer token bucket (rate limiter) and the log
//!   ring-buffer.

use std::{
    collections::VecDeque,
    net::{Ipv4Addr, SocketAddr},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
    time::Instant,
};

use dashmap::DashMap;
use sqlx::PgPool;
use tokio::sync::{mpsc, Mutex, RwLock};
use vpn_common::VpnCrypto;

// ── Token-bucket rate limiter ─────────────────────────────────────────────────

/// A simple token-bucket that enforces a per-peer bandwidth cap.
///
/// ## Algorithm
/// ```text
/// tokens += elapsed_seconds × rate_bps   (capped at rate_bps = 1-second burst)
/// if tokens >= packet_bytes → allow, deduct
/// else                      → drop packet
/// ```
///
/// The burst capacity equals exactly 1 second of traffic at the configured
/// rate.  This prevents bursty start-up while allowing sustained wire-speed
/// within the limit.
pub struct Bucket {
    /// Available token balance (in bytes).
    pub tokens: f64,
    /// Wall-clock time of the last refill, used to compute elapsed seconds.
    pub last_refill: Instant,
}

impl Bucket {
    /// Create a new bucket pre-filled with `capacity` bytes.
    ///
    /// Called once when a [`Peer`] is registered.  The initial capacity is
    /// set to a generous default; the actual limit is applied when
    /// [`consume`](Bucket::consume) is first called.
    pub fn new(capacity: f64) -> Self {
        Bucket { tokens: capacity, last_refill: Instant::now() }
    }

    /// Try to consume `bytes` from the bucket.
    ///
    /// Returns `true` if the packet should be forwarded, `false` if it
    /// should be dropped (rate-limited).
    ///
    /// When `limit_bps == 0` the bucket is bypassed and all packets are
    /// allowed (unlimited mode).
    pub fn consume(&mut self, bytes: usize, limit_bps: u64) -> bool {
        // 0 = unlimited — skip all rate-limiting logic
        if limit_bps == 0 {
            return true;
        }

        let rate = limit_bps as f64;
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;

        // Refill tokens proportional to elapsed time; cap at 1-second burst
        self.tokens = (self.tokens + elapsed * rate).min(rate);

        if self.tokens >= bytes as f64 {
            self.tokens -= bytes as f64;
            true  // packet allowed
        } else {
            false // packet dropped
        }
    }
}

// ── Live VPN peer ─────────────────────────────────────────────────────────────

/// State for one connected VPN peer.
///
/// Each peer has its own encryption context, traffic counters, speed
/// snapshots and a token-bucket rate limiter.  The struct is wrapped in
/// [`Arc`] so it can be shared across the tunnel tasks and HTTP handlers.
pub struct Peer {
    /// Assigned VPN IP for this session (e.g. `10.0.0.5`).
    pub vpn_ip: Ipv4Addr,

    /// Last known UDP source address of this client.
    ///
    /// `None` until the first encrypted packet arrives from the client.
    /// Protected by `RwLock` because the tunnel read task updates it while
    /// the tunnel write task and the dashboard read it concurrently.
    pub endpoint: RwLock<Option<SocketAddr>>,

    /// Session-specific AEAD crypto (X25519-derived key).
    pub crypto: VpnCrypto,

    /// Cumulative bytes received from this peer (UDP → TUN direction).
    pub bytes_in: AtomicU64,

    /// Cumulative bytes sent to this peer (TUN → UDP direction).
    pub bytes_out: AtomicU64,

    /// Download speed snapshot in bytes/s (updated every second by the
    /// dashboard task).
    pub speed_in_bps: AtomicU64,

    /// Upload speed snapshot in bytes/s (updated every second by the
    /// dashboard task).
    pub speed_out_bps: AtomicU64,

    /// Active bandwidth cap in bytes/s.  `0` means unlimited.
    /// Can be changed live via `PUT /api/peers/:ip/limit`.
    pub limit_bps: AtomicU64,

    /// Token-bucket state for rate limiting outgoing (server→client) packets.
    pub bucket: Mutex<Bucket>,

    /// Wall-clock time when this peer registered, used for uptime display.
    pub connected_at: Instant,

    /// Database user ID that owns this peer session (`None` for legacy
    /// PSK-only peers that predate the user-auth system).
    pub user_id: Option<i32>,
}

impl Peer {
    /// Allocate and initialise a new peer, returning it wrapped in [`Arc`].
    pub fn new(vpn_ip: Ipv4Addr, crypto: VpnCrypto, user_id: Option<i32>) -> Arc<Self> {
        Arc::new(Peer {
            vpn_ip,
            endpoint: RwLock::new(None),
            crypto,
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            speed_in_bps: AtomicU64::new(0),
            speed_out_bps: AtomicU64::new(0),
            // Starts as unlimited; caller sets the real limit after construction
            limit_bps: AtomicU64::new(0),
            // Pre-fill bucket with 1 MB to avoid throttling the burst at connect
            bucket: Mutex::new(Bucket::new(1_000_000.0)),
            connected_at: Instant::now(),
            user_id,
        })
    }

    // ── Convenience accessors (avoids raw Ordering boilerplate at call sites) ─

    /// Bytes received from this peer (total since registration).
    pub fn bytes_in(&self) -> u64  { self.bytes_in.load(Ordering::Relaxed) }

    /// Bytes sent to this peer (total since registration).
    pub fn bytes_out(&self) -> u64 { self.bytes_out.load(Ordering::Relaxed) }

    /// Instantaneous download speed in bytes/s.
    pub fn speed_in(&self) -> u64  { self.speed_in_bps.load(Ordering::Relaxed) }

    /// Instantaneous upload speed in bytes/s.
    pub fn speed_out(&self) -> u64 { self.speed_out_bps.load(Ordering::Relaxed) }

    /// Active bandwidth cap in bytes/s (`0` = unlimited).
    pub fn limit(&self) -> u64     { self.limit_bps.load(Ordering::Relaxed) }
}

// ── Server state ──────────────────────────────────────────────────────────────

/// Global server state shared across all async tasks and HTTP handlers.
///
/// Access this through the [`Shared`] type alias (`Arc<ServerState>`).
pub struct ServerState {
    // ── VPN runtime ──────────────────────────────────────────────────────────

    /// All currently registered VPN peers, keyed by their assigned VPN IP.
    pub peers: DashMap<Ipv4Addr, Arc<Peer>>,

    /// Reverse index: UDP endpoint → VPN IP.
    ///
    /// Used in the UDP receive path to look up the peer for an incoming
    /// packet when the sender address is known but the VPN IP is not.
    pub endpoints: DashMap<SocketAddr, Ipv4Addr>,

    /// Monotonically increasing counter for assigning the next VPN IP octet
    /// in the `10.0.0.x` range (wraps around after 254).
    pub next_octet: Mutex<u8>,

    // ── Server identity ───────────────────────────────────────────────────────

    /// Server X25519 static private key (bytes).
    ///
    /// Used in the proxy handshake to derive per-session shared keys.
    pub server_secret: [u8; 32],

    /// Server X25519 static public key (bytes).
    ///
    /// Sent to clients during peer registration so they can complete the DH
    /// handshake and derive the UDP session key.
    pub server_pubkey: [u8; 32],

    /// Legacy pre-shared key string — optional check for direct (non-user)
    /// peer registrations.
    pub psk: String,

    // ── Listening ports ───────────────────────────────────────────────────────

    /// UDP tunnel port (clients send encrypted IP packets here).
    pub udp_port: u16,

    /// TCP proxy port (VLESS/SOCKS5 clients connect here).
    pub proxy_port: u16,

    // ── Detected IPs ─────────────────────────────────────────────────────────

    /// Auto-detected public IP + UDP port (e.g. `"1.2.3.4:51820"`).
    /// Displayed in the dashboard header.
    pub public_ip: RwLock<String>,

    /// Auto-detected LAN IP of the machine.
    pub local_ip: RwLock<String>,

    // ── Aggregate statistics ──────────────────────────────────────────────────

    /// Server startup time — basis for uptime calculation.
    pub start_time: Instant,

    /// Total bytes received through the UDP tunnel (all peers combined).
    pub total_bytes_in: AtomicU64,

    /// Total bytes sent through the UDP tunnel (all peers combined).
    pub total_bytes_out: AtomicU64,

    // ── Dashboard log ring-buffer ─────────────────────────────────────────────

    /// Bounded ring-buffer of the last 200 log messages.
    ///
    /// Written by tunnel tasks and API handlers; read by the TUI dashboard.
    /// Uses a non-blocking `try_lock` so slow log writes never stall the
    /// data path.
    pub logs: Mutex<VecDeque<String>>,

    // ── WebSocket tunnel support ──────────────────────────────────────────────

    /// VPN IP → channel sender for WebSocket-mode peers.
    ///
    /// When the TUN→peer task wants to deliver a packet to a WS client it
    /// looks up the sender here and pushes the already-encrypted payload.
    /// The WS handler on the other end reads from the matching receiver and
    /// writes it as a binary WebSocket frame.
    pub ws_peers: DashMap<Ipv4Addr, mpsc::UnboundedSender<Vec<u8>>>,

    /// Inject decrypted IP packets into the TUN device from WebSocket handlers.
    ///
    /// WS handlers push plaintext IP packets here; a dedicated task drains
    /// the channel and writes them to the TUN write-half.
    pub tun_inject: mpsc::UnboundedSender<Vec<u8>>,

    // ── PostgreSQL connection pool ─────────────────────────────────────────────

    /// Shared sqlx connection pool (up to 20 connections).
    pub pool: PgPool,

    // ── Authentication ────────────────────────────────────────────────────────

    /// HMAC secret used to sign and verify JWT tokens.
    pub jwt_secret: String,

    // ── Telegram integration ──────────────────────────────────────────────────

    /// Telegram Bot API token (`123456:AABBcc…`).  `None` if admin OTP is
    /// disabled.
    pub tg_bot_token: Option<String>,

    /// Telegram chat ID of the admin who receives OTP codes.  `None` if admin
    /// OTP is disabled.
    pub tg_admin_chat_id: Option<String>,

    // ── Tochka Bank SBP payment integration ───────────────────────────────────

    /// Tochka Bank JWT token for SBP API authentication.
    pub tochka_jwt: Option<String>,

    /// Tochka Bank merchant ID.
    pub tochka_merchant_id: Option<String>,

    /// Tochka Bank legal entity ID.
    pub tochka_legal_id: Option<String>,
}

impl ServerState {
    /// Append a message to the in-memory log ring-buffer.
    ///
    /// Uses `try_lock` (non-blocking) to avoid stalling the data path if the
    /// dashboard is currently iterating the buffer.  Messages are silently
    /// dropped if the lock is contended, which is acceptable for log entries.
    ///
    /// The buffer is capped at 200 entries — the oldest entry is discarded
    /// when full.
    pub fn push_log(&self, msg: String) {
        if let Ok(mut buf) = self.logs.try_lock() {
            if buf.len() >= 200 {
                buf.pop_front(); // evict oldest entry
            }
            buf.push_back(msg);
        }
    }

    /// Server uptime in whole seconds since [`start_time`](Self::start_time).
    pub fn uptime_secs(&self) -> u64 { self.start_time.elapsed().as_secs() }

    /// Total bytes received through the UDP tunnel.
    pub fn total_in(&self) -> u64    { self.total_bytes_in.load(Ordering::Relaxed) }

    /// Total bytes sent through the UDP tunnel.
    pub fn total_out(&self) -> u64   { self.total_bytes_out.load(Ordering::Relaxed) }
}

/// Type alias — the server state is always heap-allocated and ref-counted.
pub type Shared = Arc<ServerState>;
