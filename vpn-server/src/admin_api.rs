//! Admin-only HTTP API handlers.
//!
//! All endpoints except the Telegram OTP flow require an admin JWT
//! (obtained via the two-step OTP process below).
//!
//! | Method | Path | Auth | Description |
//! |--------|------|------|-------------|
//! | POST | `/admin/request-code`    | — | Send OTP to admin Telegram chat |
//! | POST | `/admin/verify-code`     | — | Verify OTP, receive admin JWT |
//! | POST | `/admin/promos`          | Admin JWT | Create a promo code |
//! | GET  | `/admin/users`           | Admin JWT | List all users |
//! | PUT  | `/admin/users/:id/limit` | Admin JWT | Set user's speed limit |
//! | GET  | `/admin/peers`           | Admin JWT | List live connected peers |
//!
//! ## Admin login flow
//! ```text
//! Admin  POST /admin/request-code  →  Server generates OTP, sends via Telegram
//! Admin  POST /admin/verify-code   →  Server validates OTP, returns admin JWT
//! Admin  uses JWT for all /admin/* requests
//! ```

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

/// Convenience type alias for admin handler results.
type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

/// Construct an error tuple from a status code and message string.
fn err(code: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (code, msg.into())
}

// ── Admin auth via Telegram OTP ───────────────────────────────────────────────

/// `POST /admin/request-code` — generate a 6-digit OTP and deliver it via Telegram.
///
/// No authentication required (this is the first step of the login flow).
/// Requires `TG_BOT_TOKEN` and `TG_ADMIN_CHAT_ID` to be configured.
///
/// The OTP is stored in the database with a 5-minute TTL and sent as a
/// Markdown-formatted Telegram message to the configured admin chat.
pub async fn request_code(State(s): State<Shared>) -> ApiResult<serde_json::Value> {
    // Check that Telegram is configured before doing anything
    let Some(ref bot_token) = s.tg_bot_token else {
        return Err(err(
            StatusCode::SERVICE_UNAVAILABLE,
            "Telegram not configured (set TG_BOT_TOKEN + TG_ADMIN_CHAT_ID)",
        ));
    };
    let Some(ref chat_id) = s.tg_admin_chat_id else {
        return Err(err(StatusCode::SERVICE_UNAVAILABLE, "TG_ADMIN_CHAT_ID not set"));
    };

    // Generate a random 6-digit numeric OTP (000000–999999)
    let code: String = rand::thread_rng()
        .sample_iter(&rand::distributions::Uniform::new(0, 10))
        .take(6)
        .map(|d: u8| d.to_string())
        .collect();

    // Persist the code with a 5-minute TTL
    db::create_admin_code(&s.pool, &code)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Deliver via Telegram Bot API (Markdown formatting, monospace code)
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

/// `POST /admin/verify-code` — validate an OTP and return an admin JWT.
///
/// No authentication required (this is the second step of the login flow).
/// The OTP is consumed atomically — the same code cannot be used twice.
/// Returns `401 Unauthorized` for invalid, expired or already-used codes.
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

    // Issue an admin JWT (sub=0 is reserved for admin sessions)
    let token = make_token(0, "admin", &s.jwt_secret);
    info!("Admin authenticated via Telegram OTP");

    Ok(Json(serde_json::json!({ "token": token })))
}

// ── Promo management ──────────────────────────────────────────────────────────

/// `POST /admin/promos` — create a new promo code (admin only).
///
/// Validates the type, computes the expiry from `expires_days`, and inserts
/// the code into the database.  Returns `409 Conflict` if the code string
/// already exists.
///
/// # Promo type reference
/// | Type | `value` | `extra` |
/// |------|---------|---------|
/// | `balance`   | RUB amount to credit | — |
/// | `discount`  | % discount on next purchase | — |
/// | `free_days` | Days of free VPN | — |
/// | `speed`     | Speed cap (Mbit/s) | Days active |
pub async fn create_promo(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
    Json(req): Json<CreatePromoRequest>,
) -> ApiResult<serde_json::Value> {
    // Validate the promo type against the allowed set
    let allowed_types = ["balance", "discount", "free_days", "speed"];
    if !allowed_types.contains(&req.r#type.as_str()) {
        return Err(err(
            StatusCode::BAD_REQUEST,
            "type must be one of: balance, discount, free_days, speed",
        ));
    }

    // Compute optional absolute expiry from "days from now"
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
        // Translate UNIQUE constraint violation into a meaningful 409
        if e.to_string().contains("unique") || e.to_string().contains("duplicate") {
            err(StatusCode::CONFLICT, "Promo code already exists")
        } else {
            err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        }
    })?;

    info!("Admin created promo: {} (type={})", promo.code, promo.r#type);
    s.push_log(format!(
        "Promo created: {} type={} value={}",
        promo.code, promo.r#type, promo.value
    ));

    Ok(Json(serde_json::to_value(&promo).unwrap()))
}

// ── User management ───────────────────────────────────────────────────────────

/// `GET /admin/users` — list all registered users (admin only).
///
/// Returns all user rows including balance, subscription state and VPN IP.
/// Password hashes are never included (skipped by `#[serde(skip)]` on [`User`]).
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

/// `PUT /admin/users/:id/limit` — set a user's bandwidth cap (admin only).
///
/// Updates both the database (`sub_speed_mbps`) and the live in-memory peer
/// limit (if the user is currently connected) so the change takes effect
/// immediately without requiring the user to reconnect.
pub async fn set_user_limit(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
    axum::extract::Path(user_id): axum::extract::Path<i32>,
    Json(req): Json<SetLimitRequest>,
) -> ApiResult<serde_json::Value> {
    // Persist to DB so the limit survives reconnects
    db::set_user_limit(&s.pool, user_id, req.limit_mbps)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Also update the live peer's token bucket if they are currently connected
    if let Some(user) = db::find_user_by_id(&s.pool, user_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    {
        if let Some(ref ip_str) = user.vpn_ip {
            if let Ok(vpn_ip) = ip_str.parse::<std::net::Ipv4Addr>() {
                if let Some(peer) = s.peers.get(&vpn_ip) {
                    use std::sync::atomic::Ordering;
                    // Convert Mbit/s → bytes/s; 0 = unlimited
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

/// `GET /admin/promos/list` — list all promo codes (admin only).
pub async fn list_promos(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
) -> ApiResult<serde_json::Value> {
    let promos = sqlx::query_as::<_, crate::models::PromoCode>(
        "SELECT id, code, \"type\", value, extra, max_uses, used_count, expires_at, created_at \
         FROM promo_codes ORDER BY created_at DESC",
    )
    .fetch_all(&s.pool)
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({ "promos": promos })))
}

/// `DELETE /admin/promos/:id` — delete a promo code (admin only).
pub async fn delete_promo(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
    axum::extract::Path(promo_id): axum::extract::Path<i32>,
) -> ApiResult<serde_json::Value> {
    sqlx::query("DELETE FROM promo_codes WHERE id = $1")
        .bind(promo_id)
        .execute(&s.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!("Admin deleted promo {}", promo_id);
    Ok(Json(serde_json::json!({ "deleted": promo_id })))
}

/// `PUT /admin/users/:id/ban` — suspend or unsuspend a user (admin only).
pub async fn ban_user(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
    axum::extract::Path(user_id): axum::extract::Path<i32>,
    Json(body): Json<serde_json::Value>,
) -> ApiResult<serde_json::Value> {
    let ban = body["ban"].as_bool().unwrap_or(true);
    let role = if ban { "banned" } else { "user" };

    sqlx::query("UPDATE users SET role = $1 WHERE id = $2")
        .bind(role)
        .bind(user_id)
        .execute(&s.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!("Admin {} user {}", if ban { "banned" } else { "unbanned" }, user_id);
    s.push_log(format!("User {} {}", user_id, if ban { "banned" } else { "unbanned" }));

    Ok(Json(serde_json::json!({ "user_id": user_id, "role": role })))
}

/// `GET /admin/peers` — list all currently connected VPN peers (admin only).
///
/// Equivalent to the public `GET /api/peers` but includes additional fields
/// and requires an admin JWT.  Provides a real-time snapshot of all active
/// connections with traffic statistics.
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
                "vpn_ip":        p.vpn_ip.to_string(),
                "endpoint":      p.endpoint.blocking_read().map(|ep| ep.to_string()),
                "bytes_in":      p.bytes_in.load(Ordering::Relaxed),
                "bytes_out":     p.bytes_out.load(Ordering::Relaxed),
                "speed_in_bps":  p.speed_in_bps.load(Ordering::Relaxed),
                "speed_out_bps": p.speed_out_bps.load(Ordering::Relaxed),
                "limit_bps":     p.limit_bps.load(Ordering::Relaxed),
                "connected_secs": p.connected_at.elapsed().as_secs(),
            })
        })
        .collect();

    let total = peers.len();
    Ok(Json(serde_json::json!({ "peers": peers, "total": total })))
}
