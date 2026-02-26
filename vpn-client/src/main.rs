//! # Lowkey VPN Client
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

use std::sync::Arc;
use anyhow::{Context, Result};
use clap::{Parser, Subcommand, ValueEnum};
use serde::{Deserialize, Serialize};
use tokio::{signal, sync::Mutex};
use tracing::info;
use vpn_common::{from_hex, to_hex, VpnCrypto, DEFAULT_API_PORT};
use x25519_dalek::{PublicKey, StaticSecret};

#[cfg(unix)]
use tokio::io::{AsyncReadExt, AsyncWriteExt};

// ── Session storage ───────────────────────────────────────────────────────────

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

/// Return the path to the session file: `~/.config/lowkey/session.json`.
fn session_path() -> std::path::PathBuf {
    let home = std::env::var("HOME").unwrap_or_else(|_| ".".into());
    std::path::PathBuf::from(home).join(".config").join("lowkey").join("session.json")
}

/// Load the session from disk.  Returns a default (empty) session if the
/// file does not exist or cannot be parsed.
fn load_session() -> Session {
    std::fs::read_to_string(session_path())
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
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
    /// Create a TUN device and route all traffic.  Requires root (Unix only).
    Tun,
    /// Start a local SOCKS5 proxy that tunnels through the server.
    Socks5,
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
        /// Connection mode: `tun` (Linux/macOS, root) or `socks5` (all platforms).
        #[arg(long, default_value = "tun")]
        mode: Mode,
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
        Cmd::Promo { server, api_port, code } => {
            let session = load_session();
            let srv = server.or(session.server.clone()).context("--server required")?;
            let tok = session.token.context("Not logged in")?;
            let resp = api_post(&srv, api_port, "/promo/apply", &tok,
                &serde_json::json!({ "code": code })).await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        Cmd::Connect { server, api_port, mode, udp_port, proxy_port, socks_port, split_tunnel } => {
            let session = load_session();
            let srv = server.or(session.server.clone()).context("--server required")?;
            let tok = session.token.context("Not logged in. Run: vpn-client auth login")?;
            connect(&srv, api_port, udp_port, proxy_port, &tok, mode, socks_port, split_tunnel).await?;
        }
    }
    Ok(())
}

// ── Auth command handlers ─────────────────────────────────────────────────────

/// Handle all `auth` subcommands.
async fn handle_auth(cmd: AuthCmd) -> Result<()> {
    match cmd {
        AuthCmd::Register { server, api_port, login, password } => {
            // Register and automatically save the returned token
            let resp = api_anon(&server, api_port, "/auth/register",
                &serde_json::json!({ "login": login, "password": password })).await?;
            let tok = resp["token"].as_str().unwrap_or("").to_string();
            println!("Registered as '{}'\n{}", login, serde_json::to_string_pretty(&resp["user"])?);
            save_session(&Session {
                token: Some(tok),
                server: Some(server),
                api_port: Some(api_port),
            })?;
        }
        AuthCmd::Login { server, api_port, login, password } => {
            // Login and save the returned token to the session file
            let resp = api_anon(&server, api_port, "/auth/login",
                &serde_json::json!({ "login": login, "password": password })).await?;
            let tok = resp["token"].as_str().context("No token")?.to_string();
            println!("Logged in as '{}'\n{}", login, serde_json::to_string_pretty(&resp["user"])?);
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
            println!("{}", serde_json::to_string_pretty(
                &api_get(&srv, api_port, "/auth/me", &tok).await?
            )?);
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
            let srv = server.or(s.server).context("--server required")?;
            // Plans endpoint is public — no token needed
            let resp: serde_json::Value = reqwest::Client::new()
                .get(format!("http://{}:{}/subscription/plans", srv, api_port))
                .send().await?.json().await?;
            println!("{}", serde_json::to_string_pretty(&resp)?);
        }
        SubCmd::Buy { server, api_port, plan } => {
            let s = load_session();
            let srv = server.or(s.server.clone()).context("--server required")?;
            let tok = s.token.context("Not logged in")?;
            let resp = api_post(&srv, api_port, "/subscription/buy", &tok,
                &serde_json::json!({ "plan_id": plan })).await?;
            println!("Subscription activated!\n{}", serde_json::to_string_pretty(&resp)?);
        }
        SubCmd::Status { server, api_port } => {
            let s = load_session();
            let srv = server.or(s.server).context("--server required")?;
            let tok = s.token.context("Not logged in")?;
            println!("{}", serde_json::to_string_pretty(
                &api_get(&srv, api_port, "/subscription/status", &tok).await?
            )?);
        }
    }
    Ok(())
}

// ── VPN connect ───────────────────────────────────────────────────────────────

/// Register a peer with the server and start the VPN in the requested mode.
///
/// Performs the API registration handshake to get an assigned VPN IP and the
/// server's X25519 public key, then dispatches to either [`run_tun_mode`] or
/// [`run_socks5_mode`].
async fn connect(
    server: &str,
    api_port: u16,
    udp_override: Option<u16>,
    proxy_override: Option<u16>,
    token: &str,
    mode: Mode,
    socks_port: u16,
    split: bool,
) -> Result<()> {
    // Generate a fresh ephemeral X25519 key pair for this session
    let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let public = PublicKey::from(&secret);

    // Register with the server: sends our public key, gets VPN IP + server pubkey
    let reg = api_post(server, api_port, "/api/peers/register", token, &serde_json::json!({
        "public_key": to_hex(public.as_bytes()),
        "psk": ""  // empty = skip legacy PSK check
    }))
    .await
    .context("Registration failed — check subscription")?;

    // Parse the assigned VPN IP
    let vpn_ip: std::net::Ipv4Addr = reg["assigned_ip"]
        .as_str().context("No assigned_ip")?.parse()?;

    // Use override ports if provided, otherwise use the server's advertised ports
    let udp_port   = udp_override
        .or_else(|| reg["udp_port"].as_u64().map(|p| p as u16))
        .unwrap_or(51820);
    let proxy_port = proxy_override
        .or_else(|| reg["proxy_port"].as_u64().map(|p| p as u16))
        .unwrap_or(8388);

    // Parse server's X25519 public key and complete the DH handshake
    let spub: Vec<u8> = from_hex(reg["server_public_key"].as_str().unwrap_or(""))
        .filter(|b| b.len() == 32)
        .context("Bad server pubkey")?;
    let mut spub_arr = [0u8; 32];
    spub_arr.copy_from_slice(&spub);
    let shared = secret.diffie_hellman(&PublicKey::from(spub_arr));
    let crypto = Arc::new(VpnCrypto::from_shared_secret(&shared));

    info!("VPN IP: {}  mode: {:?}", vpn_ip, mode);

    match mode {
        Mode::Tun => {
            #[cfg(unix)]
            run_tun_mode(server, vpn_ip, udp_port, crypto, split).await?;
            #[cfg(not(unix))]
            anyhow::bail!("TUN requires Linux/macOS. Use --mode socks5");
        }
        Mode::Socks5 => run_socks5_mode(server, proxy_port, socks_port, token, &secret).await?,
    }
    Ok(())
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
    cfg.platform(|c| { c.packet_information(false); });
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
        p.extend_from_slice(&ib);  // 4-byte VPN IP prefix
        p.extend_from_slice(&enc);
        udp.send_to(&p, srv).await?;
    }

    // Split TUN into read/write halves; protect write half with a Mutex
    let (rx, tx) = tokio::io::split(dev);
    let tx = Arc::new(Mutex::new(tx));

    // Spawn forwarding tasks
    {
        let (u, c) = (udp.clone(), crypto.clone());
        tokio::spawn(async move { let _ = tun_to_udp(rx, u, c, ib, srv).await; });
    }
    {
        let (u, c, tw) = (udp.clone(), crypto.clone(), tx.clone());
        tokio::spawn(async move { let _ = udp_to_tun(u, tw, c).await; });
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
    ib: [u8; 4],  // client VPN IP bytes (routing header)
    srv: std::net::SocketAddr,
) -> Result<()> {
    let mut buf = vec![0u8; 65536];
    loop {
        let n = tun.read(&mut buf).await?;
        if n == 0 { break; }
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
async fn run_socks5_mode(
    server: &str,
    proxy_port: u16,
    socks_port: u16,
    token: &str,   // kept for future per-user proxy auth
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
    let tok = token.to_string();
    let ctrl_c = signal::ctrl_c();
    tokio::pin!(ctrl_c);

    loop {
        tokio::select! {
            _ = &mut ctrl_c => break,
            res = listener.accept() => {
                let (stream, _) = res?;
                let sa = sa.clone();
                let sb = sb;
                let tok = tok.clone();
                tokio::spawn(async move {
                    if let Err(e) = socks5_conn(stream, &sa, &sb, &tok).await {
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
    vpn_addr: &str,  // server TCP proxy address (ip:port)
    sb: &[u8; 32],  // our X25519 secret key bytes
    psk: &str,       // PSK for the proxy auth token
) -> Result<()> {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let mut buf = [0u8; 512];

    // ── SOCKS5 greeting ───────────────────────────────────────────────────────
    // Client sends: [VER=5] [NMETHODS] [methods...]
    cl.read_exact(&mut buf[..2]).await?;
    if buf[0] != 5 { anyhow::bail!("Not SOCKS5"); }
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
    let my_pub    = PublicKey::from(&my_secret);
    let mut vs = tokio::net::TcpStream::connect(vpn_addr)
        .await
        .context("VPN proxy connect failed")?;
    vs.write_all(my_pub.as_bytes()).await?;      // send our ephemeral pubkey
    let mut spb = [0u8; 32];
    vs.read_exact(&mut spb).await?;              // receive server pubkey
    let fc = vpn_common::FramedCrypto::new(&my_secret, &PublicKey::from(spb));

    // Build and send the encrypted connect-header
    let auth = vpn_common::psk_auth_token(psk);
    let mut hdr = Vec::new();
    hdr.extend_from_slice(&auth);       // 16-byte PSK auth token
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
    let (mut crx, mut ctx) = cl.into_split();  // SOCKS5 client halves
    let (mut vrx, mut vtx) = vs.into_split();  // VPN proxy halves
    let fc1 = fc.clone();

    // Client → proxy: read from SOCKS5 client, encrypt, send to VPN proxy
    let t1 = tokio::spawn(async move {
        let mut tmp = vec![0u8; 65535];
        loop {
            let n = match crx.read(&mut tmp).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            if vtx.write_all(&fc1.encode(&tmp[..n])).await.is_err() { break; }
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
                        if ctx.write_all(&p).await.is_err() { return; }
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
    fc.decode(&buf).map(|(p, _)| p).context("Frame decrypt failed")
}

// ── Routing helpers (Unix) ────────────────────────────────────────────────────

/// Detect the current default gateway using `ip route`.
#[cfg(unix)]
fn get_gw() -> Result<String> {
    let o = std::process::Command::new("sh")
        .args(["-c", "ip route show default|awk '/default/{print $3;exit}'"])
        .output()?;
    let g = String::from_utf8_lossy(&o.stdout).trim().to_string();
    if g.is_empty() { anyhow::bail!("No default gateway"); }
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
        Command::new("ip").args(["route", "add", vpn_common::VPN_SUBNET_CIDR, "dev", "tun0"]).output()?;
        return Ok(());
    }
    // Full-tunnel: route server IP via original gateway to avoid loop
    let _ = Command::new("ip").args(["route", "del", &format!("{server}/32")]).output();
    Command::new("ip").args(["route", "add", &format!("{server}/32"), "via", gw]).output()?;
    Command::new("ip").args(["route", "replace", "default", "via", vpn_common::VPN_SERVER_IP, "dev", "tun0"]).output()?;
    Ok(())
}

/// Restore the routing table to its state before the VPN was connected.
#[cfg(unix)]
fn restore_routing(server: &str, gw: &str, split: bool) {
    use std::process::Command;
    if split {
        let _ = Command::new("ip").args(["route", "del", vpn_common::VPN_SUBNET_CIDR, "dev", "tun0"]).output();
        return;
    }
    let _ = Command::new("ip").args(["route", "del", &format!("{server}/32")]).output();
    let _ = Command::new("ip").args(["route", "replace", "default", "via", gw]).output();
}

// ── HTTP API helpers ──────────────────────────────────────────────────────────

/// POST to an unauthenticated endpoint (no Bearer token).
///
/// Used for `/auth/register` and `/auth/login`.
async fn api_anon(
    server: &str,
    port: u16,
    path: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    Ok(reqwest::Client::new()
        .post(format!("http://{}:{}{}", server, port, path))
        .json(body)
        .send().await?
        .error_for_status()?
        .json().await?)
}

/// POST to an authenticated endpoint with a Bearer token.
async fn api_post(
    server: &str,
    port: u16,
    path: &str,
    tok: &str,
    body: &serde_json::Value,
) -> Result<serde_json::Value> {
    Ok(reqwest::Client::new()
        .post(format!("http://{}:{}{}", server, port, path))
        .bearer_auth(tok)
        .json(body)
        .send().await?
        .error_for_status()?
        .json().await?)
}

/// GET an authenticated endpoint with a Bearer token.
async fn api_get(
    server: &str,
    port: u16,
    path: &str,
    tok: &str,
) -> Result<serde_json::Value> {
    Ok(reqwest::Client::new()
        .get(format!("http://{}:{}{}", server, port, path))
        .bearer_auth(tok)
        .send().await?
        .error_for_status()?
        .json().await?)
}
