//! Lowkey VPN Server
//!
//! Architecture
//! ============
//!   ┌──────────┐   encrypted UDP   ┌─────────────────┐
//!   │  Client  │ ◄───────────────► │   vpn-server    │
//!   └──────────┘                   │                 │
//!                                  │  TUN (10.0.0.1) │
//!                                  │  iptables NAT   │
//!                                  │  HTTP API :8080 │
//!                                  └─────────────────┘
//!
//! UDP packet format (Client → Server):
//!   [4 B: client VPN IP] [12 B: nonce] [ciphertext + 16 B tag]
//!
//! UDP packet format (Server → Client):
//!   [12 B: nonce] [ciphertext + 16 B tag]

use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};

use anyhow::{Context, Result};
use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use clap::Parser;
use dashmap::DashMap;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UdpSocket,
    sync::{Mutex, RwLock},
};
use tracing::{error, info, warn};

use vpn_common::{
    from_hex, parse_dest_ipv4, to_hex, PeerInfo, RegisterRequest, RegisterResponse, StatusResponse,
    VpnCrypto, DEFAULT_API_PORT, DEFAULT_UDP_PORT, VPN_NETMASK, VPN_SERVER_IP, VPN_SUBNET,
    VPN_SUBNET_CIDR,
};
use x25519_dalek::{PublicKey, StaticSecret};

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser, Debug)]
#[command(
    name = "vpn-server",
    about = "Lowkey VPN Server — routes all client traffic via NAT"
)]
struct Args {
    /// HTTP API port (for peer management)
    #[arg(long, default_value_t = DEFAULT_API_PORT)]
    api_port: u16,

    /// UDP tunnel port
    #[arg(long, default_value_t = DEFAULT_UDP_PORT)]
    udp_port: u16,

    /// Pre-shared key used for client authentication (or set VPN_PSK env var)
    #[arg(long, env = "VPN_PSK")]
    psk: String,
}

// ── State ─────────────────────────────────────────────────────────────────────

struct Peer {
    vpn_ip: Ipv4Addr,
    /// Set to Some(...) once the server receives the first UDP packet from the client.
    endpoint: Option<SocketAddr>,
    crypto: VpnCrypto,
}

struct ServerState {
    /// VPN IP  →  Peer
    peers: DashMap<Ipv4Addr, Arc<RwLock<Peer>>>,
    /// UDP endpoint  →  VPN IP (filled lazily from incoming packets)
    endpoints: DashMap<SocketAddr, Ipv4Addr>,
    next_octet: Mutex<u8>,
    server_secret: [u8; 32],
    server_pubkey: [u8; 32],
    psk: String,
    udp_port: u16,
}

type Shared = Arc<ServerState>;

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("vpn_server=info".parse()?),
        )
        .init();

    let args = Args::parse();

    // Generate a fresh server X25519 key pair on every start.
    let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let public = PublicKey::from(&secret);
    let server_secret = secret.to_bytes();
    let server_pubkey = *public.as_bytes();
    info!("Server public key: {}", to_hex(&server_pubkey));

    let state: Shared = Arc::new(ServerState {
        peers: DashMap::new(),
        endpoints: DashMap::new(),
        next_octet: Mutex::new(2), // First client gets 10.0.0.2
        server_secret,
        server_pubkey,
        psk: args.psk.clone(),
        udp_port: args.udp_port,
    });

    // ── TUN interface ────────────────────────────────────────────────────────
    let mut tun_config = tun::Configuration::default();
    tun_config
        .address(VPN_SERVER_IP)
        .netmask(VPN_NETMASK)
        .destination(VPN_SUBNET)
        .up();
    #[cfg(target_os = "linux")]
    tun_config.platform(|c| {
        c.packet_information(false);
    });

    let tun_dev = tun::create_as_async(&tun_config)
        .context("Failed to create TUN device — are you running as root?")?;
    info!("TUN device created (server IP: {})", VPN_SERVER_IP);

    // ── Kernel routing / NAT ─────────────────────────────────────────────────
    setup_server_nat().context("Failed to configure iptables / IP forwarding")?;

    // ── UDP socket ───────────────────────────────────────────────────────────
    let udp = Arc::new(
        UdpSocket::bind(format!("0.0.0.0:{}", args.udp_port))
            .await
            .context("Failed to bind UDP socket")?,
    );
    info!("UDP tunnel listening on 0.0.0.0:{}", args.udp_port);

    // ── Split TUN for independent read / write ───────────────────────────────
    let (tun_rx, tun_tx) = tokio::io::split(tun_dev);
    let tun_tx = Arc::new(Mutex::new(tun_tx));

    // TUN → UDP: forward VPN-destined packets to the right peer
    {
        let (state, udp) = (state.clone(), udp.clone());
        tokio::spawn(async move {
            if let Err(e) = task_tun_to_udp(tun_rx, udp, state).await {
                error!("tun→udp task exited: {e}");
            }
        });
    }

    // UDP → TUN: receive encrypted packets, decrypt, inject into kernel
    {
        let (state, udp, tun_tx) = (state.clone(), udp.clone(), tun_tx.clone());
        tokio::spawn(async move {
            if let Err(e) = task_udp_to_tun(udp, tun_tx, state).await {
                error!("udp→tun task exited: {e}");
            }
        });
    }

    // ── HTTP API ─────────────────────────────────────────────────────────────
    let app = Router::new()
        .route("/api/status", get(api_status))
        .route("/api/peers", get(api_list_peers))
        .route("/api/peers/register", post(api_register))
        .route("/api/peers/:ip", delete(api_remove_peer))
        .with_state(state.clone());

    let api_addr = format!("0.0.0.0:{}", args.api_port);
    info!("HTTP API listening on {api_addr}");
    let listener = tokio::net::TcpListener::bind(&api_addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

// ── Kernel configuration ──────────────────────────────────────────────────────

fn setup_server_nat() -> Result<()> {
    use std::process::Command;

    // Enable IP forwarding
    std::fs::write("/proc/sys/net/ipv4/ip_forward", "1")
        .context("Failed to enable IP forwarding")?;
    info!("IP forwarding enabled");

    // Remove stale rules silently, then add fresh ones.
    let _ = Command::new("iptables")
        .args([
            "-t", "nat", "-D", "POSTROUTING",
            "-s", VPN_SUBNET_CIDR, "!", "-o", "tun0",
            "-j", "MASQUERADE",
        ])
        .output();
    Command::new("iptables")
        .args([
            "-t", "nat", "-A", "POSTROUTING",
            "-s", VPN_SUBNET_CIDR, "!", "-o", "tun0",
            "-j", "MASQUERADE",
        ])
        .output()
        .context("iptables MASQUERADE")?;

    for dir in ["-i", "-o"] {
        let _ = Command::new("iptables")
            .args(["-D", "FORWARD", dir, "tun0", "-j", "ACCEPT"])
            .output();
        Command::new("iptables")
            .args(["-A", "FORWARD", dir, "tun0", "-j", "ACCEPT"])
            .output()
            .with_context(|| format!("iptables FORWARD {dir} tun0"))?;
    }

    info!("iptables NAT rules applied");
    Ok(())
}

// ── Tunnel tasks ──────────────────────────────────────────────────────────────

/// Read raw IP packets from TUN → encrypt → send to the appropriate peer via UDP.
async fn task_tun_to_udp(
    mut tun: impl AsyncReadExt + Unpin,
    udp: Arc<UdpSocket>,
    state: Shared,
) -> Result<()> {
    let mut buf = vec![0u8; 65536];
    loop {
        let n = tun.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let pkt = &buf[..n];

        let dest = match parse_dest_ipv4(pkt) {
            Some(ip) => ip,
            None => continue,
        };

        // Only handle 10.0.0.0/24 traffic (VPN clients)
        let oct = dest.octets();
        if oct[0] != 10 || oct[1] != 0 || oct[2] != 0 {
            continue;
        }

        let peer_arc = match state.peers.get(&dest) {
            Some(p) => p.clone(),
            None => continue,
        };

        let peer = peer_arc.read().await;
        let endpoint = match peer.endpoint {
            Some(ep) => ep,
            None => continue, // Client hasn't sent a UDP packet yet
        };

        let encrypted = peer.crypto.encrypt(pkt);
        if let Err(e) = udp.send_to(&encrypted, endpoint).await {
            warn!("UDP send to {endpoint} failed: {e}");
        }
    }
    Ok(())
}

/// Receive encrypted UDP packets from clients → decrypt → write to TUN.
///
/// Wire format (Client → Server):
///   [4 B: client VPN IP] [12 B: nonce] [ciphertext]
async fn task_udp_to_tun(
    udp: Arc<UdpSocket>,
    tun: Arc<Mutex<impl AsyncWriteExt + Unpin>>,
    state: Shared,
) -> Result<()> {
    // Extra headroom: 4 B header + 12 B nonce + 16 B tag + max IP packet
    let mut buf = vec![0u8; 65536 + 64];
    loop {
        let (n, src) = udp.recv_from(&mut buf).await?;
        if n < 5 {
            continue;
        }

        // First 4 bytes identify the client's VPN IP.
        let vpn_ip = Ipv4Addr::new(buf[0], buf[1], buf[2], buf[3]);
        let payload = &buf[4..n];

        let peer_arc = match state.peers.get(&vpn_ip) {
            Some(p) => p.clone(),
            None => {
                warn!("Packet from unknown VPN IP {vpn_ip} (src={src})");
                continue;
            }
        };

        // Learn / update the client's real UDP endpoint.
        {
            let mut peer = peer_arc.write().await;
            if peer.endpoint != Some(src) {
                info!("Endpoint for {vpn_ip}: {src}");
                if let Some(old) = peer.endpoint {
                    state.endpoints.remove(&old);
                }
                peer.endpoint = Some(src);
                state.endpoints.insert(src, vpn_ip);
            }
        }

        let peer = peer_arc.read().await;
        let plain = match peer.crypto.decrypt(payload) {
            Some(p) => p,
            None => {
                warn!("Decryption failed from {src}");
                continue;
            }
        };

        // Skip keepalive probes (single-byte "hello")
        if plain == b"hello" {
            continue;
        }

        let mut tw = tun.lock().await;
        if let Err(e) = tw.write_all(&plain).await {
            error!("TUN write error: {e}");
        }
    }
}

// ── HTTP API handlers ─────────────────────────────────────────────────────────

async fn api_status(State(s): State<Shared>) -> Json<StatusResponse> {
    Json(StatusResponse {
        running: true,
        peer_count: s.peers.len(),
        server_vpn_ip: VPN_SERVER_IP.to_string(),
        udp_port: s.udp_port,
    })
}

async fn api_list_peers(State(s): State<Shared>) -> Json<Vec<PeerInfo>> {
    let mut out = Vec::new();
    for entry in s.peers.iter() {
        let peer = entry.value().read().await;
        out.push(PeerInfo {
            vpn_ip: peer.vpn_ip.to_string(),
            endpoint: peer
                .endpoint
                .map(|e| e.to_string())
                .unwrap_or_else(|| "pending".into()),
        });
    }
    Json(out)
}

async fn api_register(
    State(s): State<Shared>,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, (StatusCode, String)> {
    // Authenticate
    if req.psk != s.psk {
        return Err((StatusCode::UNAUTHORIZED, "Invalid PSK".into()));
    }

    // Parse client public key
    let pub_bytes = from_hex(&req.public_key)
        .filter(|b| b.len() == 32)
        .ok_or((StatusCode::BAD_REQUEST, "Invalid public key".into()))?;
    let mut pub_arr = [0u8; 32];
    pub_arr.copy_from_slice(&pub_bytes);

    // X25519 key exchange
    let server_secret = StaticSecret::from(s.server_secret);
    let client_pub = PublicKey::from(pub_arr);
    let shared = server_secret.diffie_hellman(&client_pub);
    let crypto = VpnCrypto::from_shared_secret(&shared);

    // Assign next available VPN IP (10.0.0.2 .. 10.0.0.254)
    let octet = {
        let mut next = s.next_octet.lock().await;
        let o = *next;
        *next = if *next >= 254 { 2 } else { *next + 1 };
        o
    };
    let vpn_ip = Ipv4Addr::new(10, 0, 0, octet);

    s.peers.insert(
        vpn_ip,
        Arc::new(RwLock::new(Peer {
            vpn_ip,
            endpoint: None,
            crypto,
        })),
    );

    info!("Registered peer → {vpn_ip}");

    Ok(Json(RegisterResponse {
        server_public_key: to_hex(&s.server_pubkey),
        assigned_ip: vpn_ip.to_string(),
        udp_port: s.udp_port,
        subnet: VPN_SUBNET_CIDR.to_string(),
    }))
}

async fn api_remove_peer(
    State(s): State<Shared>,
    Path(ip): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let vpn_ip: Ipv4Addr = ip
        .parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid IP".into()))?;

    match s.peers.remove(&vpn_ip) {
        Some((_, peer_arc)) => {
            let peer = peer_arc.read().await;
            if let Some(ep) = peer.endpoint {
                s.endpoints.remove(&ep);
            }
            info!("Removed peer {vpn_ip}");
            Ok(Json(serde_json::json!({ "status": "removed", "vpn_ip": ip })))
        }
        None => Err((StatusCode::NOT_FOUND, "Peer not found".into())),
    }
}
