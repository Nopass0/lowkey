//! VPN connection management — embedded Hysteria2 SOCKS5 proxy.
//!
//! Replaces the external `lowkey-vpn-client` binary with a Rust-native
//! Hysteria2 client that runs in-process.  The proxy is exposed as a
//! local SOCKS5 endpoint at `127.0.0.1:<socks5_port>`.

use anyhow::Result;
use std::sync::Mutex;
use crate::hysteria::HysteriaConnection;

static VPN_CONN: Mutex<Option<HysteriaConnection>> = Mutex::new(None);

/// Connect to the Hysteria2 server and start the local SOCKS5 proxy.
///
/// - `server_ip`     — Hysteria2 server hostname or IP address
/// - `hysteria_port` — Hysteria2 server port (default: 8388)
/// - `password`      — Hysteria2 authentication password (the PSK)
/// - `socks5_port`   — Local port to listen on for SOCKS5 (default: 10808)
pub async fn connect(
    server_ip: &str,
    hysteria_port: u16,
    password: &str,
    socks5_port: u16,
) -> Result<()> {
    // Tear down any existing connection first
    disconnect().await.ok();

    let conn = crate::hysteria::connect(server_ip, hysteria_port, password, socks5_port).await?;
    *VPN_CONN.lock().unwrap() = Some(conn);
    Ok(())
}

/// Stop the VPN connection and release the SOCKS5 port.
pub async fn disconnect() -> Result<()> {
    if let Some(conn) = VPN_CONN.lock().unwrap().take() {
        conn.abort();
    }
    Ok(())
}
