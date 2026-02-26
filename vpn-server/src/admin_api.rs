use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use rand::Rng;
use tracing::info;

use crate::{
    auth_middleware::{make_token, AdminUser},
    db,
    models::{AdminVerifyRequest, CreatePromoRequest, SetLimitRequest},
    state::Shared,
    telegram,
};

type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

fn err(code: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (code, msg.into())
}

// ── Admin auth via Telegram OTP ───────────────────────────────────────────────

/// POST /admin/request-code
/// Generates a 6-digit OTP, stores it, sends it to the configured Telegram chat.
pub async fn request_code(State(s): State<Shared>) -> ApiResult<serde_json::Value> {
    let Some(ref bot_token) = s.tg_bot_token else {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "Telegram not configured (set TG_BOT_TOKEN + TG_ADMIN_CHAT_ID)"));
    };
    let Some(ref chat_id) = s.tg_admin_chat_id else {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "TG_ADMIN_CHAT_ID not set"));
    };

    let code: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Uniform::new(0, 10))
        .take(6)
        .map(|d| d.to_string())
        .collect();

    db::create_admin_code(&s.pool, &code)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let text = format!(
        "🔐 *Lowkey VPN Admin*\n\nВаш код входа: `{}`\nДействует 5 минут.",
        code
    );

    telegram::send_message(bot_token, chat_id, &text)
        .await
        .map_err(|e| err(StatusCode::BAD_GATEWAY, format!("Telegram error: {e}")))?;

    info!("Admin login code sent via Telegram");

    Ok(Json(serde_json::json!({
        "status": "Code sent to Telegram",
        "expires_in_seconds": 300
    })))
}

/// POST /admin/verify-code  { "code": "123456" }
/// Validates OTP and returns an admin JWT.
pub async fn verify_code(
    State(s): State<Shared>,
    Json(req): Json<AdminVerifyRequest>,
) -> ApiResult<serde_json::Value> {
    let valid = db::verify_admin_code(&s.pool, &req.code)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    if !valid {
        return Err(err(StatusCode::UNAUTHORIZED, "Invalid or expired code"));
    }

    // Issue an admin token (sub=0 is reserved for admin sessions)
    let token = make_token(0, "admin", &s.jwt_secret);
    info!("Admin authenticated via Telegram OTP");

    Ok(Json(serde_json::json!({ "token": token })))
}

// ── Promo management ──────────────────────────────────────────────────────────

/// POST /admin/promos
pub async fn create_promo(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
    Json(req): Json<CreatePromoRequest>,
) -> ApiResult<serde_json::Value> {
    let allowed_types = ["balance", "discount", "free_days", "speed"];
    if !allowed_types.contains(&req.r#type.as_str()) {
        return Err(err(StatusCode::BAD_REQUEST, "type must be one of: balance, discount, free_days, speed"));
    }

    let expires_at = req.expires_days.map(|d| Utc::now() + chrono::Duration::days(d));
    let extra = req.extra.unwrap_or(0.0);
    let max_uses = req.max_uses.unwrap_or(1);

    let promo = db::create_promo(
        &s.pool,
        &req.code,
        &req.r#type,
        req.value,
        extra,
        max_uses,
        expires_at,
    )
    .await
    .map_err(|e| {
        if e.to_string().contains("unique") {
            err(StatusCode::CONFLICT, "Promo code already exists")
        } else {
            err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        }
    })?;

    info!("Admin created promo: {} (type={})", promo.code, promo.r#type);
    s.push_log(format!("Promo created: {} type={} value={}", promo.code, promo.r#type, promo.value));

    Ok(Json(serde_json::to_value(&promo).unwrap()))
}

// ── User management ───────────────────────────────────────────────────────────

/// GET /admin/users
pub async fn list_users(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
) -> ApiResult<serde_json::Value> {
    let users = db::list_users(&s.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let total = users.len();
    Ok(Json(serde_json::json!({ "users": users, "total": total })))
}

/// PUT /admin/users/:id/limit  { "limit_mbps": 10.0 }
pub async fn set_user_limit(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
    axum::extract::Path(user_id): axum::extract::Path<i32>,
    Json(req): Json<SetLimitRequest>,
) -> ApiResult<serde_json::Value> {
    db::set_user_limit(&s.pool, user_id, req.limit_mbps)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Also update live VPN peer speed if they are connected
    if let Some(user) = db::find_user_by_id(&s.pool, user_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    {
        if let Some(ref ip_str) = user.vpn_ip {
            if let Ok(vpn_ip) = ip_str.parse::<std::net::Ipv4Addr>() {
                if let Some(peer) = s.peers.get(&vpn_ip) {
                    use std::sync::atomic::Ordering;
                    let bps = (req.limit_mbps * 1_000_000.0 / 8.0) as u64;
                    peer.limit_bps.store(bps, Ordering::Relaxed);
                }
            }
        }
    }

    info!("Admin set user {} limit to {:.1} Mbps", user_id, req.limit_mbps);
    s.push_log(format!("User {} speed limit → {:.1} Mbps", user_id, req.limit_mbps));

    Ok(Json(serde_json::json!({
        "user_id": user_id,
        "limit_mbps": req.limit_mbps
    })))
}

/// GET /admin/peers  — live connected VPN peers
pub async fn list_peers(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
) -> ApiResult<serde_json::Value> {
    use std::sync::atomic::Ordering;
    let peers: Vec<_> = s
        .peers
        .iter()
        .map(|e| {
            let p = e.value();
            serde_json::json!({
                "vpn_ip": p.vpn_ip.to_string(),
                "endpoint": p.endpoint.blocking_read().map(|ep| ep.to_string()),
                "bytes_in": p.bytes_in.load(Ordering::Relaxed),
                "bytes_out": p.bytes_out.load(Ordering::Relaxed),
                "speed_in_bps": p.speed_in_bps.load(Ordering::Relaxed),
                "speed_out_bps": p.speed_out_bps.load(Ordering::Relaxed),
                "limit_bps": p.limit_bps.load(Ordering::Relaxed),
                "connected_secs": p.connected_at.elapsed().as_secs(),
            })
        })
        .collect();

    let total = peers.len();
    Ok(Json(serde_json::json!({ "peers": peers, "total": total })))
}
