//! VPN peer management API handlers.
//!
//! These endpoints handle the VPN-specific lifecycle:
//!
//! | Method | Path | Auth | Description |
//! |--------|------|------|-------------|
//! | GET  | `/api/status`          | — (public) | Server status & stats |
//! | GET  | `/api/peers`           | — (public) | List connected peers |
//! | POST | `/api/peers/register`  | JWT + subscription | Register a new VPN peer |
//! | DELETE | `/api/peers/:ip`     | — | Disconnect a peer |
//! | PUT  | `/api/peers/:ip/limit` | — | Set per-peer bandwidth cap |
//!
//! # Registration flow
//! ```text
//! Client                           Server
//!   |                                |
//!   |--- POST /api/peers/register -->|  (JWT + client X25519 pubkey)
//!   |                                |-- DH handshake
//!   |                                |-- assign VPN IP (10.0.0.x)
//!   |<-- RegisterResponse -----------|  (server pubkey, VPN IP, ports)
//!   |                                |
//!   |=== encrypted UDP tunnel ======>|  (ChaCha20-Poly1305)
//! ```

use std::{net::Ipv4Addr, sync::atomic::Ordering};

use axum::{
    extract::{Path, State},
    http::StatusCode,
    Json,
};
use chrono::Utc;
use tracing::info;
use vpn_common::{
    from_hex, to_hex, LimitRequest, PeerInfo, RegisterRequest, RegisterResponse, StatusResponse,
    VPN_SUBNET_CIDR,
};
use x25519_dalek::{PublicKey, StaticSecret};

use crate::{
    auth_middleware::AuthUser,
    db,
    state::{Peer, Shared},
};

// ── Server status ─────────────────────────────────────────────────────────────

/// `GET /api/status` — public server status endpoint.
///
/// Returns runtime information that clients can use to verify connectivity
/// and display to the user (uptime, peer count, transferred bytes).
pub async fn api_status(State(s): State<Shared>) -> Json<StatusResponse> {
    Json(StatusResponse {
        running: true,
        peer_count: s.peers.len(),
        server_vpn_ip: vpn_common::VPN_SERVER_IP.to_string(),
        public_ip: s.public_ip.read().await.clone(),
        udp_port: s.udp_port,
        proxy_port: s.proxy_port,
        uptime_secs: s.uptime_secs(),
        total_bytes_in: s.total_in(),
        total_bytes_out: s.total_out(),
    })
}

// ── Peer listing ──────────────────────────────────────────────────────────────

/// `GET /api/peers` — list all currently connected VPN peers.
///
/// Returns a JSON array of [`PeerInfo`] objects with live traffic statistics.
/// This endpoint is public (no auth) and is polled by the dashboard and
/// external monitoring tools.
pub async fn api_list_peers(State(s): State<Shared>) -> Json<Vec<PeerInfo>> {
    let mut out = Vec::new();
    for entry in s.peers.iter() {
        let p = entry.value();
        out.push(PeerInfo {
            vpn_ip: p.vpn_ip.to_string(),
            endpoint: p.endpoint.read().await
                .map(|e| e.to_string())
                .unwrap_or_else(|| "pending".into()),
            bytes_in: p.bytes_in(),
            bytes_out: p.bytes_out(),
            speed_in_bps: p.speed_in(),
            speed_out_bps: p.speed_out(),
            limit_bps: p.limit(),
            connected_secs: p.connected_at.elapsed().as_secs(),
        });
    }
    Json(out)
}

// ── Peer registration ─────────────────────────────────────────────────────────

/// `POST /api/peers/register` — register a new VPN peer (requires JWT + active subscription).
///
/// ## Steps
/// 1. Validate the JWT and verify the user has an active subscription
///    (admins bypass the subscription check).
/// 2. Parse the client's X25519 ephemeral public key from the request.
/// 3. Perform an X25519 DH key exchange with the server's static private key
///    to derive the per-session ChaCha20-Poly1305 key.
/// 4. Assign a VPN IP (`10.0.0.x`) — reusing the user's last IP if available.
/// 5. Apply the user's subscription speed limit to the new peer.
/// 6. Persist the assigned VPN IP to the database.
///
/// Returns the server's public key (for the client to complete DH on its side),
/// the assigned VPN IP, and the tunnel/proxy ports.
pub async fn api_register(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, (StatusCode, String)> {
    // Optional legacy PSK check (empty string bypasses it for JWT clients)
    if !req.psk.is_empty() && req.psk != s.psk {
        return Err((StatusCode::UNAUTHORIZED, "Invalid PSK".into()));
    }

    // Verify the user exists and has an active subscription
    let user = db::find_user_by_id(&s.pool, claims.sub)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "User not found".into()))?;

    // Admins always have VPN access regardless of subscription
    if user.role != "admin" {
        let active = user.sub_status == "active"
            && user.sub_expires_at.map(|e| e > Utc::now()).unwrap_or(false);
        if !active {
            return Err((
                StatusCode::PAYMENT_REQUIRED,
                "No active subscription. Buy one at POST /subscription/buy".into(),
            ));
        }
    }

    // Parse the client's 32-byte X25519 public key from hex
    let pub_bytes = from_hex(&req.public_key)
        .filter(|b| b.len() == 32)
        .ok_or((StatusCode::BAD_REQUEST, "Invalid public key".into()))?;
    let mut pub_arr = [0u8; 32];
    pub_arr.copy_from_slice(&pub_bytes);

    // X25519 DH → HKDF → ChaCha20-Poly1305 session key
    let server_secret = StaticSecret::from(s.server_secret);
    let client_pub = PublicKey::from(pub_arr);
    let shared = server_secret.diffie_hellman(&client_pub);
    let crypto = vpn_common::VpnCrypto::from_shared_secret(&shared);

    // Reuse the persisted VPN IP if available; otherwise allocate a new one
    let vpn_ip = if let Some(ref ip_str) = user.vpn_ip {
        if let Ok(ip) = ip_str.parse::<Ipv4Addr>() {
            ip
        } else {
            assign_new_ip(&s).await?
        }
    } else {
        assign_new_ip(&s).await?
    };

    // Convert subscription speed (Mbit/s) to bytes/s for the rate limiter
    let speed_bps = if user.sub_speed_mbps > 0.0 {
        (user.sub_speed_mbps * 1_000_000.0 / 8.0) as u64
    } else {
        0 // unlimited
    };
    let peer = Peer::new(vpn_ip, crypto, Some(claims.sub));
    peer.limit_bps.store(speed_bps, Ordering::Relaxed);
    s.peers.insert(vpn_ip, peer);

    // Persist the assigned IP so the same address is reused on reconnect
    let _ = db::update_user_vpn_ip(&s.pool, claims.sub, &vpn_ip.to_string()).await;

    info!("VPN peer registered: user={} vpn_ip={}", claims.sub, vpn_ip);
    s.push_log(format!("Peer up: user={} → {}", user.login, vpn_ip));

    Ok(Json(RegisterResponse {
        server_public_key: to_hex(&s.server_pubkey),
        assigned_ip: vpn_ip.to_string(),
        udp_port: s.udp_port,
        proxy_port: s.proxy_port,
        subnet: VPN_SUBNET_CIDR.to_string(),
    }))
}

/// Allocate the next available VPN IP in the `10.0.0.2–10.0.0.254` range.
///
/// The `next_octet` counter wraps back to `2` after `254` (`.1` is the server
/// gateway and `.255` is broadcast).  This does not check for collisions with
/// currently connected peers — with 253 available slots the probability is
/// negligible for typical deployments.
async fn assign_new_ip(s: &Shared) -> Result<Ipv4Addr, (StatusCode, String)> {
    let octet = {
        let mut next = s.next_octet.lock().await;
        let o = *next;
        *next = if *next >= 254 { 2 } else { *next + 1 };
        o
    };
    Ok(Ipv4Addr::new(10, 0, 0, octet))
}

// ── Peer management ───────────────────────────────────────────────────────────

/// `DELETE /api/peers/:ip` — forcefully disconnect a peer.
///
/// Removes the peer from the in-memory state.  The client will notice that
/// its encrypted packets are no longer accepted and will disconnect.
pub async fn api_remove_peer(
    State(s): State<Shared>,
    Path(ip): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let vpn_ip: Ipv4Addr = ip.parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid IP".into()))?;

    match s.peers.remove(&vpn_ip) {
        Some((_, peer)) => {
            // Also remove the reverse endpoint→IP mapping
            if let Some(ep) = *peer.endpoint.read().await {
                s.endpoints.remove(&ep);
            }
            Ok(Json(serde_json::json!({ "status": "removed" })))
        }
        None => Err((StatusCode::NOT_FOUND, "Peer not found".into())),
    }
}

/// `PUT /api/peers/:ip/limit` — set a live bandwidth cap for a specific peer.
///
/// The change takes effect immediately for the next packet.  Setting
/// `limit_mbps = 0` removes the cap (unlimited mode).
pub async fn api_set_limit(
    State(s): State<Shared>,
    Path(ip): Path<String>,
    Json(req): Json<LimitRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let vpn_ip: Ipv4Addr = ip.parse()
        .map_err(|_| (StatusCode::BAD_REQUEST, "Invalid IP".into()))?;
    let peer = s.peers.get(&vpn_ip)
        .ok_or((StatusCode::NOT_FOUND, "Peer not found".into()))?;

    // Convert Mbit/s → bytes/s and store atomically
    let bps = (req.limit_mbps * 1_000_000.0 / 8.0) as u64;
    peer.limit_bps.store(bps, Ordering::Relaxed);

    Ok(Json(serde_json::json!({ "limit_mbps": req.limit_mbps })))
}
