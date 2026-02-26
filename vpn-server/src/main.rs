mod admin_api;
mod api;
mod auth_middleware;
mod dashboard;
mod db;
mod models;
mod proxy;
mod state;
mod telegram;
mod tunnel;
mod user_api;

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
use tokio::{net::UdpSocket, sync::{Mutex, RwLock}, time::Duration};
use tower_http::cors::CorsLayer;
use tracing::info;

use vpn_common::{to_hex, DEFAULT_API_PORT, DEFAULT_PROXY_PORT, DEFAULT_UDP_PORT, VPN_NETMASK, VPN_SERVER_IP, VPN_SUBNET};
use x25519_dalek::{PublicKey, StaticSecret};

use state::{ServerState, Shared};

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(name = "vpn-server", about = "Lowkey VPN Server")]
struct Args {
    #[arg(long, default_value_t = DEFAULT_API_PORT)]
    api_port: u16,

    #[arg(long, default_value_t = DEFAULT_UDP_PORT)]
    udp_port: u16,

    #[arg(long, default_value_t = DEFAULT_PROXY_PORT)]
    proxy_port: u16,

    /// Legacy PSK for direct (non-user) peers
    #[arg(long, env = "VPN_PSK", default_value = "changeme")]
    psk: String,

    /// PostgreSQL connection URL
    #[arg(long, env = "DATABASE_URL")]
    database_url: String,

    /// JWT signing secret
    #[arg(long, env = "JWT_SECRET", default_value = "change-this-secret")]
    jwt_secret: String,

    /// Telegram bot token (optional — needed for admin OTP)
    #[arg(long, env = "TG_BOT_TOKEN")]
    tg_bot_token: Option<String>,

    /// Telegram admin chat ID (optional)
    #[arg(long, env = "TG_ADMIN_CHAT_ID")]
    tg_admin_chat_id: Option<String>,

    /// Disable TUI (useful when running under SSH or systemd)
    #[arg(long, default_value_t = false)]
    no_tui: bool,
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    // Set up tracing BEFORE anything else; when TUI is active we redirect
    // logs to a file so they don't corrupt the alternate-screen display.
    let use_tui = !std::env::args().any(|a| a == "--no-tui")
        && std::io::IsTerminal::is_terminal(&std::io::stdout());

    if use_tui {
        let log_file = std::fs::OpenOptions::new()
            .create(true).append(true)
            .open("vpn-server.log")
            .unwrap_or_else(|_| std::fs::File::create("/dev/null").unwrap());
        tracing_subscriber::fmt()
            .with_writer(std::sync::Mutex::new(log_file))
            .with_env_filter("info")
            .init();
    } else {
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::from_default_env()
                    .add_directive("vpn_server=info".parse()?),
            )
            .init();
    }

    let args = Args::parse();

    // ── Database ─────────────────────────────────────────────────────────────
    let pool = db::create_pool(&args.database_url).await?;
    db::run_migrations(&pool).await?;

    // ── Server keypair ────────────────────────────────────────────────────────
    let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let public = PublicKey::from(&secret);
    let server_secret = secret.to_bytes();
    let server_pubkey = *public.as_bytes();
    info!("Server public key: {}", to_hex(&server_pubkey));

    // ── Detect IPs ────────────────────────────────────────────────────────────
    let local_ip = detect_local_ip();
    let public_ip = detect_public_ip().await.unwrap_or_else(|| local_ip.clone());
    info!("Local: {local_ip}  Public: {public_ip}");

    // ── Shared state ──────────────────────────────────────────────────────────
    let state: Shared = Arc::new(ServerState {
        peers: DashMap::new(),
        endpoints: DashMap::new(),
        next_octet: Mutex::new(2),
        server_secret,
        server_pubkey,
        psk: args.psk.clone(),
        udp_port: args.udp_port,
        proxy_port: args.proxy_port,
        public_ip: RwLock::new(format!("{public_ip}:{}", args.udp_port)),
        local_ip: RwLock::new(local_ip),
        start_time: std::time::Instant::now(),
        total_bytes_in: AtomicU64::new(0),
        total_bytes_out: AtomicU64::new(0),
        logs: Mutex::new(VecDeque::new()),
        pool,
        jwt_secret: args.jwt_secret.clone(),
        tg_bot_token: args.tg_bot_token.clone(),
        tg_admin_chat_id: args.tg_admin_chat_id.clone(),
    });

    // ── TUN interface ─────────────────────────────────────────────────────────
    let mut tun_config = tun::Configuration::default();
    tun_config.address(VPN_SERVER_IP).netmask(VPN_NETMASK).destination(VPN_SUBNET).up();
    #[cfg(target_os = "linux")]
    tun_config.platform(|c| { c.packet_information(false); });

    let tun_dev = tun::create_as_async(&tun_config)
        .context("TUN creation failed — run as root / grant CAP_NET_ADMIN")?;
    info!("TUN up ({})", VPN_SERVER_IP);
    state.push_log(format!("TUN up — {VPN_SERVER_IP}/24"));

    setup_nat().context("iptables/ip_forward failed")?;

    // ── UDP socket ────────────────────────────────────────────────────────────
    let udp = Arc::new(
        UdpSocket::bind(format!("0.0.0.0:{}", args.udp_port)).await
            .context("UDP bind failed")?,
    );
    info!("UDP tunnel on :{}", args.udp_port);
    state.push_log(format!("UDP on :{}", args.udp_port));

    let (tun_rx, tun_tx) = tokio::io::split(tun_dev);
    let tun_tx = Arc::new(Mutex::new(tun_tx));

    {
        let (s, u) = (state.clone(), udp.clone());
        tokio::spawn(async move {
            if let Err(e) = tunnel::task_tun_to_udp(tun_rx, u, s).await { tracing::error!("{e}"); }
        });
    }
    {
        let (s, u, tw) = (state.clone(), udp.clone(), tun_tx.clone());
        tokio::spawn(async move {
            if let Err(e) = tunnel::task_udp_to_tun(u, tw, s).await { tracing::error!("{e}"); }
        });
    }

    // ── TCP proxy ─────────────────────────────────────────────────────────────
    {
        let s = state.clone();
        tokio::spawn(async move {
            if let Err(e) = proxy::run_proxy_server(s).await { tracing::error!("{e}"); }
        });
    }

    // ── HTTP API ──────────────────────────────────────────────────────────────
    let app = Router::new()
        // Auth
        .route("/auth/register",          post(user_api::register))
        .route("/auth/login",             post(user_api::login))
        .route("/auth/me",                get(user_api::me))
        // Subscription & promos (user)
        .route("/subscription/plans",     get(user_api::list_plans))
        .route("/subscription/buy",       post(user_api::buy_subscription))
        .route("/subscription/status",    get(user_api::subscription_status))
        .route("/promo/apply",            post(user_api::apply_promo))
        // VPN peers (require auth + active subscription)
        .route("/api/status",             get(api::api_status))
        .route("/api/peers",              get(api::api_list_peers))
        .route("/api/peers/register",     post(api::api_register))
        .route("/api/peers/:ip",          delete(api::api_remove_peer))
        .route("/api/peers/:ip/limit",    put(api::api_set_limit))
        // Admin
        .route("/admin/request-code",     post(admin_api::request_code))
        .route("/admin/verify-code",      post(admin_api::verify_code))
        .route("/admin/promos",           post(admin_api::create_promo))
        .route("/admin/users",            get(admin_api::list_users))
        .route("/admin/users/:id/limit",  put(admin_api::set_user_limit))
        .route("/admin/peers",            get(admin_api::list_peers))
        .layer(CorsLayer::permissive())
        .with_state(state.clone());

    let api_addr = format!("0.0.0.0:{}", args.api_port);
    info!("HTTP API on {api_addr}");
    state.push_log(format!("API on {api_addr}"));

    let listener = tokio::net::TcpListener::bind(&api_addr).await?;
    tokio::spawn(async move {
        if let Err(e) = axum::serve(listener, app).await { tracing::error!("{e}"); }
    });

    // ── Dashboard or plain wait ───────────────────────────────────────────────
    if use_tui && !args.no_tui {
        dashboard::run_dashboard(state).await?;
    } else {
        info!("Server running. Ctrl-C to stop. Logs → vpn-server.log");
        tokio::signal::ctrl_c().await?;
    }

    info!("Shutting down.");
    Ok(())
}

// ── NAT ───────────────────────────────────────────────────────────────────────

fn setup_nat() -> Result<()> {
    use std::process::Command;

    std::fs::write("/proc/sys/net/ipv4/ip_forward", "1")
        .context("Cannot enable IP forwarding")?;

    let _ = Command::new("iptables")
        .args(["-t","nat","-D","POSTROUTING","-s",vpn_common::VPN_SUBNET_CIDR,"!","-o","tun0","-j","MASQUERADE"])
        .output();
    Command::new("iptables")
        .args(["-t","nat","-A","POSTROUTING","-s",vpn_common::VPN_SUBNET_CIDR,"!","-o","tun0","-j","MASQUERADE"])
        .output().context("iptables MASQUERADE")?;

    for dir in ["-i", "-o"] {
        let _ = Command::new("iptables").args(["-D","FORWARD",dir,"tun0","-j","ACCEPT"]).output();
        Command::new("iptables").args(["-A","FORWARD",dir,"tun0","-j","ACCEPT"])
            .output().with_context(|| format!("FORWARD {dir} tun0"))?;
    }
    info!("NAT configured");
    Ok(())
}

// ── IP detection ──────────────────────────────────────────────────────────────

fn detect_local_ip() -> String {
    if let Ok(sock) = std::net::UdpSocket::bind("0.0.0.0:0") {
        let _ = sock.connect("1.1.1.1:80");
        if let Ok(addr) = sock.local_addr() {
            return addr.ip().to_string();
        }
    }
    "unknown".into()
}

async fn detect_public_ip() -> Option<String> {
    for url in &["https://api4.my-ip.io/ip", "https://api.ipify.org"] {
        if let Ok(resp) = reqwest::Client::new()
            .get(*url).timeout(Duration::from_secs(3)).send().await
        {
            if let Ok(t) = resp.text().await {
                let ip = t.trim().to_string();
                if !ip.is_empty() && ip.len() < 20 { return Some(ip); }
            }
        }
    }
    None
}
