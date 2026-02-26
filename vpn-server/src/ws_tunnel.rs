//! WebSocket VPN tunnel — transport layer for clients behind firewalls.
//!
//! # Why WebSocket?
//! Raw UDP (port 51820) is blocked by many corporate firewalls, ISPs and
//! hotel/airport networks.  A WebSocket upgrade on port 8080 (the same port
//! as the HTTP API) looks identical to regular browser traffic and bypasses
//! virtually all deep-packet-inspection filters.
//!
//! # Protocol
//! ```text
//! GET /ws-tunnel?token=<JWT>
//! Upgrade: websocket
//!
//! ── Handshake ────────────────────────────────────────────────────────────────
//! Client → Server  binary frame:  [32 B: client X25519 ephemeral pubkey]
//! Server → Client  binary frame:  [32 B: server pubkey]  [4 B: VPN IP (BE)]
//!
//! ── Data phase (bidirectional binary frames) ──────────────────────────────
//! [12 B: ChaCha20 nonce]  [N B: ciphertext + 16 B AEAD tag]
//! ```
//!
//! The session key is derived identically to the UDP tunnel:
//! `X25519 DH  →  HKDF-SHA256  →  ChaCha20-Poly1305`.
//!
//! There is **no 4-byte VPN-IP prefix** on data frames: the persistent
//! WebSocket connection unambiguously identifies the peer.

use std::{net::Ipv4Addr, sync::atomic::Ordering};

use axum::{
    extract::{
        ws::{Message, WebSocket, WebSocketUpgrade},
        Query, State,
    },
    http::StatusCode,
    response::IntoResponse,
};
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use jsonwebtoken::{decode, DecodingKey, Validation};
use serde::Deserialize;
use tokio::sync::mpsc;
use tracing::{info, warn};
use vpn_common::VPN_SUBNET_OCTETS;
use x25519_dalek::{PublicKey, StaticSecret};

use crate::{db, models::Claims, state::{Peer, Shared}};

// ── Query parameters ──────────────────────────────────────────────────────────

/// Query parameters accepted on `GET /ws-tunnel`.
#[derive(Deserialize)]
pub struct WsQuery {
    /// The JWT token (same one used in `Authorization: Bearer` headers).
    pub token: String,
}

// ── Axum handler ──────────────────────────────────────────────────────────────

/// `GET /ws-tunnel?token=<JWT>` — WebSocket VPN tunnel endpoint.
///
/// Validates the JWT *before* accepting the WebSocket upgrade so that
/// unauthenticated clients receive a plain HTTP 401 rather than a WebSocket
/// close frame.
pub async fn ws_handler(
    ws: WebSocketUpgrade,
    Query(q): Query<WsQuery>,
    State(state): State<Shared>,
) -> impl IntoResponse {
    match decode_jwt(&q.token, &state.jwt_secret) {
        Some(claims) => ws
            .on_upgrade(move |socket| handle_ws_connection(socket, state, claims))
            .into_response(),
        None => StatusCode::UNAUTHORIZED.into_response(),
    }
}

// ── Session handler ───────────────────────────────────────────────────────────

/// Handle one authenticated WebSocket VPN session end-to-end.
///
/// # Steps
/// 1. Receive client's ephemeral X25519 public key (first binary frame).
/// 2. Verify the user's subscription.
/// 3. Perform DH key exchange and assign a VPN IP.
/// 4. Send back the server's public key + assigned VPN IP.
/// 5. Spawn bidirectional relay tasks:
///    - *WS → TUN*: decrypt frames and push plaintext packets to the TUN
///      inject channel so the OS kernel routes them.
///    - *TUN → WS*: receive pre-encrypted packets from [`ServerState::ws_peers`]
///      and forward them as binary WebSocket frames to the client.
/// 6. Clean up the peer entry on disconnect.
async fn handle_ws_connection(mut socket: WebSocket, state: Shared, claims: Claims) {
    // ── 1. Receive client pubkey ──────────────────────────────────────────────
    let client_pub_bytes: Vec<u8> = match socket.recv().await {
        Some(Ok(Message::Binary(b))) if b.len() == 32 => b.to_vec(),
        other => {
            warn!("WS: unexpected handshake frame from user {}: {:?}", claims.sub, other);
            return;
        }
    };

    // ── 2. Subscription check ─────────────────────────────────────────────────
    let user = match db::find_user_by_id(&state.pool, claims.sub).await {
        Ok(Some(u)) => u,
        _ => {
            warn!("WS: user {} not found in DB", claims.sub);
            return;
        }
    };

    if user.role != "admin" {
        let active = user.sub_status == "active"
            && user.sub_expires_at.map(|e| e > Utc::now()).unwrap_or(false);
        if !active {
            warn!("WS: user {} has no active subscription", claims.sub);
            let _ = socket
                .send(Message::Close(Some(axum::extract::ws::CloseFrame {
                    code: 4003,
                    reason: "No active subscription".into(),
                })))
                .await;
            return;
        }
    }

    // ── 3. Derive session key and assign VPN IP ───────────────────────────────
    let vpn_ip = assign_ip(&state).await;

    let mut pub_arr = [0u8; 32];
    pub_arr.copy_from_slice(&client_pub_bytes);
    let shared = StaticSecret::from(state.server_secret)
        .diffie_hellman(&PublicKey::from(pub_arr));
    let crypto = vpn_common::VpnCrypto::from_shared_secret(&shared);

    // ── 4. Send server pubkey + VPN IP ────────────────────────────────────────
    let mut handshake = Vec::with_capacity(36);
    handshake.extend_from_slice(&state.server_pubkey);
    handshake.extend_from_slice(&vpn_ip.octets());
    if socket.send(Message::Binary(handshake.into())).await.is_err() {
        return;
    }

    // ── 5. Register peer ──────────────────────────────────────────────────────
    let speed_bps = if user.sub_speed_mbps > 0.0 {
        (user.sub_speed_mbps * 1_000_000.0 / 8.0) as u64
    } else {
        0 // unlimited
    };
    let peer = Peer::new(vpn_ip, crypto, Some(claims.sub));
    peer.limit_bps.store(speed_bps, Ordering::Relaxed);
    state.peers.insert(vpn_ip, peer.clone());

    // Channel: task_tun_to_peer → this WS send loop
    let (ws_out_tx, mut ws_out_rx) = mpsc::unbounded_channel::<Vec<u8>>();
    state.ws_peers.insert(vpn_ip, ws_out_tx);

    let _ = db::update_user_vpn_ip(&state.pool, claims.sub, &vpn_ip.to_string()).await;
    info!("WS peer connected: user={} vpn_ip={}", claims.sub, vpn_ip);
    state.push_log(format!("WS peer up: user={} → {}", user.login, vpn_ip));

    // ── 6. Bidirectional relay ────────────────────────────────────────────────
    let (mut ws_sink, mut ws_stream) = socket.split();

    // TUN → WS: forward pre-encrypted packets received from the tunnel task
    let send_task = tokio::spawn(async move {
        while let Some(enc) = ws_out_rx.recv().await {
            if ws_sink.send(Message::Binary(enc.into())).await.is_err() {
                break;
            }
        }
    });

    // WS → TUN: decrypt incoming frames and inject into TUN device
    let tun_inject = state.tun_inject.clone();
    let state2 = state.clone();
    let recv_task = tokio::spawn(async move {
        while let Some(msg) = ws_stream.next().await {
            match msg {
                Ok(Message::Binary(data)) => {
                    match peer.crypto.decrypt(&data) {
                        Some(plain) if plain != b"hello" => {
                            peer.bytes_in
                                .fetch_add(plain.len() as u64, Ordering::Relaxed);
                            state2
                                .total_bytes_in
                                .fetch_add(plain.len() as u64, Ordering::Relaxed);
                            let _ = tun_inject.send(plain);
                        }
                        Some(_) => {} // keepalive probe
                        None => warn!("WS: decryption failed (peer {})", peer.vpn_ip),
                    }
                }
                Ok(Message::Ping(_)) | Ok(Message::Pong(_)) => {}
                Ok(Message::Close(_)) | Err(_) => break,
                _ => {}
            }
        }
    });

    // Block until one side disconnects
    tokio::select! {
        _ = send_task => {}
        _ = recv_task => {}
    }

    // ── Cleanup ───────────────────────────────────────────────────────────────
    state.ws_peers.remove(&vpn_ip);
    state.peers.remove(&vpn_ip);
    info!("WS peer disconnected: vpn_ip={}", vpn_ip);
    state.push_log(format!("WS peer down: {}", vpn_ip));
}

// ── Helpers ───────────────────────────────────────────────────────────────────

/// Allocate the next available VPN IP in the 10.66.0.2–10.66.0.254 range.
async fn assign_ip(s: &Shared) -> Ipv4Addr {
    let octet = {
        let mut next = s.next_octet.lock().await;
        let o = *next;
        *next = if *next >= 254 { 2 } else { *next + 1 };
        o
    };
    Ipv4Addr::new(
        VPN_SUBNET_OCTETS[0],
        VPN_SUBNET_OCTETS[1],
        VPN_SUBNET_OCTETS[2],
        octet,
    )
}

/// Decode and validate a JWT string, returning its claims on success.
fn decode_jwt(token: &str, secret: &str) -> Option<Claims> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .ok()
    .map(|td| td.claims)
}
