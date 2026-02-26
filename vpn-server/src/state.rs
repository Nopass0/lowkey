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
use tokio::sync::{Mutex, RwLock};
use vpn_common::VpnCrypto;

// ── Token-bucket rate limiter ─────────────────────────────────────────────────

pub struct Bucket {
    pub tokens: f64,
    pub last_refill: Instant,
}

impl Bucket {
    pub fn new(capacity: f64) -> Self {
        Bucket { tokens: capacity, last_refill: Instant::now() }
    }

    pub fn consume(&mut self, bytes: usize, limit_bps: u64) -> bool {
        if limit_bps == 0 {
            return true;
        }
        let rate = limit_bps as f64;
        let now = Instant::now();
        let elapsed = now.duration_since(self.last_refill).as_secs_f64();
        self.last_refill = now;
        self.tokens = (self.tokens + elapsed * rate).min(rate); // 1-s burst
        if self.tokens >= bytes as f64 {
            self.tokens -= bytes as f64;
            true
        } else {
            false
        }
    }
}

// ── Live VPN peer ─────────────────────────────────────────────────────────────

pub struct Peer {
    pub vpn_ip: Ipv4Addr,
    pub endpoint: RwLock<Option<SocketAddr>>,
    pub crypto: VpnCrypto,

    pub bytes_in: AtomicU64,
    pub bytes_out: AtomicU64,
    pub speed_in_bps: AtomicU64,
    pub speed_out_bps: AtomicU64,
    pub limit_bps: AtomicU64, // 0 = unlimited

    pub bucket: Mutex<Bucket>,
    pub connected_at: Instant,

    pub user_id: Option<i32>,
}

impl Peer {
    pub fn new(vpn_ip: Ipv4Addr, crypto: VpnCrypto, user_id: Option<i32>) -> Arc<Self> {
        Arc::new(Peer {
            vpn_ip,
            endpoint: RwLock::new(None),
            crypto,
            bytes_in: AtomicU64::new(0),
            bytes_out: AtomicU64::new(0),
            speed_in_bps: AtomicU64::new(0),
            speed_out_bps: AtomicU64::new(0),
            limit_bps: AtomicU64::new(0),
            bucket: Mutex::new(Bucket::new(1_000_000.0)),
            connected_at: Instant::now(),
            user_id,
        })
    }

    pub fn bytes_in(&self) -> u64  { self.bytes_in.load(Ordering::Relaxed) }
    pub fn bytes_out(&self) -> u64 { self.bytes_out.load(Ordering::Relaxed) }
    pub fn speed_in(&self) -> u64  { self.speed_in_bps.load(Ordering::Relaxed) }
    pub fn speed_out(&self) -> u64 { self.speed_out_bps.load(Ordering::Relaxed) }
    pub fn limit(&self) -> u64     { self.limit_bps.load(Ordering::Relaxed) }
}

// ── Server state ──────────────────────────────────────────────────────────────

pub struct ServerState {
    // ── VPN runtime ──────────────────────────────────────────────────────────
    pub peers: DashMap<Ipv4Addr, Arc<Peer>>,
    pub endpoints: DashMap<SocketAddr, Ipv4Addr>,
    pub next_octet: Mutex<u8>,

    // ── Server identity ───────────────────────────────────────────────────────
    pub server_secret: [u8; 32],
    pub server_pubkey: [u8; 32],
    pub psk: String,          // legacy PSK (kept for direct peers)

    // ── Ports ─────────────────────────────────────────────────────────────────
    pub udp_port: u16,
    pub proxy_port: u16,

    // ── Detected IPs ─────────────────────────────────────────────────────────
    pub public_ip: RwLock<String>,
    pub local_ip: RwLock<String>,

    // ── Statistics ────────────────────────────────────────────────────────────
    pub start_time: Instant,
    pub total_bytes_in: AtomicU64,
    pub total_bytes_out: AtomicU64,

    // ── Dashboard log ring-buffer ─────────────────────────────────────────────
    pub logs: Mutex<VecDeque<String>>,

    // ── Database ──────────────────────────────────────────────────────────────
    pub pool: PgPool,

    // ── Auth ──────────────────────────────────────────────────────────────────
    pub jwt_secret: String,

    // ── Telegram ─────────────────────────────────────────────────────────────
    pub tg_bot_token: Option<String>,
    pub tg_admin_chat_id: Option<String>,
}

impl ServerState {
    pub fn push_log(&self, msg: String) {
        if let Ok(mut buf) = self.logs.try_lock() {
            if buf.len() >= 200 { buf.pop_front(); }
            buf.push_back(msg);
        }
    }

    pub fn uptime_secs(&self) -> u64 { self.start_time.elapsed().as_secs() }
    pub fn total_in(&self) -> u64    { self.total_bytes_in.load(Ordering::Relaxed) }
    pub fn total_out(&self) -> u64   { self.total_bytes_out.load(Ordering::Relaxed) }
}

pub type Shared = Arc<ServerState>;
