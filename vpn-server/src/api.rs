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

pub async fn api_list_peers(State(s): State<Shared>) -> Json<Vec<PeerInfo>> {
    let mut out = Vec::new();
    for entry in s.peers.iter() {
        let p = entry.value();
        out.push(PeerInfo {
            vpn_ip: p.vpn_ip.to_string(),
            endpoint: p.endpoint.read().await.map(|e| e.to_string()).unwrap_or_else(|| "pending".into()),
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

/// Register a VPN peer. Requires a valid user token + active subscription.
pub async fn api_register(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
    Json(req): Json<RegisterRequest>,
) -> Result<Json<RegisterResponse>, (StatusCode, String)> {
    // Verify the legacy PSK field (kept for backward compat if present)
    if !req.psk.is_empty() && req.psk != s.psk {
        return Err((StatusCode::UNAUTHORIZED, "Invalid PSK".into()));
    }

    // Check user subscription
    let user = db::find_user_by_id(&s.pool, claims.sub)
        .await
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or((StatusCode::NOT_FOUND, "User not found".into()))?;

    // Admins bypass subscription check
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

    let pub_bytes = from_hex(&req.public_key)
        .filter(|b| b.len() == 32)
        .ok_or((StatusCode::BAD_REQUEST, "Invalid public key".into()))?;
    let mut pub_arr = [0u8; 32];
    pub_arr.copy_from_slice(&pub_bytes);

    let server_secret = StaticSecret::from(s.server_secret);
    let client_pub = PublicKey::from(pub_arr);
    let shared = server_secret.diffie_hellman(&client_pub);
    let crypto = vpn_common::VpnCrypto::from_shared_secret(&shared);

    // Reuse the same VPN IP if user already has one assigned
    let vpn_ip = if let Some(ref ip_str) = user.vpn_ip {
        if let Ok(ip) = ip_str.parse::<Ipv4Addr>() {
            ip
        } else {
            assign_new_ip(&s).await?
        }
    } else {
        assign_new_ip(&s).await?
    };

    // Store/update peer
    let speed_bps = if user.sub_speed_mbps > 0.0 {
        (user.sub_speed_mbps * 1_000_000.0 / 8.0) as u64
    } else {
        0
    };
    let peer = Peer::new(vpn_ip, crypto, Some(claims.sub));
    peer.limit_bps.store(speed_bps, Ordering::Relaxed);
    s.peers.insert(vpn_ip, peer);

    // Persist assigned IP to DB
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

async fn assign_new_ip(s: &Shared) -> Result<Ipv4Addr, (StatusCode, String)> {
    let octet = {
        let mut next = s.next_octet.lock().await;
        let o = *next;
        *next = if *next >= 254 { 2 } else { *next + 1 };
        o
    };
    Ok(Ipv4Addr::new(10, 0, 0, octet))
}

pub async fn api_remove_peer(
    State(s): State<Shared>,
    Path(ip): Path<String>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let vpn_ip: Ipv4Addr = ip.parse().map_err(|_| (StatusCode::BAD_REQUEST, "Invalid IP".into()))?;
    match s.peers.remove(&vpn_ip) {
        Some((_, peer)) => {
            if let Some(ep) = *peer.endpoint.read().await {
                s.endpoints.remove(&ep);
            }
            Ok(Json(serde_json::json!({ "status": "removed" })))
        }
        None => Err((StatusCode::NOT_FOUND, "Peer not found".into())),
    }
}

pub async fn api_set_limit(
    State(s): State<Shared>,
    Path(ip): Path<String>,
    Json(req): Json<LimitRequest>,
) -> Result<Json<serde_json::Value>, (StatusCode, String)> {
    let vpn_ip: Ipv4Addr = ip.parse().map_err(|_| (StatusCode::BAD_REQUEST, "Invalid IP".into()))?;
    let peer = s.peers.get(&vpn_ip).ok_or((StatusCode::NOT_FOUND, "Peer not found".into()))?;
    let bps = (req.limit_mbps * 1_000_000.0 / 8.0) as u64;
    peer.limit_bps.store(bps, Ordering::Relaxed);
    Ok(Json(serde_json::json!({ "limit_mbps": req.limit_mbps })))
}
