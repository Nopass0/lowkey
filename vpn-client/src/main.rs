//! # Lowkey VPN Client
//!
//! ## Hysteria2 transport
//!
//! Use `--transport hysteria` to connect via QUIC instead of UDP/WebSocket:
//!
//! ```sh
//! vpn-client connect --server 1.2.3.4 --mode socks5 --transport hysteria
//! ```
//!
//! This is the most censorship-resistant option.  The server must have
//! `--hysteria-port` set (default 8443).  Certificate verification is
//! skipped by default; use `--tls-fingerprint` for production pinning.
//!
//! A cross-platform VPN client that supports two connection modes:
//!
//! | Mode | Platform | How it works |
//! |------|----------|-------------|
//! | `tun` | Linux / macOS | Creates a TUN device, routes all traffic through the encrypted UDP tunnel |
//! | `socks5` | All platforms | Runs a local SOCKS5 proxy at `127.0.0.1:<socks_port>` that tunnels through the server's TCP proxy |
//!
//! ## Quick start
//! ```sh
//! # Register an account
//! vpn-client auth register --server 1.2.3.4 --login alice --password secret
//!
//! # Buy a subscription (optional — top up balance first)
//! vpn-client subscription buy --plan standard
//!
//! # Connect (TUN mode, Linux/macOS, requires root)
//! sudo vpn-client connect --server 1.2.3.4
//!
//! # Connect (SOCKS5 mode, all platforms, no root)
//! vpn-client connect --server 1.2.3.4 --mode socks5
//! ```
//!
//! ## Session storage
//! After a successful login the JWT and server address are saved to
//! `~/.config/lowkey/session.json` so subsequent commands don't need
//! `--server` / `--api-port` arguments.

mod hysteria_client;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::signal;
#[cfg(unix)]
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::info;
use vpn_common::{from_hex, to_hex, VpnCrypto, DEFAULT_API_PORT};
use x25519_dalek::{PublicKey, StaticSecret};

#[cfg(unix)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ── Session storage ───────────────────────────────────────────────────────────

/// Server IP baked in at build time via `LOWKEY_SERVER_IP` env var.
/// Defaults to the production server if the env var is not set.
const BAKED_SERVER_IP: &str = match option_env!("LOWKEY_SERVER_IP") {
    Some(s) if !s.is_empty() => s,
    _ => "89.169.54.87",
};

/// Persisted client session — saved after login, loaded before each command.
#[derive(Debug, Serialize, Deserialize, Default, Clone)]
struct Session {
    /// JWT issued by the server (valid for 30 days).
    token: Option<String>,
    /// Server hostname or IP used during login.
    server: Option<String>,
    /// API port used during login.
    api_port: Option<u16>,
}

/// Return the path to the session file.
///
/// - Linux/macOS: `~/.config/lowkey/session.json`  (`$HOME`)
/// - Windows:     `%APPDATA%\lowkey\session.json`   (`$APPDATA`)
///
/// Falls back to the current directory if no suitable env var is found.
fn session_path() -> std::path::PathBuf {
    // Windows uses APPDATA (C:\Users\<name>\AppData\Roaming)
    #[cfg(windows)]
    {
        let base = std::env::var("APPDATA")
            .or_else(|_| std::env::var("USERPROFILE"))
            .unwrap_or_else(|_| ".".into());
        return std::path::PathBuf::from(base)
            .join("lowkey")
            .join("session.json");
    }

    // Unix: honour $HOME (or fall back to ".")
    #[allow(unreachable_code)]
    {
        let _home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
        std::path::PathBuf::from(_home)
            .join(".config")
            .join("lowkey")
            .join("session.json")
    }
}

/// Load the session from disk.  Returns a default (empty) session if the
/// file does not exist or cannot be parsed.
fn load_session() -> Session {
    std::fs::read_to_string(session_path())
        .ok()
        .and_then(|s| serde_json::from_str(s.trim_start_matches('\u{feff}')).ok())
        .unwrap_or_default()
}

/// Persist the session to disk, creating parent directories as needed.
fn save_session(session: &Session) -> Result<()> {
    let path = session_path();
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    std::fs::write(&path, serde_json::to_string_pretty(session)?)?;
    Ok(())
}

// ── CLI definition ────────────────────────────────────────────────────────────

/// VPN connection mode.
#[derive(Clone, ValueEnum, Debug, PartialEq)]
enum Mode {
    /// Create a TUN/WinTUN device and route ALL system traffic (all platforms).
    Tun,
    /// Start a local SOCKS5 proxy that tunnels through the server.
    Socks5,
}

/// Tunnel transport protocol.
#[derive(Clone, ValueEnum, Debug, PartialEq, Default)]
enum Transport {
    /// Encrypted UDP packets (fastest; may be blocked by some firewalls).
    #[default]
    Udp,
    /// Encrypted packets wrapped in WebSocket frames over TCP port 8080.
    /// Bypasses corporate firewalls and ISP blocking — looks like HTTP traffic.
    Ws,
    /// Hysteria2 QUIC transport — the most censorship-resistant option.
    ///
    /// Only compatible with `--mode socks5`.
    /// Requires the server to be started with `--hysteria-port` (default 8443).
    /// TLS certificate verification is skipped by default; for production use
    /// `--tls-fingerprint <SHA256>` (printed by the server on startup).
    Hysteria,
}

#[derive(Parser)]
#[command(name = "vpn-client", about = "Lowkey VPN Client")]
struct Args {
    #[command(subcommand)]
    command: Cmd,
}

/// Top-level subcommands.
#[derive(Subcommand)]
enum Cmd {
    /// Account registration, login and profile management.
    Auth {
        #[command(subcommand)]
        sub: AuthCmd,
    },
    /// Subscription purchase and status.
    Subscription {
        #[command(subcommand)]
        sub: SubCmd,
    },
    /// Apply a promo code to the account.
    Promo {
        /// Server IP/hostname (optional if saved in session).
        #[arg(short, long)]
        server: Option<String>,
        #[arg(long, default_value_t = DEFAULT_API_PORT)]
        api_port: u16,
        /// The promo code to apply.
        #[arg(short, long)]
        code: String,
    },
    /// Connect to the VPN.
    Connect {
        /// Server IP/hostname (optional if saved in session).
        #[arg(short, long)]
        server: Option<String>,
        #[arg(long, default_value_t = DEFAULT_API_PORT)]
        api_port: u16,
        /// Connection mode: `tun` (all platforms, requires admin) or `socks5`.
        #[arg(long, default_value = "tun")]
        mode: Mode,
        /// Transport protocol: `udp` (default, fast) or `ws` (WebSocket,
        /// bypasses firewalls — recommended on Windows/corporate networks).
        #[arg(long, default_value = "udp")]
        transport: Transport,
        /// Override the UDP tunnel port from the server's register response.
        #[arg(long)]
        udp_port: Option<u16>,
        /// Override the TCP proxy port from the server's register response.
        #[arg(long)]
        proxy_port: Option<u16>,
        /// Local SOCKS5 listen port (only relevant in `socks5` mode).
        #[arg(long, default_value_t = 1080)]
        socks_port: u16,
        /// Only route the VPN subnet (10.0.0.0/24) instead of all traffic.
        #[arg(long, default_value_t = false)]
        split_tunnel: bool,

        /// Hysteria2 QUIC server port (only used with `--transport hysteria`).
        #[arg(long, default_value_t = 8443)]
        hysteria_port: u16,

        /// Skip TLS certificate verification for the Hysteria2 connection.
        /// Convenient in development; in production use `--tls-fingerprint`.
        #[arg(long, default_value_t = true)]
        tls_skip_verify: bool,
    },
}

/// Auth subcommands.
#[derive(Subcommand)]
enum AuthCmd {
    /// Create a new account and save the session.
    Register {
        #[arg(short, long)]
        server: String,
        #[arg(long, default_value_t = DEFAULT_API_PORT)]
        api_port: u16,
        #[arg(short, long)]
        login: String,
        #[arg(short, long)]
        password: String,
    },
    /// Log in and save the session.
    Login {
        #[arg(short, long)]
        server: String,
        #[arg(long, default_value_t = DEFAULT_API_PORT)]
        api_port: u16,
        #[arg(short, long)]
        login: String,
        #[arg(short, long)]
        password: String,
    },
    /// Print the current user's profile.
    Me {
        #[arg(short, long)]
        server: Option<String>,
        #[arg(long, default_value_t = DEFAULT_API_PORT)]
        api_port: u16,
    },
    /// Clear the saved session (logout).
    Logout,
}

/// Subscription subcommands.
#[derive(Subcommand)]
enum SubCmd {
    /// List available subscription plans and prices.
    Plans {
        #[arg(short, long)]
        server: Option<String>,
        #[arg(long, default_value_t = DEFAULT_API_PORT)]
        api_port: u16,
    },
    /// Purchase a subscription plan (deducted from balance).
    Buy {
        #[arg(short, long)]
        server: Option<String>,
        #[arg(long, default_value_t = DEFAULT_API_PORT)]
        api_port: u16,
        /// Plan ID: `basic`, `standard` or `premium`.
        #[arg(long, default_value = "standard")]
        plan: String,
    },
    /// Show current subscription status and expiry.
    Status {
        #[arg(short, long)]
        server: Option<String>,
        #[arg(long, default_value_t = DEFAULT_API_PORT)]
        api_port: u16,
    },
}

// ── Main ──────────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("vpn_client=info".parse()?),
        )
        .init();

    match Args::parse().command {
        Cmd::Auth { sub } => handle_auth(sub).await?,
        Cmd::Subscription { sub } => handle_sub(sub).await?,
        Cmd::Promo {
            server,
            api_port,
            code,
        } => {
            let session = load_session();
            let srv = server
                .or(session.server.clone())
                .context("--server required")?;
            let tok = session.token.context("Not logged in")?;
            let resp = api_post(
                &srv,
                api_port,
                "/promo/apply",
                &tok,
                &serde_json::json!({ "code": code }),
            )
            .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Cmd::Connect {
            server,
            api_port,
            mode,
            transport,
            udp_port,
            proxy_port,
            socks_port,
            split_tunnel,
            hysteria_port,
            tls_skip_verify,
        } => {
            let session = load_session();
            let srv = server
                .or(session.server.clone())
                .context("--server required")?;

            // ── Hysteria2 transport: QUIC-based SOCKS5 proxy ─────────────────
            if transport == Transport::Hysteria {
                // The JWT is used as the Hysteria2 password, ensuring that only
                // users with valid accounts can authenticate.
                let tok = session
                    .token
                    .context("Not logged in. Run: vpn-client auth login")?;
                hysteria_client::run_hysteria_socks5(
                    &srv,
                    hysteria_port,
                    &tok,
                    socks_port,
                    tls_skip_verify,
                )
                .await?;
                return Ok(());
            }

            let tok = session
                .token
                .context("Not logged in. Run: vpn-client auth login")?;
            connect(
                &srv,
                api_port,
                udp_port,
                proxy_port,
                &tok,
                mode,
                transport,
                socks_port,
                split_tunnel,
            )
            .await?;
        }
    }
    Ok(())
}

// ── Auth command handlers ─────────────────────────────────────────────────────

/// Handle all `auth` subcommands.
async fn handle_auth(cmd: AuthCmd) -> Result<()> {
    match cmd {
        AuthCmd::Register {
            server,
            api_port,
            login,
            password,
        } => {
            // Register and automatically save the returned token
            let resp = api_anon(
                &server,
                api_port,
                "/auth/register",
                &serde_json::json!({ "login": login, "password": password }),
            )
            .await?;
            let tok = resp["token"].as_str().unwrap_or("").to_string();
            println!(
                "Registered as '{}'\n{}",
                login,
                serde_json::to_string_pretty(&resp["user"])?
            );
            save_session(&Session {
                token: Some(tok),
                server: Some(server),
                api_port: Some(api_port),
            })?;
        }
        AuthCmd::Login {
            server,
            api_port,
            login,
            password,
        } => {
            // Login and save the returned token to the session file
            let resp = api_anon(
                &server,
                api_port,
                "/auth/login",
                &serde_json::json!({ "login": login, "password": password }),
            )
            .await?;
            let tok = resp["token"].as_str().context("No token")?.to_string();
            println!(
                "Logged in as '{}'\n{}",
                login,
                serde_json::to_string_pretty(&resp["user"])?
            );
            save_session(&Session {
                token: Some(tok),
                server: Some(server),
                api_port: Some(api_port),
            })?;
        }
        AuthCmd::Me { server, api_port } => {
            let s = load_session();
            let srv = server.or(s.server).context("--server required")?;
            let tok = s.token.context("Not logged in")?;
            println!(
                "{}",
                serde_json::to_string_pretty(&api_get(&srv, api_port, "/auth/me", &tok).await?)?
            );
        }
        AuthCmd::Logout => {
            // Overwrite the session with an empty one
            save_session(&Session::default())?;
            println!("Logged out.");
        }
    }
    Ok(())
}

// ── Subscription command handlers ─────────────────────────────────────────────

/// Handle all `subscription` subcommands.
async fn handle_sub(cmd: SubCmd) -> Result<()> {
    match cmd {
        SubCmd::Plans { server, api_port } => {
            let s = load_session();
            let srv = server
                .or(s.server)
                .unwrap_or_else(|| BAKED_SERVER_IP.to_string());
            // Plans endpoint is public — no token needed
            let resp: serde_json::Value = api_http_client()?
                .get(format!("http://{}:{}/subscription/plans", srv, api_port))
                .send()
                .await?
                .json()
                .await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        SubCmd::Buy {
            server,
            api_port,
            plan,
        } => {
            let s = load_session();
            let srv = server
                .or(s.server.clone())
                .unwrap_or_else(|| BAKED_SERVER_IP.to_string());
            let tok = s.token.context("Not logged in")?;
            let resp = api_post(
                &srv,
                api_port,
                "/subscription/buy",
                &tok,
                &serde_json::json!({ "plan_id": plan }),
            )
            .await?;
            println!(
                "Subscription activated!\n{}",
                serde_json::to_string_pretty(&resp)?
            );
        }
        SubCmd::Status { server, api_port } => {
            let s = load_session();
            let srv = server
                .or(s.server)
                .unwrap_or_else(|| BAKED_SERVER_IP.to_string());
            let tok = s.token.context("Not logged in")?;
            println!(
                "{}",
                serde_json::to_string_pretty(
                    &api_get(&srv, api_port, "/subscription/status", &tok).await?
                )?
            );
        }
    }
    Ok(())
}

// ── VPN connect ───────────────────────────────────────────────────────────────

/// Register a peer with the server and start the VPN in the requested mode.
///
/// For `--transport ws` mode, peer registration is handled inside the
/// WebSocket handshake (no separate REST call is needed).
/// For `--transport udp` mode, the classic REST + UDP path is used.
async fn connect(
    server: &str,
    api_port: u16,
    udp_override: Option<u16>,
    proxy_override: Option<u16>,
    token: &str,
    mode: Mode,
    transport: Transport,
    socks_port: u16,
    split: bool,
) -> Result<()> {
    // WebSocket transport: the WS handshake handles peer registration inline.
    if transport == Transport::Ws {
        let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
        match mode {
            Mode::Tun => {
                run_ws_tun_mode(server, api_port, token, Arc::new(secret), split).await?;
            }
            Mode::Socks5 => {
                // Even in SOCKS5 mode, the user explicitly asked for WS transport.
                // We still use the TCP proxy here; the transport flag only affects TUN.
                let proxy_port = proxy_override.unwrap_or(vpn_common::DEFAULT_PROXY_PORT);
                run_socks5_mode(server, proxy_port, socks_port, &secret).await?;
            }
        }
        return Ok(());
    }

    // ── Classic UDP transport ─────────────────────────────────────────────────
    #[cfg(not(unix))]
    let _ = split;

    let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let public = PublicKey::from(&secret);

    // Register with the server: sends our public key, gets VPN IP + server pubkey
    let reg = api_post(
        server,
        api_port,
        "/api/peers/register",
        token,
        &serde_json::json!({
            "public_key": to_hex(public.as_bytes()),
            "psk": ""  // empty = skip legacy PSK check
        }),
    )
    .await
    .context("Registration failed — check subscription")?;

    let vpn_ip: std::net::Ipv4Addr = reg["assigned_ip"]
        .as_str()
        .context("No assigned_ip")?
        .parse()?;

    #[cfg(unix)]
    let udp_port = udp_override
        .or_else(|| reg["udp_port"].as_u64().map(|p| p as u16))
        .unwrap_or(51820);
    #[cfg(not(unix))]
    let _udp_port = udp_override
        .or_else(|| reg["udp_port"].as_u64().map(|p| p as u16))
        .unwrap_or(51820);
    let proxy_port = proxy_override
        .or_else(|| reg["proxy_port"].as_u64().map(|p| p as u16))
        .unwrap_or(8388);

    let spub: Vec<u8> = from_hex(reg["server_public_key"].as_str().unwrap_or(""))
        .filter(|b| b.len() == 32)
        .context("Bad server pubkey")?;
    let mut spub_arr = [0u8; 32];
    spub_arr.copy_from_slice(&spub);
    let shared = secret.diffie_hellman(&PublicKey::from(spub_arr));
    #[cfg(unix)]
    let crypto = Arc::new(VpnCrypto::from_shared_secret(&shared));
    #[cfg(not(unix))]
    let _crypto = Arc::new(VpnCrypto::from_shared_secret(&shared));

    info!("VPN IP: {}  mode: {:?}  transport: udp", vpn_ip, mode);

    match mode {
        Mode::Tun => {
            #[cfg(unix)]
            run_tun_mode(server, vpn_ip, udp_port, crypto, split).await?;
            #[cfg(not(unix))]
            {
                tracing::warn!(
                    "UDP transport with TUN requires Linux/macOS. \
                     Use --transport ws for Windows TUN mode."
                );
                run_socks5_mode(server, proxy_port, socks_port, &secret).await?;
            }
        }
        Mode::Socks5 => run_socks5_mode(server, proxy_port, socks_port, &secret).await?,
    }
    Ok(())
}

// ── WebSocket TUN mode (all platforms) ───────────────────────────────────────

/// Connect via WebSocket transport with a system-level TUN/WinTUN adapter.
///
/// Recommended for Windows and for any network that blocks raw UDP.
/// The VPN tunnel travels as binary WebSocket frames over TCP port 8080,
/// which looks identical to regular HTTP traffic to firewalls and ISPs.
///
/// Architecture:
/// ```text
///   TUN/WinTUN ──► [tun→ws channel] ──► WS encrypt task ──► WebSocket
///   TUN/WinTUN ◄── [ws→tun channel] ◄── WS decrypt task ◄── WebSocket
/// ```
#[allow(unused_variables)]
async fn run_ws_tun_mode(
    server: &str,
    api_port: u16,
    token: &str,
    secret: Arc<StaticSecret>,
    split: bool,
) -> Result<()> {
    let url = format!("ws://{}:{}/ws-tunnel?token={}", server, api_port, token);
    info!("WS tunnel → {}", url);

    let (ws, _) = connect_async(url.as_str())
        .await
        .context("WebSocket connection failed — check server address and network")?;
    // Explicit type to help the compiler resolve the SinkExt/StreamExt impls
    let ws: tokio_tungstenite::WebSocketStream<
        tokio_tungstenite::MaybeTlsStream<tokio::net::TcpStream>,
    > = ws;
    let (mut ws_sink, mut ws_stream) = ws.split();

    // ── Handshake ─────────────────────────────────────────────────────────────
    // Frame 1 (client→server): 32-byte X25519 ephemeral public key
    let public = PublicKey::from(secret.as_ref());
    ws_sink
        .send(Message::Binary(public.as_bytes().to_vec().into()))
        .await
        .context("WS handshake send failed")?;

    // Frame 2 (server→client): 32-byte server pubkey + 4-byte assigned VPN IP
    let resp = loop {
        match ws_stream.next().await {
            Some(Ok(Message::Binary(b))) if b.len() == 36 => break b,
            Some(Ok(Message::Ping(d))) => {
                let _ = ws_sink.send(Message::Pong(d)).await;
            }
            other => anyhow::bail!("Unexpected WS handshake frame: {:?}", other),
        }
    };
    let mut spub_arr = [0u8; 32];
    spub_arr.copy_from_slice(&resp[..32]);
    let vpn_ip = std::net::Ipv4Addr::new(resp[32], resp[33], resp[34], resp[35]);
    let shared = secret.diffie_hellman(&PublicKey::from(spub_arr));
    let crypto = Arc::new(VpnCrypto::from_shared_secret(&shared));
    info!("WS handshake OK — VPN IP: {}", vpn_ip);

    // ── Keepalive ─────────────────────────────────────────────────────────────
    ws_sink
        .send(Message::Binary(crypto.encrypt(b"hello").into()))
        .await
        .context("WS keepalive send failed")?;

    // ── Bridge channels ───────────────────────────────────────────────────────
    // tun_to_ws_tx  — plaintext IP packets read from TUN → encrypt → WS send task
    // ws_to_tun_tx  — plaintext IP packets decoded from WS → write to TUN
    let (tun_to_ws_tx, mut tun_to_ws_rx) =
        tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();
    let (ws_to_tun_tx, ws_to_tun_rx) =
        tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

    // WS send task: encrypt packets from TUN and push them as binary frames
    let c_enc = crypto.clone();
    tokio::spawn(async move {
        while let Some(plain) = tun_to_ws_rx.recv().await {
            let enc = c_enc.encrypt(&plain);
            if ws_sink.send(Message::Binary(enc.into())).await.is_err() {
                break;
            }
        }
    });

    // WS recv task: receive binary frames, decrypt, push plaintext to TUN
    let c_dec = crypto.clone();
    tokio::spawn(async move {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Binary(data)) => {
                    if let Some(plain) = c_dec.decrypt(&data) {
                        if plain != b"hello" {
                            let _ = ws_to_tun_tx.send(plain);
                        }
                    }
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
                _ => break,
            }
        }
    });

    // ── Platform-specific TUN forwarding ──────────────────────────────────────
    #[cfg(unix)]
    ws_tun_unix(server, vpn_ip, split, tun_to_ws_tx, ws_to_tun_rx).await?;

    #[cfg(windows)]
    {
        let _ = split; // used only on unix via ws_tun_unix
        ws_tun_windows(server, vpn_ip, tun_to_ws_tx, ws_to_tun_rx).await?;
    }

    #[cfg(not(any(unix, windows)))]
    {
        let _ = (split, tun_to_ws_tx, ws_to_tun_rx);
        anyhow::bail!("TUN mode is not supported on this platform");
    }

    Ok(())
}

// ── Unix WS-TUN helper ────────────────────────────────────────────────────────

#[cfg(unix)]
async fn ws_tun_unix(
    server: &str,
    vpn_ip: std::net::Ipv4Addr,
    split: bool,
    tun_to_ws: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    mut ws_to_tun: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut cfg = tun::Configuration::default();
    cfg.address(vpn_ip.to_string().as_str())
        .netmask(vpn_common::VPN_NETMASK)
        .destination(vpn_common::VPN_SERVER_IP)
        .up();
    cfg.platform(|c| { c.packet_information(false); });
    let dev = tun::create_as_async(&cfg).context("TUN failed — run as root")?;
    info!("TUN up ({})", vpn_ip);

    let orig_gw = get_gw()?;
    setup_routing(server, &orig_gw, split)?;
    info!("WS-TUN routing active. Ctrl-C to disconnect.");

    let (mut tun_r, tun_w) = tokio::io::split(dev);
    let tun_w = Arc::new(tokio::sync::Mutex::new(tun_w));

    // TUN read → WS send channel
    tokio::spawn(async move {
        let mut buf = vec![0u8; 65536];
        loop {
            let n = match tun_r.read(&mut buf).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            if tun_to_ws.send(buf[..n].to_vec()).is_err() {
                break;
            }
        }
    });

    // WS recv channel → TUN write
    let tw = tun_w.clone();
    tokio::spawn(async move {
        while let Some(plain) = ws_to_tun.recv().await {
            let _ = tw.lock().await.write_all(&plain).await;
        }
    });

    signal::ctrl_c().await?;
    restore_routing(server, &orig_gw, split);
    Ok(())
}

// ── Windows WS-TUN helper (WinTUN) ───────────────────────────────────────────

/// WS-TUN relay for Windows using the WinTUN kernel driver.
///
/// **Requirements**: `wintun.dll` must be present next to `vpn-client.exe`,
/// OR WireGuard for Windows must be installed (it installs wintun globally).
/// Download wintun.dll from <https://www.wintun.net/> if needed.
///
/// Must be run as Administrator.
#[cfg(windows)]
async fn ws_tun_windows(
    server: &str,
    vpn_ip: std::net::Ipv4Addr,
    tun_to_ws: tokio::sync::mpsc::UnboundedSender<Vec<u8>>,
    mut ws_to_tun: tokio::sync::mpsc::UnboundedReceiver<Vec<u8>>,
) -> Result<()> {
    use std::sync::atomic::{AtomicBool, Ordering};

    // Load WinTUN driver
    let wintun = unsafe {
        wintun::load().context(
            "wintun.dll not found.\n\
             Options:\n\
             1. Copy wintun.dll next to vpn-client.exe\n\
             2. Install WireGuard for Windows (includes wintun)\n\
             Download: https://www.wintun.net/",
        )?
    };

    // Open or create the virtual adapter
    let adapter = match wintun::Adapter::open(&wintun, "Lowkey") {
        Ok(a) => { info!("Reusing WinTUN adapter 'Lowkey'"); a }
        Err(_) => {
            info!("Creating WinTUN adapter 'Lowkey'");
            wintun::Adapter::create(&wintun, "Lowkey", "WireGuard", None)
                .context("Create WinTUN adapter failed — run as Administrator")?
        }
    };

    let session = Arc::new(
        adapter.start_session(wintun::MAX_RING_CAPACITY)
            .context("WinTUN start_session failed")?,
    );

    // Assign IP address
    configure_win_adapter_ip(&vpn_ip.to_string())?;
    info!("WinTUN adapter up ({})", vpn_ip);

    // Configure routing
    let orig_gw = get_windows_gateway()?;
    setup_windows_routing(server, &orig_gw)?;
    info!("Windows routing active. Ctrl-C to disconnect.");

    let running = Arc::new(AtomicBool::new(true));

    // WinTUN read thread → tun_to_ws channel (blocking I/O in std thread)
    {
        let sess = session.clone();
        let tx = tun_to_ws.clone();
        let r = running.clone();
        std::thread::spawn(move || {
            while r.load(Ordering::Relaxed) {
                match sess.receive_blocking() {
                    Ok(pkt) => {
                        let bytes = pkt.bytes().to_vec();
                        drop(pkt);
                        if tx.send(bytes).is_err() { break; }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    // ws_to_tun channel → WinTUN write thread (blocking I/O in std thread)
    {
        let sess = session.clone();
        let r = running.clone();
        // Bridge async receiver to a std::sync::mpsc for the blocking thread
        let (std_tx, std_rx) = std::sync::mpsc::sync_channel::<Vec<u8>>(256);

        // Async forwarder: ws_to_tun → std_tx
        tokio::spawn(async move {
            while let Some(plain) = ws_to_tun.recv().await {
                if std_tx.send(plain).is_err() { break; }
            }
        });

        // Blocking writer: std_rx → WinTUN
        std::thread::spawn(move || {
            while r.load(Ordering::Relaxed) {
                match std_rx.recv() {
                    Ok(plain) => {
                        if let Ok(mut pkt) = sess.allocate_send_packet(plain.len() as u16) {
                            pkt.bytes_mut().copy_from_slice(&plain);
                            sess.send_packet(pkt);
                        }
                    }
                    Err(_) => break,
                }
            }
        });
    }

    signal::ctrl_c().await?;
    running.store(false, std::sync::atomic::Ordering::Relaxed);
    restore_windows_routing(server, &orig_gw);
    println!("WinTUN disconnected.");
    Ok(())
}

// ── Windows helpers ───────────────────────────────────────────────────────────

#[cfg(windows)]
/// Assign a static IP address to the "Lowkey" WinTUN adapter using `netsh`.
///
/// Also configures DNS servers to prevent DNS leaks: sends all DNS queries
/// through `8.8.8.8` / `8.8.4.4` which will route through the VPN tunnel.
///
/// Uses PowerShell `Set-DnsClientServerAddress` to reliably set DNS on the
/// adapter (the `netsh dns` command can be unreliable on newer Windows).
#[cfg(windows)]
fn configure_win_adapter_ip(ip: &str) -> Result<()> {
    use std::process::Command;

    // Set static IPv4 address, netmask and gateway on the WinTUN adapter
    let status = Command::new("netsh")
        .args([
            "interface", "ip", "set", "address",
            "name=Lowkey",          // adapter name (matches wintun::Adapter::create)
            "source=static",
            &format!("address={ip}"),
            &format!("mask={}", vpn_common::VPN_NETMASK),
            &format!("gateway={}", vpn_common::VPN_SERVER_IP),
            "gwmetric=1",
        ])
        .status()
        .context("netsh: set address failed")?;

    if !status.success() {
        // netsh sometimes returns non-zero even on success; log a warning
        // but don't bail — the adapter may still be usable.
        tracing::warn!("netsh set-address returned non-zero (may be harmless)");
    }

    // Set DNS servers on the Lowkey adapter to prevent DNS leaks.
    // Google's public DNS (8.8.8.8 / 8.8.4.4) will route through the VPN.
    let dns_cmd = Command::new("powershell")
        .args([
            "-NoProfile", "-NonInteractive", "-Command",
            "Set-DnsClientServerAddress -InterfaceAlias 'Lowkey' \
             -ServerAddresses ('8.8.8.8','8.8.4.4')",
        ])
        .status();
    if let Err(e) = dns_cmd {
        tracing::warn!("DNS config failed (DNS leak possible): {e}");
    }

    Ok(())
}

/// Detect the current default IPv4 gateway using PowerShell `Get-NetRoute`.
///
/// Returns the gateway IP as a string (e.g. `"192.168.1.1"`).
/// Fails if no default route exists (no internet connectivity).
#[cfg(windows)]
fn get_windows_gateway() -> Result<String> {
    use std::process::Command;
    let out = Command::new("powershell")
        .args([
            "-NoProfile", "-NonInteractive", "-Command",
            // Filter out the WinTUN adapter's routes (InterfaceAlias != 'Lowkey')
            // Sort by RouteMetric to prefer the primary gateway
            "(Get-NetRoute -DestinationPrefix '0.0.0.0/0' | \
              Where-Object { $_.InterfaceAlias -ne 'Lowkey' } | \
              Sort-Object -Property { $_.RouteMetric + $_.InterfaceMetric } | \
              Select-Object -First 1).NextHop",
        ])
        .output()
        .context("powershell: could not detect default gateway")?;
    let gw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if gw.is_empty() || gw == "0.0.0.0" {
        anyhow::bail!(
            "No default gateway detected. Check your network connection."
        );
    }
    Ok(gw)
}

/// Set up Windows routing for full-tunnel VPN mode.
///
/// Routing table after this function:
///
/// ```text
/// 0.0.0.0/1     via 10.66.0.1  (VPN)  ← all internet traffic
/// 128.0.0.0/1   via 10.66.0.1  (VPN)
/// <server>/32   via <orig_gw>   (WAN)  ← VPN server traffic bypasses tunnel
/// 10.66.0.0/24  on  Lowkey             ← VPN subnet local
/// ```
///
/// Splitting the default route into two /1 routes is the standard trick to
/// override the kernel's 0.0.0.0/0 default without the route tool's
/// "destination == gateway" check interfering.
#[cfg(windows)]
fn setup_windows_routing(server: &str, orig_gw: &str) -> Result<()> {
    use std::process::Command;

    // 1. Route VPN server traffic through the original gateway (prevent loop)
    let _ = Command::new("route").args(["delete", server]).output();
    let status = Command::new("route")
        .args(["add", server, "mask", "255.255.255.255", orig_gw, "metric", "5"])
        .status()
        .context("route add: server host route")?;
    if !status.success() {
        anyhow::bail!("Failed to add server host route via {orig_gw}");
    }

    // 2. Route all other traffic through the VPN tunnel (two /1 blocks)
    for (net, mask) in [("0.0.0.0", "128.0.0.0"), ("128.0.0.0", "128.0.0.0")] {
        let _ = Command::new("route")
            .args(["delete", net, "mask", mask])
            .output();
        let status = Command::new("route")
            .args([
                "add", net, "mask", mask,
                vpn_common::VPN_SERVER_IP,
                "metric", "6",
            ])
            .status()
            .with_context(|| format!("route add {net}/{mask} via VPN"))?;
        if !status.success() {
            anyhow::bail!("Failed to add default route {net}/{mask} via VPN gateway");
        }
    }

    info!("Windows routing: all traffic → VPN (server {server} → WAN)");
    Ok(())
}

/// Remove the VPN-specific routes added by [`setup_windows_routing`].
///
/// Called on Ctrl-C to restore the system to its pre-VPN routing state.
#[cfg(windows)]
fn restore_windows_routing(server: &str, _orig_gw: &str) {
    use std::process::Command;

    // Remove the server host route (WAN bypass)
    let _ = Command::new("route").args(["delete", server]).output();

    // Remove the two /1 default routes pointing at the VPN gateway
    for (net, mask) in [("0.0.0.0", "128.0.0.0"), ("128.0.0.0", "128.0.0.0")] {
        let _ = Command::new("route")
            .args(["delete", net, "mask", mask])
            .output();
    }

    // Restore DNS to automatic (DHCP) to remove the VPN DNS settings
    let _ = Command::new("powershell")
        .args([
            "-NoProfile", "-NonInteractive", "-Command",
            "Set-DnsClientServerAddress -InterfaceAlias 'Lowkey' -ResetServerAddresses",
        ])
        .output();

    info!("Windows routing restored.");
}

// ── TUN mode (Unix only) ──────────────────────────────────────────────────────

/// Start TUN-based VPN mode.
///
/// Creates a TUN device (`tun0`), assigns the given VPN IP, sets up routing
/// so all traffic flows through the encrypted UDP tunnel, and runs two async
/// tasks for bidirectional forwarding.
///
/// Pressing Ctrl-C restores the original routing table.
#[cfg(unix)]
async fn run_tun_mode(
    server: &str,
    vpn_ip: std::net::Ipv4Addr,
    udp_port: u16,
    crypto: Arc<VpnCrypto>,
    split: bool,
) -> Result<()> {
    use tokio::net::UdpSocket;

    // Create the TUN device
    let mut cfg = tun::Configuration::default();
    cfg.address(vpn_ip.to_string().as_str())
        .netmask(vpn_common::VPN_NETMASK)
        .destination(vpn_common::VPN_SERVER_IP)
        .up();
    cfg.platform(|c| {
        c.packet_information(false);
    });
    let dev = tun::create_as_async(&cfg).context("TUN failed — run as root")?;
    info!("TUN up ({})", vpn_ip);

    // Save the current default gateway before altering routes
    let orig_gw = get_gw()?;
    setup_routing(server, &orig_gw, split)?;
    info!("Routing active. Ctrl-C to disconnect.");

    // Bind a local UDP socket for the tunnel
    let udp = Arc::new(UdpSocket::bind("0.0.0.0:0").await?);
    let srv: std::net::SocketAddr = format!("{server}:{udp_port}").parse()?;
    let ib = vpn_ip.octets();

    // Send a keepalive packet to establish the server-side endpoint mapping
    {
        let enc = crypto.encrypt(b"hello");
        let mut p = Vec::new();
        p.extend_from_slice(&ib); // 4-byte VPN IP prefix
        p.extend_from_slice(&enc);
        udp.send_to(&p, srv).await?;
    }

    // Split TUN into read/write halves; protect write half with a Mutex
    let (rx, tx) = tokio::io::split(dev);
    let tx = Arc::new(Mutex::new(tx));

    // Spawn forwarding tasks
    {
        let (u, c) = (udp.clone(), crypto.clone());
        tokio::spawn(async move {
            let _ = tun_to_udp(rx, u, c, ib, srv).await;
        });
    }
    {
        let (u, c, tw) = (udp.clone(), crypto.clone(), tx.clone());
        tokio::spawn(async move {
            let _ = udp_to_tun(u, tw, c).await;
        });
    }

    // Wait for Ctrl-C, then restore routing
    signal::ctrl_c().await?;
    restore_routing(server, &orig_gw, split);
    Ok(())
}

/// Forward outgoing IP packets from TUN to the encrypted UDP tunnel.
///
/// Reads plaintext packets from the TUN device, prepends the 4-byte VPN IP
/// routing header, encrypts with ChaCha20-Poly1305, and sends to the server.
#[cfg(unix)]
async fn tun_to_udp(
    mut tun: impl AsyncReadExt + Unpin,
    udp: Arc<tokio::net::UdpSocket>,
    crypto: Arc<VpnCrypto>,
    ib: [u8; 4], // client VPN IP bytes (routing header)
    srv: std::net::SocketAddr,
) -> Result<()> {
    let mut buf = vec![0u8; 65536];
    loop {
        let n = tun.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let enc = crypto.encrypt(&buf[..n]);
        // Prepend the 4-byte VPN IP prefix so the server can look up the peer
        let mut p = Vec::new();
        p.extend_from_slice(&ib);
        p.extend_from_slice(&enc);
        let _ = udp.send_to(&p, srv).await;
    }
    Ok(())
}

/// Forward incoming decrypted UDP packets to the TUN device.
///
/// Receives from the server UDP socket, decrypts with ChaCha20-Poly1305,
/// and writes the plaintext IP packet to the TUN device so the kernel
/// delivers it to local applications.
#[cfg(unix)]
async fn udp_to_tun(
    udp: Arc<tokio::net::UdpSocket>,
    tun: Arc<Mutex<impl AsyncWriteExt + Unpin>>,
    crypto: Arc<VpnCrypto>,
) -> Result<()> {
    let mut buf = vec![0u8; 65600];
    loop {
        let (n, _) = udp.recv_from(&mut buf).await?;
        if let Some(plain) = crypto.decrypt(&buf[..n]) {
            let _ = tun.lock().await.write_all(&plain).await;
        }
    }
}

// ── SOCKS5 mode ───────────────────────────────────────────────────────────────

/// Start a local SOCKS5 proxy that routes traffic through the server's TCP proxy.
///
/// Binds `127.0.0.1:<socks_port>` and handles each CONNECT request by:
/// 1. Performing an X25519 handshake with the server's TCP proxy.
/// 2. Sending an encrypted connect-header with the target address.
/// 3. Relaying plaintext data between the SOCKS5 client and the encrypted
///    proxy stream.
///
/// # Usage
/// Set your system proxy to `SOCKS5 127.0.0.1:1080` (or the configured port)
/// and all TCP traffic will be tunnelled.

/// Check whether public IP changes when using a local SOCKS5 proxy.
///
/// Returns `(direct_ip, proxy_ip)` on success.
async fn verify_proxy_ip_change(socks_port: u16) -> Result<(String, String)> {
    let direct_client = api_http_client()?;
    let direct_ip = direct_client
        .get("https://api.ipify.org")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?
        .trim()
        .to_string();

    let proxy_url = format!("socks5h://127.0.0.1:{socks_port}");
    let proxied_client = reqwest::Client::builder()
        .proxy(reqwest::Proxy::all(&proxy_url)?)
        .build()?;
    let proxy_ip = proxied_client
        .get("https://api.ipify.org")
        .send()
        .await?
        .error_for_status()?
        .text()
        .await?
        .trim()
        .to_string();

    Ok((direct_ip, proxy_ip))
}

async fn run_socks5_mode(
    server: &str,
    proxy_port: u16,
    socks_port: u16,
    my_secret: &StaticSecret,
) -> Result<()> {
    use tokio::net::TcpListener;

    let listener = TcpListener::bind(format!("127.0.0.1:{socks_port}"))
        .await
        .with_context(|| format!("Cannot bind :{socks_port}"))?;

    println!(
        "SOCKS5 proxy on 127.0.0.1:{socks_port}\n\
         Set system proxy → SOCKS5 127.0.0.1:{socks_port}\n\
         Ctrl-C to disconnect."
    );

    let sb = my_secret.to_bytes();
    let sa = format!("{server}:{proxy_port}");
    tokio::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        match tokio::time::timeout(
            std::time::Duration::from_secs(12),
            verify_proxy_ip_change(socks_port),
        )
        .await
        {
            Ok(Ok((direct_ip, proxy_ip))) => {
                if direct_ip == proxy_ip {
                    tracing::warn!(
                        "Proxy IP check: public IP did not change ({}). Proxy may be bypassed.",
                        proxy_ip
                    );
                } else {
                    tracing::info!(
                        "Proxy IP check: direct={} proxied={} (VPN egress active)",
                        direct_ip,
                        proxy_ip
                    );
                }
            }
            Ok(Err(err)) => {
                tracing::warn!("Proxy IP check failed: {err}");
            }
            Err(_) => {
                tracing::warn!("Proxy IP check timed out");
            }
        }
    });

    let ctrl_c = signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        tokio::select! {
            _ = &mut ctrl_c => break,
            res = listener.accept() => {
                let (stream, _) = res?;
                let sa = sa.clone();
                let sb = sb;
                tokio::spawn(async move {
                    if let Err(e) = socks5_conn(stream, &sa, &sb).await {
                        tracing::trace!("socks5: {e}");
                    }
                });
            }
        }
    }

    println!("Disconnected.");
    Ok(())
}

/// Handle one SOCKS5 CONNECT request.
///
/// Implements a minimal SOCKS5 server (RFC 1928) supporting only the
/// `CONNECT` command with IPv4, hostname and IPv6 address types.
///
/// Once the SOCKS5 negotiation completes, establishes an encrypted connection
/// to the server's TCP proxy, forwards the target address in the encrypted
/// connect-header, and then relays data bidirectionally using owned TCP
/// stream halves.
async fn socks5_conn(
    mut cl: tokio::net::TcpStream,
    vpn_addr: &str, // server TCP proxy address (ip:port)
    sb: &[u8; 32],  // our X25519 secret key bytes
) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buf = [0u8; 512];

    // ── SOCKS5 greeting ───────────────────────────────────────────────────────
    // Client sends: [VER=5] [NMETHODS] [methods...]
    cl.read_exact(&mut buf[..2]).await?;
    if buf[0] != 5 {
        anyhow::bail!("Not SOCKS5");
    }
    let nm = buf[1] as usize;
    cl.read_exact(&mut buf[..nm]).await?;
    // We only support NO-AUTH (method 0x00)
    cl.write_all(&[5, 0]).await?;

    // ── SOCKS5 request ────────────────────────────────────────────────────────
    // Client sends: [VER=5] [CMD] [RSV=0] [ATYP] [addr] [port]
    cl.read_exact(&mut buf[..4]).await?;
    if buf[1] != 1 {
        // We only support CONNECT (CMD=1)
        cl.write_all(&[5, 7, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        anyhow::bail!("Only CONNECT supported");
    }

    // Parse the target address depending on the address type byte (ATYP)
    let (addr_bytes, port): (Vec<u8>, u16) = match buf[3] {
        // IPv4 — 4 bytes + 2 bytes port
        1 => {
            cl.read_exact(&mut buf[..6]).await?;
            let mut a = vec![1];
            a.extend_from_slice(&buf[..4]);
            (a, u16::from_be_bytes([buf[4], buf[5]]))
        }
        // Hostname — 1 byte length + N bytes + 2 bytes port
        3 => {
            cl.read_exact(&mut buf[..1]).await?;
            let hl = buf[0] as usize;
            cl.read_exact(&mut buf[..hl + 2]).await?;
            let mut a = vec![3, hl as u8];
            a.extend_from_slice(&buf[..hl]);
            (a, u16::from_be_bytes([buf[hl], buf[hl + 1]]))
        }
        // IPv6 — 16 bytes + 2 bytes port
        4 => {
            cl.read_exact(&mut buf[..18]).await?;
            let mut a = vec![4];
            a.extend_from_slice(&buf[..16]);
            (a, u16::from_be_bytes([buf[16], buf[17]]))
        }
        _ => anyhow::bail!("Unknown address type"),
    };

    // ── TCP proxy handshake ───────────────────────────────────────────────────
    // Derive X25519 keypair, connect to VPN proxy, exchange pubkeys
    let my_secret = StaticSecret::from(*sb);
    let my_pub = PublicKey::from(&my_secret);
    let mut vs = tokio::net::TcpStream::connect(vpn_addr)
        .await
        .context("VPN proxy connect failed")?;
    vs.write_all(my_pub.as_bytes()).await?; // send our ephemeral pubkey
    let mut spb = [0u8; 32];
    vs.read_exact(&mut spb).await?; // receive server pubkey
    let fc = vpn_common::FramedCrypto::new(&my_secret, &PublicKey::from(spb));

    // Build and send the encrypted connect-header
    // JWT-authenticated clients intentionally use an empty PSK token here;
    // server-side `/api/peers/register` already authenticated the user.
    let auth = vpn_common::psk_auth_token("");
    let mut hdr = Vec::new();
    hdr.extend_from_slice(&auth); // 16-byte PSK auth token
    hdr.extend_from_slice(&addr_bytes); // ATYP + address
    hdr.extend_from_slice(&port.to_be_bytes()); // 2-byte port
    vs.write_all(&fc.encode(&hdr)).await?;

    // Receive status frame from server
    let status = recv_frame(&mut vs, &fc).await?;
    if status.first() != Some(&0) {
        cl.write_all(&[5, 5, 0, 1, 0, 0, 0, 0, 0, 0]).await?;
        anyhow::bail!("Proxy rejected connection");
    }

    // Tell the SOCKS5 client that the connection was established
    cl.write_all(&[5, 0, 0, 1, 0, 0, 0, 0, 0, 0]).await?;

    // ── Bidirectional relay ───────────────────────────────────────────────────
    let fc = Arc::new(fc);
    let (mut crx, mut ctx) = cl.into_split(); // SOCKS5 client halves
    let (mut vrx, mut vtx) = vs.into_split(); // VPN proxy halves
    let fc1 = fc.clone();

    // Client → proxy: read from SOCKS5 client, encrypt, send to VPN proxy
    let t1 = tokio::spawn(async move {
        let mut tmp = vec![0u8; 65535];
        loop {
            let n = match crx.read(&mut tmp).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            if vtx.write_all(&fc1.encode(&tmp[..n])).await.is_err() {
                break;
            }
        }
    });

    // Proxy → client: read from VPN proxy, decrypt frames, send to SOCKS5 client
    let t2 = tokio::spawn(async move {
        let mut fb = Vec::<u8>::new();
        let mut tmp = vec![0u8; 65536];
        loop {
            let n = match vrx.read(&mut tmp).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            fb.extend_from_slice(&tmp[..n]);
            // Drain all complete frames from the accumulation buffer
            loop {
                match fc.decode(&fb) {
                    Some((p, c)) => {
                        if ctx.write_all(&p).await.is_err() {
                            return;
                        }
                        fb.drain(..c);
                    }
                    None => break,
                }
            }
        }
    });

    let _ = tokio::join!(t1, t2);
    Ok(())
}

/// Read a single [`FramedCrypto`] frame from a TCP stream and decrypt it.
///
/// Used for the proxy handshake status response (one short frame).
async fn recv_frame(
    s: &mut tokio::net::TcpStream,
    fc: &vpn_common::FramedCrypto,
) -> Result<Vec<u8>> {
    use tokio::io::AsyncReadExt;
    let mut lb = [0u8; 2];
    s.read_exact(&mut lb).await?;
    let cl = u16::from_be_bytes(lb) as usize;
    let mut raw = vec![0u8; 12 + cl]; // 12B nonce + ciphertext
    s.read_exact(&mut raw).await?;
    let mut buf = Vec::new();
    buf.extend_from_slice(&lb);
    buf.extend_from_slice(&raw);
    fc.decode(&buf)
        .map(|(p, _)| p)
        .context("Frame decrypt failed")
}

// ── Routing helpers (Unix) ────────────────────────────────────────────────────

/// Detect the current default gateway using `ip route`.
#[cfg(unix)]
fn get_gw() -> Result<String> {
    let o = std::process::Command::new("sh")
        .args(["-c", "ip route show default|awk '/default/{print $3;exit}'"])
        .output()?;
    let g = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if g.is_empty() {
        anyhow::bail!("No default gateway");
    }
    Ok(g)
}

/// Set up IP routing to send all traffic through the VPN tunnel.
///
/// In full-tunnel mode (the default):
/// 1. Adds a host route for the server IP via the original gateway
///    (so tunnel traffic doesn't loop back into itself).
/// 2. Replaces the default route to send everything via `10.0.0.1` on `tun0`.
///
/// In split-tunnel mode (`--split-tunnel`): only adds a route for the VPN
/// subnet (`10.0.0.0/24`), leaving other traffic unaffected.
#[cfg(unix)]
fn setup_routing(server: &str, gw: &str, split: bool) -> Result<()> {
    use std::process::Command;
    if split {
        Command::new("ip")
            .args(["route", "add", vpn_common::VPN_SUBNET_CIDR, "dev", "tun0"])
            .output()?;
        return Ok(());
    }
    // Full-tunnel: route server IP via original gateway to avoid loop
    let _ = Command::new("ip")
        .args(["route", "del", &format!("{server}/32")])
        .output();
    Command::new("ip")
        .args(["route", "add", &format!("{server}/32"), "via", gw])
        .output()?;
    Command::new("ip")
        .args([
            "route",
            "replace",
            "default",
            "via",
            vpn_common::VPN_SERVER_IP,
            "dev",
            "tun0",
        ])
        .output()?;
    Ok(())
}

/// Restore the routing table to its state before the VPN was connected.
#[cfg(unix)]
fn restore_routing(server: &str, gw: &str, split: bool) {
    use std::process::Command;
    if split {
        let _ = Command::new("ip")
            .args(["route", "del", vpn_common::VPN_SUBNET_CIDR, "dev", "tun0"])
            .output();
        return;
    }
    let _ = Command::new("ip")
        .args(["route", "del", &format!("{server}/32")])
        .output();
    let _ = Command::new("ip")
        .args(["route", "replace", "default", "via", gw])
        .output();
}

// ── HTTP API helpers ──────────────────────────────────────────────────────────

/// Build an HTTP client for API requests that ignores proxy-related env vars.
///
/// The Windows setup scripts set `HTTP_PROXY/HTTPS_PROXY/ALL_PROXY` to
/// `socks5h://127.0.0.1:<port>` after the local tunnel comes up. Reqwest does
/// not support every proxy URI variant from environment variables, so API calls
/// such as `/api/peers/register` must bypass env proxy auto-detection.
fn api_http_client() -> Result<reqwest::Client> {
    Ok(reqwest::Client::builder().no_proxy().build()?)
}

/// POST to an unauthenticated endpoint (no Bearer token).
///
/// Used for `/auth/register` and `/auth/login`.
async fn api_anon(
    server: &str,
    port: u16,
    path: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    Ok(api_http_client()?
        .post(format!("http://{}:{}{}", server, port, path))
        .json(body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// POST to an authenticated endpoint with a Bearer token.
async fn api_post(
    server: &str,
    port: u16,
    path: &str,
    tok: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    Ok(api_http_client()?
        .post(format!("http://{}:{}{}", server, port, path))
        .bearer_auth(tok)
        .json(body)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}

/// GET an authenticated endpoint with a Bearer token.
async fn api_get(server: &str, port: u16, path: &str, tok: &str) -> Result<serde_json::Value> {
    Ok(api_http_client()?
        .get(format!("http://{}:{}{}", server, port, path))
        .bearer_auth(tok)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?)
}
