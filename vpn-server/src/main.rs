//! Lowkey VPN Server — entry point.
//!
//! # Startup sequence
//! 1. Detect TTY and configure tracing (file log when TUI is active).
//! 2. Parse CLI arguments / environment variables.
//! 3. Connect to PostgreSQL and run migrations.
//! 4. Generate an ephemeral X25519 server keypair.
//! 5. Auto-detect local and public IP addresses.
//! 6. Build shared [`ServerState`].
//! 7. Create a TUN device and configure NAT via iptables.
//! 8. Bind the UDP tunnel socket.
//! 9. Spawn tunnel tasks (TUN↔UDP bidirectional forwarding).
//! 10. Spawn the TCP proxy server.
//! 11. Start the HTTP API server (axum).
//! 12. Either run the TUI dashboard or wait for Ctrl-C.
//!
//! # SSH / non-TTY safety
//! The TUI dashboard uses `crossterm::enable_raw_mode()` which hijacks the
//! terminal.  We guard it with:
//! ```rust,ignore
//! let use_tui = !args.no_tui && std::io::IsTerminal::is_terminal(&std::io::stdout());
//! ```
//! When running over SSH or under systemd, `stdout` is not a TTY so the
//! check fails and the server falls back to structured log output + Ctrl-C.

mod admin_api;
mod api;
mod auth_middleware;
mod dashboard;
mod db;
mod models;
mod payment_api;
mod proxy;
mod referral_api;
mod state;
mod telegram;
mod tunnel;
mod user_api;
mod ws_tunnel;

use std::{
    collections::VecDeque,
    sync::{atomic::AtomicU64, Arc},
};

use anyhow::{Context, Result};
use axum::{
    routing::{delete, get, post, put},
    Router,
};
use clap::Parser;
use dashmap::DashMap;
use tokio::{io::AsyncWriteExt, net::UdpSocket, sync::{mpsc, Mutex, RwLock}, time::Duration};
use tower_http::cors::CorsLayer;
use tracing::info;

use vpn_common::{to_hex, DEFAULT_API_PORT, DEFAULT_PROXY_PORT, DEFAULT_UDP_PORT, VPN_NETMASK, VPN_SERVER_IP, VPN_SUBNET};
use x25519_dalek::{PublicKey, StaticSecret};

use state::{ServerState, Shared};

// ── CLI arguments ─────────────────────────────────────────────────────────────

/// All runtime configuration is accepted both as CLI flags and as environment
/// variables (thanks to the `env` feature of clap).
#[derive(Parser, Debug)]
#[command(name = "vpn-server", about = "Lowkey VPN Server")]
struct Args {
    /// HTTP API listen port.
    #[arg(long, default_value_t = DEFAULT_API_PORT)]
    api_port: u16,

    /// UDP tunnel listen port.
    #[arg(long, default_value_t = DEFAULT_UDP_PORT)]
    udp_port: u16,

    /// TCP proxy listen port (VLESS/SOCKS5 clients).
    #[arg(long, default_value_t = DEFAULT_PROXY_PORT)]
    proxy_port: u16,

    /// Pre-shared key for the legacy UDP tunnel handshake.
    /// Also used as the PSK auth token in the TCP proxy protocol.
    #[arg(long, env = "VPN_PSK", default_value = "changeme")]
    psk: String,

    /// PostgreSQL connection URL.
    /// Example: `postgres://user:pass@localhost/lowkey`
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    /// JWT HMAC signing secret.  Change this in production!
    #[arg(long, env = "JWT_SECRET", default_value = "change-this-secret")]
    jwt_secret: String,

    /// Telegram Bot API token.  Required for admin OTP login.
    #[arg(long, env = "TG_BOT_TOKEN")]
    tg_bot_token: Option<String>,

    /// Telegram admin chat ID.  The bot sends OTP codes here.
    #[arg(long, env = "TG_ADMIN_CHAT_ID")]
    tg_admin_chat_id: Option<String>,

    /// Tochka Bank JWT token for SBP payments.
    #[arg(long, env = "TOCHKA_JWT")]
    tochka_jwt: Option<String>,

    /// Tochka Bank merchant ID.
    #[arg(long, env = "TOCHKA_MERCHANT_ID")]
    tochka_merchant_id: Option<String>,

    /// Tochka Bank legal entity ID.
    #[arg(long, env = "TOCHKA_LEGAL_ID")]
    tochka_legal_id: Option<String>,

    /// Disable the TUI dashboard.  Useful under SSH, systemd or in CI.
    #[arg(long, default_value_t = false)]
    no_tui: bool,
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // ── Tracing setup ─────────────────────────────────────────────────────────
    //
    // Decide before parsing args because `--no-tui` would not be available yet.
    // We use the same early-check logic: is stdout a real TTY?
    let use_tui = !std::env::args().any(|a| a == "--no-tui")
        && std::io::IsTerminal::is_terminal(&std::io::stdout());

    if use_tui {
        // When TUI is active, direct all tracing output to a log file so it
        // doesn't corrupt the alternate-screen display.
        let log_file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("vpn-server.log")
            .unwrap_or_else(|_| std::fs::File::create("/dev/null").unwrap());
        tracing_subscriber::fmt()
            .with_writer(std::sync::Mutex::new(log_file))
            .with_env_filter("info")
            .init();
    } else {
        // Non-TTY: plain structured output to stdout/stderr.
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive("vpn_server=info".parse()?),
            )
            .init();
    }

    let args = Args::parse();

    // ── Database ──────────────────────────────────────────────────────────────
    let pool = db::create_pool(&args.database_url).await?;
    db::run_migrations(&pool).await?;

    // ── Server keypair ────────────────────────────────────────────────────────
    // A fresh ephemeral keypair is generated on every start.  Clients
    // re-register each session so there is no need for key persistence.
    let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let public = PublicKey::from(&secret);
    let server_secret = secret.to_bytes();
    let server_pubkey = *public.as_bytes();
    info!("Server public key: {}", to_hex(&server_pubkey));

    // ── IP detection ──────────────────────────────────────────────────────────
    let local_ip  = detect_local_ip();
    let public_ip = detect_public_ip().await.unwrap_or_else(|| local_ip.clone());
    info!("Local: {local_ip}  Public: {public_ip}");

    // ── TUN interface (created before state so the inject channel can be wired in) ─
    let mut tun_config = tun::Configuration::default();
    tun_config
        .address(VPN_SERVER_IP)
        .netmask(VPN_NETMASK)
        .destination(VPN_SUBNET)
        .up();
    #[cfg(target_os = "linux")]
    tun_config.platform(|c| { c.packet_information(false); });

    let tun_dev = tun::create_as_async(&tun_config)
        .context("TUN creation failed — run as root / grant CAP_NET_ADMIN")?;
    info!("TUN up ({})", VPN_SERVER_IP);

    // Configure iptables MASQUERADE so VPN traffic can reach the internet
    setup_nat().context("iptables/ip_forward failed")?;

    // ── TUN inject channel ────────────────────────────────────────────────────
    // WebSocket handlers push decrypted plaintext IP packets here; a dedicated
    // task drains the channel and writes them directly into the TUN device.
    // The sender is stored in ServerState so any handler can reach it.
    let (tun_inject_tx, mut tun_inject_rx) = mpsc::unbounded_channel::<Vec<u8>>();

    // ── Shared state ──────────────────────────────────────────────────────────
    let state: Shared = Arc::new(ServerState {
        peers:       DashMap::new(),
        endpoints:   DashMap::new(),
        next_octet:  Mutex::new(2),  // start IP allocation at 10.0.0.2
        server_secret,
        server_pubkey,
        psk:         args.psk.clone(),
        udp_port:    args.udp_port,
        proxy_port:  args.proxy_port,
        public_ip:   RwLock::new(format!("{public_ip}:{}", args.udp_port)),
        local_ip:    RwLock::new(local_ip),
        start_time:  std::time::Instant::now(),
        total_bytes_in:  AtomicU64::new(0),
        total_bytes_out: AtomicU64::new(0),
        logs:        Mutex::new(VecDeque::new()),
        ws_peers:    DashMap::new(),
        tun_inject:  tun_inject_tx,
        pool,
        jwt_secret:          args.jwt_secret.clone(),
        tg_bot_token:        args.tg_bot_token.clone(),
        tg_admin_chat_id:    args.tg_admin_chat_id.clone(),
        tochka_jwt:          args.tochka_jwt.clone(),
        tochka_merchant_id:  args.tochka_merchant_id.clone(),
        tochka_legal_id:     args.tochka_legal_id.clone(),
    });
    state.push_log(format!("TUN up — {VPN_SERVER_IP}/24"));

    // ── UDP socket ────────────────────────────────────────────────────────────
    let udp = Arc::new(
        UdpSocket::bind(format!("0.0.0.0:{}", args.udp_port)).await
            .context("UDP bind failed")?,
    );
    info!("UDP tunnel on :{}", args.udp_port);
    state.push_log(format!("UDP on :{}", args.udp_port));

    // Split TUN into read/write halves; protect the write half with a Mutex
    // so the UDP→TUN task and the WS inject task can share it safely.
    let (tun_rx, tun_tx) = tokio::io::split(tun_dev);
    let tun_tx = Arc::new(Mutex::new(tun_tx));

    // Drain the tun_inject channel into the TUN write half
    {
        let tw = tun_tx.clone();
        tokio::spawn(async move {
            while let Some(pkt) = tun_inject_rx.recv().await {
                let mut w = tw.lock().await;
                if let Err(e) = w.write_all(&pkt).await {
                    tracing::error!("TUN inject write error: {e}");
                }
            }
        });
    }

    // ── Tunnel tasks ──────────────────────────────────────────────────────────
    {
        let (s, u) = (state.clone(), udp.clone());
        tokio::spawn(async move {
            if let Err(e) = tunnel::task_tun_to_udp(tun_rx, u, s).await {
                tracing::error!("TUN→peer task died: {e}");
            }
        });
    }
    {
        let (s, u, tw) = (state.clone(), udp.clone(), tun_tx.clone());
        tokio::spawn(async move {
            if let Err(e) = tunnel::task_udp_to_tun(u, tw, s).await {
                tracing::error!("UDP→TUN task died: {e}");
            }
        });
    }

    // ── TCP proxy ─────────────────────────────────────────────────────────────
    {
        let s = state.clone();
        tokio::spawn(async move {
            if let Err(e) = proxy::run_proxy_server(s).await {
                tracing::error!("Proxy task died: {e}");
            }
        });
    }

    // ── HTTP API ──────────────────────────────────────────────────────────────
    let app = Router::new()
        // ── Auth ──────────────────────────────────────────────────────────────
        .route("/auth/register",         post(user_api::register))
        .route("/auth/login",            post(user_api::login))
        .route("/auth/me",               get(user_api::me))
        // ── Subscription & promos ─────────────────────────────────────────────
        .route("/subscription/plans",    get(user_api::list_plans))
        .route("/subscription/buy",      post(user_api::buy_subscription))
        .route("/subscription/status",   get(user_api::subscription_status))
        .route("/promo/apply",           post(user_api::apply_promo))
        // ── VPN peer management (requires JWT + active subscription) ──────────
        .route("/api/status",            get(api::api_status))
        .route("/api/peers",             get(api::api_list_peers))
        .route("/api/peers/register",    post(api::api_register))
        .route("/api/peers/:ip",         delete(api::api_remove_peer))
        .route("/api/peers/:ip/limit",   put(api::api_set_limit))
        // ── SBP Payments ──────────────────────────────────────────────────────
        .route("/payment/sbp/create",              post(payment_api::create_sbp_payment))
        .route("/payment/sbp/status/:id",          get(payment_api::get_payment_status))
        .route("/payment/webhook",                 post(payment_api::payment_webhook))
        .route("/payment/history",                 get(payment_api::payment_history))
        // ── Referral system ───────────────────────────────────────────────────
        .route("/referral/stats",                  get(referral_api::referral_stats))
        .route("/referral/withdraw",               post(referral_api::request_withdrawal))
        .route("/referral/withdrawals",            get(referral_api::list_withdrawals))
        // ── Admin endpoints ───────────────────────────────────────────────────
        .route("/admin/request-code",              post(admin_api::request_code))
        .route("/admin/verify-code",               post(admin_api::verify_code))
        .route("/admin/promos",                    post(admin_api::create_promo))
        .route("/admin/promos/list",               get(admin_api::list_promos))
        .route("/admin/promos/:id",                delete(admin_api::delete_promo))
        .route("/admin/users",                     get(admin_api::list_users))
        .route("/admin/users/:id/limit",           put(admin_api::set_user_limit))
        .route("/admin/users/:id/ban",             put(admin_api::ban_user))
        .route("/admin/peers",                     get(admin_api::list_peers))
        .route("/admin/stats",                     get(referral_api::admin_stats))
        .route("/admin/payments",                  get(payment_api::admin_list_payments))
        .route("/admin/payment/:id/confirm",       post(payment_api::admin_confirm_payment))
        .route("/admin/referral/withdrawals",      get(referral_api::admin_list_withdrawals))
        .route("/admin/referral/withdrawals/:id/approve", put(referral_api::admin_approve_withdrawal))
        .route("/admin/referral/withdrawals/:id/reject",  put(referral_api::admin_reject_withdrawal))
        .route("/admin/plans",                     get(referral_api::admin_list_plans))
        .route("/admin/plans/:key/price",          put(referral_api::admin_update_plan_price))
        // ── WebSocket VPN tunnel (firewall-bypass transport) ──────────────────
        .route("/ws-tunnel", get(ws_tunnel::ws_handler))
        // CORS: allow all origins so web-based admin panels can talk to the API
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    let api_addr = format!("0.0.0.0:{}", args.api_port);
    info!("HTTP API on {api_addr}");
    state.push_log(format!("API on {api_addr}"));

    let listener = tokio::net::TcpListener::bind(&api_addr).await?;
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await {
            tracing::error!("HTTP server error: {e}");
        }
    });

    // ── Main loop ─────────────────────────────────────────────────────────────
    if use_tui && !args.no_tui {
        // Interactive TUI dashboard — blocks until the user presses q/Esc
        dashboard::run_dashboard(state).await?;
    } else {
        // Headless mode — just wait for Ctrl-C
        info!("Server running. Ctrl-C to stop. Logs → vpn-server.log");
        tokio::signal::ctrl_c().await?;
    }

    info!("Shutting down.");
    Ok(())
}

// ── NAT configuration ─────────────────────────────────────────────────────────

/// Enable IP forwarding and configure iptables MASQUERADE for the VPN subnet.
///
/// This allows VPN clients (10.0.0.x) to reach the internet through the
/// server's public network interface.
///
/// The MASQUERADE rule is first deleted (idempotent), then re-added, so the
/// function is safe to call on restart.
fn setup_nat() -> Result<()> {
    use std::process::Command;

    // Enable IPv4 packet forwarding in the kernel
    std::fs::write("/proc/sys/net/ipv4/ip_forward", "1")
        .context("Cannot enable IP forwarding")?;

    // Remove any existing MASQUERADE rule to avoid duplicates on restart
    let _ = Command::new("iptables")
        .args(["-t", "nat", "-D", "POSTROUTING", "-s", vpn_common::VPN_SUBNET_CIDR, "!", "-o", "tun0", "-j", "MASQUERADE"])
        .output();

    // Add the MASQUERADE rule: NAT outgoing traffic from the VPN subnet
    Command::new("iptables")
        .args(["-t", "nat", "-A", "POSTROUTING", "-s", vpn_common::VPN_SUBNET_CIDR, "!", "-o", "tun0", "-j", "MASQUERADE"])
        .output()
        .context("iptables MASQUERADE")?;

    // Allow forwarding through the tun0 interface in both directions
    for dir in ["-i", "-o"] {
        let _ = Command::new("iptables").args(["-D", "FORWARD", dir, "tun0", "-j", "ACCEPT"]).output();
        Command::new("iptables").args(["-A", "FORWARD", dir, "tun0", "-j", "ACCEPT"])
            .output()
            .with_context(|| format!("FORWARD {dir} tun0"))?;
    }
    info!("NAT configured");
    Ok(())
}

// ── IP detection ──────────────────────────────────────────────────────────────

/// Detect the machine's default outbound local IP address.
///
/// Uses a UDP socket trick: connecting to `1.1.1.1:80` (no packets are sent)
/// forces the OS to select the appropriate source address, which we then read
/// back via `local_addr()`.  Falls back to `"unknown"` if this fails.
fn detect_local_ip() -> String {
    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
        let _ = sock.connect("1.1.1.1:80");
        if let Ok(addr) = sock.local_addr() {
            return addr.ip().to_string();
        }
    }
    "unknown".into()
}

/// Detect the machine's public (internet-facing) IP address.
///
/// Tries two public IP echo services with a 3-second timeout each.
/// Returns `None` if all attempts fail (no internet, behind CGNAT with no
/// public IP, etc.).
async fn detect_public_ip() -> Option<String> {
    for url in &["https://api4.my-ip.io/ip", "https://api.ipify.org"] {
        if let Ok(resp) = reqwest::Client::new()
            .get(*url)
            .timeout(Duration::from_secs(3))
            .send()
            .await
        {
            if let Ok(t) = resp.text().await {
                let ip = t.trim().to_string();
                // Sanity-check: a valid IPv4 address is 7–15 characters
                if !ip.is_empty() && ip.len() < 20 {
                    return Some(ip);
                }
            }
        }
    }
    None
}
