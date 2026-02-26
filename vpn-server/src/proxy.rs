//! TCP proxy — VLESS-style encrypted proxy for Windows/any-platform clients.
//!
//! Protocol:
//!   1. Client  → Server : [32 B: client X25519 ephemeral pubkey]
//!   2. Server  → Client : [32 B: server X25519 pubkey]
//!   3. Derive shared key K = X25519 + HKDF (FramedCrypto)
//!   4. Client  → Server : encrypted connect-header
//!        [16 B: SHA256(psk)[0..16]]
//!        [1 B: addr_type: 1=IPv4, 3=hostname, 4=IPv6]
//!        [N B: address]
//!        [2 B: port BE]
//!   5. Server  → Client : encrypted status [1 B: 0x00=ok, 0x01=auth, 0x02=conn]
//!   6. Bidirectional relay with framed encryption:
//!        [2 B: len BE] [12 B: nonce] [len B: ciphertext]

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

async fn handle_proxy_conn(mut stream: TcpStream, state: Shared) -> Result<()> {
    // ── Handshake ─────────────────────────────────────────────────────────────

    // 1. Receive client ephemeral public key
    let mut client_pub_bytes = [0u8; 32];
    stream.read_exact(&mut client_pub_bytes).await?;

    // 2. Send server public key
    stream.write_all(&state.server_pubkey).await?;
    stream.flush().await?;

    // 3. Derive framed crypto
    let server_secret = StaticSecret::from(state.server_secret);
    let client_pub = PublicKey::from(client_pub_bytes);
    let fc = FramedCrypto::new(&server_secret, &client_pub);

    // 4. Read connect header (one encrypted frame)
    let connect_hdr = read_frame(&mut stream, &fc).await?;
    if connect_hdr.len() < 20 {
        bail!("Connect header too short");
    }

    // Verify auth token
    let expected_token = psk_auth_token(&state.psk);
    if connect_hdr[..16] != expected_token {
        let frame = fc.encode(&[0x01]); // auth fail
        let _ = stream.write_all(&frame).await;
        bail!("Auth token mismatch");
    }

    // Parse target address
    let (target_host, target_port) = parse_target(&connect_hdr[16..])?;
    let target_addr = format!("{target_host}:{target_port}");

    // 5. Connect to target
    let target = match TcpStream::connect(&target_addr).await {
        Ok(t) => t,
        Err(e) => {
            let frame = fc.encode(&[0x02]); // connect fail
            let _ = stream.write_all(&frame).await;
            bail!("Cannot connect to {target_addr}: {e}");
        }
    };

    // Send success
    let ok_frame = fc.encode(&[0x00]);
    stream.write_all(&ok_frame).await?;
    stream.flush().await?;

    info!("Proxy: → {target_addr}");
    state.push_log(format!("Proxy connected → {target_addr}"));

    // 6. Bidirectional relay with framed encryption
    relay_encrypted(stream, target, fc, state).await
}

/// Read a single encrypted frame from the stream.
async fn read_frame(stream: &mut TcpStream, fc: &FramedCrypto) -> Result<Vec<u8>> {
    // Frame header: 2B len
    let mut len_buf = [0u8; 2];
    stream.read_exact(&mut len_buf).await?;
    let ct_len = u16::from_be_bytes(len_buf) as usize;
    // Read nonce (12B) + ciphertext
    let mut raw = vec![0u8; 12 + ct_len];
    stream.read_exact(&mut raw).await?;

    // Reconstruct the frame buffer the FramedCrypto expects: [2B][12B][ctlen]
    let mut buf = Vec::with_capacity(2 + 12 + ct_len);
    buf.extend_from_slice(&len_buf);
    buf.extend_from_slice(&raw);

    match fc.decode(&buf) {
        Some((plain, _)) => Ok(plain),
        None => bail!("Frame decryption failed"),
    }
}

fn parse_target(data: &[u8]) -> Result<(String, u16)> {
    if data.is_empty() {
        bail!("Empty target");
    }
    let addr_type = data[0];
    let (host, port_offset) = match addr_type {
        1 => {
            // IPv4
            if data.len() < 7 {
                bail!("IPv4 too short");
            }
            let ip = format!("{}.{}.{}.{}", data[1], data[2], data[3], data[4]);
            (ip, 5)
        }
        3 => {
            // Hostname
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
        4 => {
            // IPv6
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

/// Relay data between `client` (encrypted framed) and `target` (raw TCP).
async fn relay_encrypted(
    client: TcpStream,
    target: TcpStream,
    fc: FramedCrypto,
    state: Shared,
) -> Result<()> {
    use std::sync::Arc;
    let fc = Arc::new(fc);

    let (mut client_rx, mut client_tx) = client.into_split();
    let (mut target_rx, mut target_tx) = target.into_split();

    let state1 = state.clone();
    let fc1 = fc.clone();

    // client → target (decrypt incoming frames, write raw to target)
    let t1 = tokio::spawn(async move {
        let mut frame_buf: Vec<u8> = Vec::new();
        let mut tmp = vec![0u8; 65536];
        loop {
            let n = match client_rx.read(&mut tmp).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            frame_buf.extend_from_slice(&tmp[..n]);

            // Drain complete frames
            loop {
                match fc1.decode(&frame_buf) {
                    Some((plain, consumed)) => {
                        state1.total_bytes_in.fetch_add(plain.len() as u64, Ordering::Relaxed);
                        if target_tx.write_all(&plain).await.is_err() {
                            return;
                        }
                        frame_buf.drain(..consumed);
                    }
                    None => break,
                }
            }
        }
    });

    // target → client (read raw from target, encrypt, write frames to client)
    let t2 = tokio::spawn(async move {
        let mut tmp = vec![0u8; 65535];
        loop {
            let n = match target_rx.read(&mut tmp).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            let frame = fc.encode(&tmp[..n]);
            state.total_bytes_out.fetch_add(n as u64, Ordering::Relaxed);
            if client_tx.write_all(&frame).await.is_err() {
                break;
            }
        }
    });

    let _ = tokio::join!(t1, t2);
    Ok(())
}
