//! TCP proxy — VLESS-style encrypted proxy for Windows/any-platform clients.
//!
//! Clients that cannot create TUN devices (e.g. Windows without admin rights,
//! Android apps, browsers) use this proxy instead of the UDP tunnel.  The
//! client runs a local SOCKS5 server that forwards traffic through this proxy.
//!
//! ## Protocol overview
//!
//! ```text
//! Client                              Server
//!   |                                   |
//!   |--- TCP connect ------------------>|
//!   |--- [32 B: ephemeral pubkey] ----->|  X25519 key exchange
//!   |<-- [32 B: server pubkey] ---------|
//!   |  (both sides derive shared key via DH + HKDF)
//!   |                                   |
//!   |--- encrypted "connect header" --->|  FramedCrypto frame
//!   |    [16 B: SHA256(psk)[0..16]]     |  PSK auth token
//!   |    [1 B:  addr_type]              |  1=IPv4, 3=hostname, 4=IPv6
//!   |    [N B:  address]                |
//!   |    [2 B:  port BE]                |
//!   |                                   |-- connects to target host:port
//!   |<-- encrypted status [1 B] --------|  0x00=ok, 0x01=auth fail, 0x02=conn fail
//!   |                                   |
//!   |=== encrypted relay (bidirectional) ===================>|
//! ```
//!
//! All subsequent traffic is framed with [`FramedCrypto`]:
//! `[2 B len] [12 B nonce] [ciphertext+tag]`

use std::sync::atomic::Ordering;

use anyhow::{bail, Result};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{TcpListener, TcpStream},
};
use tracing::{info, warn};
use x25519_dalek::{PublicKey, StaticSecret};
use vpn_common::{FramedCrypto, psk_auth_token};

use crate::state::Shared;

// ── Proxy server ──────────────────────────────────────────────────────────────

/// Accept TCP connections and handle each one in a new task.
///
/// Runs for the lifetime of the server.  Each accepted connection is handed
/// off to [`handle_proxy_conn`] in a separate `tokio::spawn` so a slow or
/// misbehaving client cannot block others.
pub async fn run_proxy_server(state: Shared) -> Result<()> {
    let addr = format!("0.0.0.0:{}", state.proxy_port);
    let listener = TcpListener::bind(&addr).await?;
    info!("TCP proxy listening on {addr}");

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let state = state.clone();
        tokio::spawn(async move {
            if let Err(e) = handle_proxy_conn(stream, state).await {
                warn!("Proxy connection from {peer_addr} error: {e}");
            }
        });
    }
}

// ── Connection handler ────────────────────────────────────────────────────────

/// Handle a single proxy client connection end-to-end.
///
/// Performs the X25519 handshake, authenticates the client via the PSK token,
/// connects to the requested target host, and then enters the bidirectional
/// relay loop.
async fn handle_proxy_conn(mut stream: TcpStream, state: Shared) -> Result<()> {
    // ── Step 1: Receive client ephemeral public key (32 bytes) ────────────────
    let mut client_pub_bytes = [0u8; 32];
    stream.read_exact(&mut client_pub_bytes).await?;

    // ── Step 2: Send server's static public key ───────────────────────────────
    stream.write_all(&state.server_pubkey).await?;
    stream.flush().await?;

    // ── Step 3: Derive FramedCrypto session key via X25519 + HKDF ────────────
    let server_secret = StaticSecret::from(state.server_secret);
    let client_pub = PublicKey::from(client_pub_bytes);
    let fc = FramedCrypto::new(&server_secret, &client_pub);

    // ── Step 4: Read and parse the encrypted connect header ───────────────────
    let connect_hdr = read_frame(&mut stream, &fc).await?;
    if connect_hdr.len() < 20 {
        bail!("Connect header too short");
    }

    // Verify the PSK auth token (first 16 bytes of the header).
    //
    // Historically, older clients authenticated users via `/api/peers/register`
    // and then sent an empty PSK token in proxy mode. Keep that flow working
    // so Windows SOCKS5 mode remains compatible with existing installers.
    let expected_token = psk_auth_token(&state.psk);
    let legacy_empty_token = psk_auth_token("");
    if connect_hdr[..16] != expected_token && connect_hdr[..16] != legacy_empty_token {
        let frame = fc.encode(&[0x01]); // 0x01 = auth failure
        let _ = stream.write_all(&frame).await;
        bail!("Auth token mismatch");
    }
    if connect_hdr[..16] == legacy_empty_token {
        warn!("Proxy auth accepted legacy empty PSK token");
    }

    // Parse the target address from the remaining header bytes
    let (target_host, target_port) = parse_target(&connect_hdr[16..])?;
    let target_addr = format!("{target_host}:{target_port}");

    // ── Step 5: Connect to the upstream target ────────────────────────────────
    let target = match TcpStream::connect(&target_addr).await {
        Ok(t) => t,
        Err(e) => {
            let frame = fc.encode(&[0x02]); // 0x02 = connect failure
            let _ = stream.write_all(&frame).await;
            bail!("Cannot connect to {target_addr}: {e}");
        }
    };

    // Signal success to the client
    let ok_frame = fc.encode(&[0x00]); // 0x00 = success
    stream.write_all(&ok_frame).await?;
    stream.flush().await?;

    info!("Proxy: → {target_addr}");
    state.push_log(format!("Proxy connected → {target_addr}"));

    // ── Step 6: Bidirectional relay ───────────────────────────────────────────
    relay_encrypted(stream, target, fc, state).await
}

// ── Frame I/O helpers ─────────────────────────────────────────────────────────

/// Read a single encrypted [`FramedCrypto`] frame from a TCP stream.
///
/// Reads the 2-byte length header, then the nonce and ciphertext, then
/// decrypts.  Returns the plaintext bytes or an error if the MAC check fails.
async fn read_frame(stream: &mut TcpStream, fc: &FramedCrypto) -> Result<Vec<u8>> {
    // 2-byte big-endian ciphertext length
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let ct_len = u16::from_be_bytes(len_buf) as usize;

    // 12-byte nonce + ciphertext
    let mut raw = vec![0u8; 12 + ct_len];
    stream.read_exact(&mut raw).await?;

    // Reconstruct the full frame buffer that FramedCrypto::decode expects:
    // [2B len] [12B nonce] [ct_len B ciphertext]
    let mut buf = Vec::with_capacity(2 + 12 + ct_len);
    buf.extend_from_slice(&len_buf);
    buf.extend_from_slice(&raw);

    match fc.decode(&buf) {
        Some((plain, _)) => Ok(plain),
        None => bail!("Frame decryption failed"),
    }
}

// ── Target address parser ─────────────────────────────────────────────────────

/// Parse the target address from the connect-header payload (after the 16-byte
/// auth token).
///
/// Address encoding (SOCKS5-style):
/// * `0x01` — IPv4: 4 bytes address + 2 bytes port
/// * `0x03` — Hostname: 1 byte length + N bytes name + 2 bytes port
/// * `0x04` — IPv6: 16 bytes address + 2 bytes port
///
/// Returns `(host_string, port)`.
fn parse_target(data: &[u8]) -> Result<(String, u16)> {
    if data.is_empty() {
        bail!("Empty target");
    }
    let addr_type = data[0];

    let (host, port_offset) = match addr_type {
        // IPv4 — 4 bytes
        1 => {
            if data.len() < 7 {
                bail!("IPv4 too short");
            }
            let ip = format!("{}.{}.{}.{}", data[1], data[2], data[3], data[4]);
            (ip, 5)
        }
        // Hostname — length-prefixed
        3 => {
            if data.len() < 2 {
                bail!("Hostname too short");
            }
            let hlen = data[1] as usize;
            if data.len() < 2 + hlen + 2 {
                bail!("Hostname data too short");
            }
            let host = String::from_utf8_lossy(&data[2..2 + hlen]).to_string();
            (host, 2 + hlen)
        }
        // IPv6 — 16 bytes
        4 => {
            if data.len() < 19 {
                bail!("IPv6 too short");
            }
            let bytes: Vec<u8> = data[1..17].to_vec();
            let mut addr = [0u16; 8];
            for i in 0..8 {
                addr[i] = u16::from_be_bytes([bytes[i * 2], bytes[i * 2 + 1]]);
            }
            let ip = format!(
                "{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}:{:x}",
                addr[0], addr[1], addr[2], addr[3],
                addr[4], addr[5], addr[6], addr[7]
            );
            (ip, 17)
        }
        _ => bail!("Unknown addr_type {addr_type}"),
    };

    if data.len() < port_offset + 2 {
        bail!("Missing port");
    }
    let port = u16::from_be_bytes([data[port_offset], data[port_offset + 1]]);
    Ok((host, port))
}

// ── Bidirectional relay ───────────────────────────────────────────────────────

/// Relay data between the encrypted client side and the plaintext target.
///
/// Two independent tasks run concurrently:
/// * **client → target**: reads FramedCrypto frames from the client, decrypts
///   them, and writes the plaintext to the upstream target.
/// * **target → client**: reads plaintext from the upstream target, encrypts
///   each chunk as a FramedCrypto frame, and sends it to the client.
///
/// Both tasks run until either side closes the connection; then `tokio::join!`
/// waits for both to finish before returning.
async fn relay_encrypted(
    client: TcpStream,
    target: TcpStream,
    fc: FramedCrypto,
    state: Shared,
) -> Result<()> {
    use std::sync::Arc;
    let fc = Arc::new(fc);

    // Split both sockets into owned halves so they can be sent to separate tasks
    let (mut client_rx, mut client_tx) = client.into_split();
    let (mut target_rx, mut target_tx) = target.into_split();

    let state1 = state.clone();
    let fc1 = fc.clone();

    // Task 1: client → target (decrypt incoming frames, write raw to target)
    let t1 = tokio::spawn(async move {
        let mut frame_buf: Vec<u8> = Vec::new();
        let mut tmp = vec![0u8; 65536];
        loop {
            let n = match client_rx.read(&mut tmp).await {
                Ok(0) | Err(_) => break, // client disconnected
                Ok(n) => n,
            };
            frame_buf.extend_from_slice(&tmp[..n]);

            // Drain all complete frames from the accumulation buffer
            loop {
                match fc1.decode(&frame_buf) {
                    Some((plain, consumed)) => {
                        state1.total_bytes_in.fetch_add(plain.len() as u64, Ordering::Relaxed);
                        if target_tx.write_all(&plain).await.is_err() {
                            return; // target disconnected
                        }
                        frame_buf.drain(..consumed); // remove processed bytes
                    }
                    None => break, // incomplete frame — wait for more data
                }
            }
        }
    });

    // Task 2: target → client (read raw from target, encrypt, write frames to client)
    let t2 = tokio::spawn(async move {
        let mut tmp = vec![0u8; 65535];
        loop {
            let n = match target_rx.read(&mut tmp).await {
                Ok(0) | Err(_) => break, // target disconnected
                Ok(n) => n,
            };
            let frame = fc.encode(&tmp[..n]);
            state.total_bytes_out.fetch_add(n as u64, Ordering::Relaxed);
            if client_tx.write_all(&frame).await.is_err() {
                break; // client disconnected
            }
        }
    });

    // Wait for both directions to finish
    let _ = tokio::join!(t1, t2);
    Ok(())
}
