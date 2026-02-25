//! Lowkey VPN Client
//!
//! Usage
//! -----
//!   # Connect (runs in foreground; Ctrl-C disconnects cleanly)
//!   sudo vpn-client connect --server 1.2.3.4 --psk mysecret
//!
//!   # With custom ports
//!   sudo vpn-client connect --server 1.2.3.4 --psk mysecret \
//!       --api-port 8080 --udp-port 51820
//!
//! What it does
//! ------------
//!  1. Generates an ephemeral X25519 key pair.
//!  2. Registers with the server API (POST /api/peers/register).
//!  3. Creates a local TUN interface (tun0) with the assigned IP.
//!  4. Saves the current default gateway, then routes ALL traffic via the VPN:
//!       • server_ip/32  →  original gateway  (so tunnel traffic doesn't loop)
//!       • default       →  10.0.0.1 via tun0
//!  5. Runs two tasks:
//!       tun → udp : reads IP packets from TUN, encrypts, sends to server.
//!       udp → tun : receives encrypted packets from server, decrypts, writes to TUN.
//!  6. On Ctrl-C, restores the original routing and exits.

use std::{
    net::{Ipv4Addr, SocketAddr},
    sync::Arc,
};

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::UdpSocket,
    signal,
    sync::Mutex,
};
use tracing::{error, info, warn};

use vpn_common::{
    from_hex, to_hex, RegisterRequest, RegisterResponse, VpnCrypto,
    DEFAULT_API_PORT, VPN_NETMASK,
};
use x25519_dalek::{PublicKey, StaticSecret};

// ── CLI ───────────────────────────────────────────────────────────────────────

#[derive(Parser)]
#[command(
    name = "vpn-client",
    about = "Lowkey VPN Client — routes all traffic through a Lowkey VPN server"
)]
struct Args {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Connect to a VPN server (runs in the foreground; Ctrl-C to disconnect)
    Connect {
        /// Server IP or hostname
        #[arg(short, long)]
        server: String,

        /// Pre-shared key (must match the server's VPN_PSK)
        #[arg(short, long)]
        psk: String,

        /// Server HTTP API port
        #[arg(long, default_value_t = DEFAULT_API_PORT)]
        api_port: u16,

        /// Server UDP tunnel port (defaults to the port returned by the server)
        #[arg(long)]
        udp_port: Option<u16>,

        /// Route only VPN subnet traffic (split tunnel); default routes ALL traffic
        #[arg(long, default_value_t = false)]
        split_tunnel: bool,
    },
}

// ── Entry point ───────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("vpn_client=info".parse()?),
        )
        .init();

    let args = Args::parse();
    match args.command {
        Cmd::Connect {
            server,
            psk,
            api_port,
            udp_port,
            split_tunnel,
        } => connect(&server, api_port, udp_port, &psk, split_tunnel).await?,
    }
    Ok(())
}

// ── Connect flow ──────────────────────────────────────────────────────────────

async fn connect(
    server: &str,
    api_port: u16,
    udp_port_override: Option<u16>,
    psk: &str,
    split_tunnel: bool,
) -> Result<()> {
    // ── 1. Key pair ──────────────────────────────────────────────────────────
    let secret = StaticSecret::random_from_rng(rand::rngs::OsRng);
    let public = PublicKey::from(&secret);
    let client_pubkey = *public.as_bytes();
    info!("Client public key: {}", to_hex(&client_pubkey));

    // ── 2. Register ──────────────────────────────────────────────────────────
    info!("Registering with server {}:{} …", server, api_port);
    let http = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(10))
        .build()?;
    let reg_resp: RegisterResponse = http
        .post(format!("http://{}:{}/api/peers/register", server, api_port))
        .json(&RegisterRequest {
            public_key: to_hex(&client_pubkey),
            psk: psk.to_string(),
        })
        .send()
        .await
        .context("Could not reach server API")?
        .error_for_status()
        .context("Server rejected registration (wrong PSK?)")?
        .json()
        .await
        .context("Unexpected server response")?;

    let vpn_ip: Ipv4Addr = reg_resp.assigned_ip.parse().context("Invalid assigned IP")?;
    let udp_port = udp_port_override.unwrap_or(reg_resp.udp_port);
    info!(
        "Assigned IP: {}  |  server UDP port: {}",
        vpn_ip, udp_port
    );

    // ── 3. Shared secret ─────────────────────────────────────────────────────
    let spub_bytes = from_hex(&reg_resp.server_public_key)
        .filter(|b| b.len() == 32)
        .context("Invalid server public key")?;
    let mut spub_arr = [0u8; 32];
    spub_arr.copy_from_slice(&spub_bytes);
    let server_pub = PublicKey::from(spub_arr);
    let shared = secret.diffie_hellman(&server_pub);
    let crypto = Arc::new(VpnCrypto::from_shared_secret(&shared));

    // ── 4. TUN interface ─────────────────────────────────────────────────────
    let mut tun_config = tun::Configuration::default();
    tun_config
        .address(vpn_ip.to_string().as_str())
        .netmask(VPN_NETMASK)
        .destination("10.0.0.1")
        .up();
    #[cfg(target_os = "linux")]
    tun_config.platform(|c| {
        c.packet_information(false);
    });

    let tun_dev = tun::create_as_async(&tun_config)
        .context("Failed to create TUN device — are you running as root?")?;
    info!("TUN interface up (VPN IP: {})", vpn_ip);

    // ── 5. Routing ───────────────────────────────────────────────────────────
    let original_gw = get_default_gateway().context("Could not detect current default gateway")?;
    info!("Original gateway: {original_gw}");

    setup_routing(server, &original_gw, split_tunnel)
        .context("Failed to configure routing")?;
    info!(
        "Routing active ({})",
        if split_tunnel { "split tunnel" } else { "full tunnel" }
    );

    // ── 6. UDP socket ────────────────────────────────────────────────────────
    let udp = Arc::new(
        UdpSocket::bind("0.0.0.0:0")
            .await
            .context("Failed to bind UDP socket")?,
    );
    let server_udp: SocketAddr = format!("{server}:{udp_port}")
        .parse()
        .context("Invalid server address")?;

    // Send an announcement so the server learns our UDP endpoint immediately.
    send_announcement(&udp, &crypto, vpn_ip, server_udp).await?;

    // ── 7. Tunnel tasks ──────────────────────────────────────────────────────
    let (tun_rx, tun_tx) = tokio::io::split(tun_dev);
    let tun_tx = Arc::new(Mutex::new(tun_tx));

    let vpn_ip_bytes = vpn_ip.octets();

    // TUN → UDP
    {
        let (udp, crypto) = (udp.clone(), crypto.clone());
        tokio::spawn(async move {
            if let Err(e) =
                task_tun_to_udp(tun_rx, udp, crypto, vpn_ip_bytes, server_udp).await
            {
                error!("tun→udp: {e}");
            }
        });
    }

    // UDP → TUN
    {
        let (udp, crypto, tun_tx) = (udp.clone(), crypto.clone(), tun_tx.clone());
        tokio::spawn(async move {
            if let Err(e) = task_udp_to_tun(udp, tun_tx, crypto).await {
                error!("udp→tun: {e}");
            }
        });
    }

    // ── 8. Wait for Ctrl-C ───────────────────────────────────────────────────
    info!("VPN is active. Press Ctrl-C to disconnect.");
    signal::ctrl_c().await?;
    info!("Disconnecting …");

    // ── 9. Restore routing ───────────────────────────────────────────────────
    restore_routing(server, &original_gw, split_tunnel);
    info!("Routing restored. Disconnected.");

    Ok(())
}

// ── Announcement ──────────────────────────────────────────────────────────────

/// Send a tiny encrypted probe so the server learns our UDP endpoint.
async fn send_announcement(
    udp: &UdpSocket,
    crypto: &VpnCrypto,
    vpn_ip: Ipv4Addr,
    server: SocketAddr,
) -> Result<()> {
    let ip_bytes = vpn_ip.octets();
    let encrypted = crypto.encrypt(b"hello");
    let mut pkt = Vec::with_capacity(4 + encrypted.len());
    pkt.extend_from_slice(&ip_bytes);
    pkt.extend_from_slice(&encrypted);
    udp.send_to(&pkt, server)
        .await
        .context("Failed to send announcement")?;
    info!("Announcement sent to server");
    Ok(())
}

// ── Tunnel tasks ──────────────────────────────────────────────────────────────

/// Read IP packets from TUN → prepend VPN IP header → encrypt → send to server.
async fn task_tun_to_udp(
    mut tun: impl AsyncReadExt + Unpin,
    udp: Arc<UdpSocket>,
    crypto: Arc<VpnCrypto>,
    vpn_ip_bytes: [u8; 4],
    server: SocketAddr,
) -> Result<()> {
    let mut buf = vec![0u8; 65536];
    loop {
        let n = tun.read(&mut buf).await?;
        if n == 0 {
            break;
        }
        let pkt = &buf[..n];
        let encrypted = crypto.encrypt(pkt);

        // Wire: [4 B VPN IP] [encrypted]
        let mut wire = Vec::with_capacity(4 + encrypted.len());
        wire.extend_from_slice(&vpn_ip_bytes);
        wire.extend_from_slice(&encrypted);

        if let Err(e) = udp.send_to(&wire, server).await {
            warn!("UDP send failed: {e}");
        }
    }
    Ok(())
}

/// Receive encrypted packets from server → decrypt → write to TUN.
async fn task_udp_to_tun(
    udp: Arc<UdpSocket>,
    tun: Arc<Mutex<impl AsyncWriteExt + Unpin>>,
    crypto: Arc<VpnCrypto>,
) -> Result<()> {
    let mut buf = vec![0u8; 65536 + 64];
    loop {
        let (n, _src) = udp.recv_from(&mut buf).await?;
        let data = &buf[..n];

        let plain = match crypto.decrypt(data) {
            Some(p) => p,
            None => {
                warn!("Decryption failed (packet dropped)");
                continue;
            }
        };

        let mut tw = tun.lock().await;
        if let Err(e) = tw.write_all(&plain).await {
            error!("TUN write error: {e}");
        }
    }
}

// ── Routing helpers ───────────────────────────────────────────────────────────

fn get_default_gateway() -> Result<String> {
    let out = std::process::Command::new("sh")
        .args(["-c", "ip route show default | awk '/default/ {print $3; exit}'"])
        .output()
        .context("ip route failed")?;
    let gw = String::from_utf8_lossy(&out.stdout).trim().to_string();
    if gw.is_empty() {
        anyhow::bail!("No default gateway found");
    }
    Ok(gw)
}

fn setup_routing(server: &str, original_gw: &str, split_tunnel: bool) -> Result<()> {
    use std::process::Command;

    if split_tunnel {
        // Only route VPN subnet traffic through tun0; everything else is unchanged.
        Command::new("ip")
            .args(["route", "add", "10.0.0.0/24", "dev", "tun0"])
            .output()
            .context("add VPN subnet route")?;
        return Ok(());
    }

    // Full tunnel: route ALL traffic via VPN.
    // Step 1: make sure the VPN server itself is reachable via the real interface.
    let _ = Command::new("ip")
        .args(["route", "del", &format!("{server}/32")])
        .output();
    Command::new("ip")
        .args(["route", "add", &format!("{server}/32"), "via", original_gw])
        .output()
        .context("add server-specific route")?;

    // Step 2: replace the default route.
    Command::new("ip")
        .args(["route", "replace", "default", "via", "10.0.0.1", "dev", "tun0"])
        .output()
        .context("replace default route")?;

    Ok(())
}

fn restore_routing(server: &str, original_gw: &str, split_tunnel: bool) {
    use std::process::Command;

    if split_tunnel {
        let _ = Command::new("ip")
            .args(["route", "del", "10.0.0.0/24", "dev", "tun0"])
            .output();
        return;
    }

    // Remove the server-specific route.
    let _ = Command::new("ip")
        .args(["route", "del", &format!("{server}/32")])
        .output();

    // Restore the original default route.
    if let Err(e) = Command::new("ip")
        .args(["route", "replace", "default", "via", original_gw])
        .output()
    {
        error!("Failed to restore default route: {e}");
    }
}
