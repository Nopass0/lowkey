//! Referral system and withdrawal endpoints.
//!
//! | Method | Path | Auth | Description |
//! |--------|------|------|-------------|
//! | GET  | `/referral/stats`              | JWT | Get referral stats and balance |
//! | GET  | `/referral/withdrawals`        | JWT | List user withdrawal requests |
//! | POST | `/referral/withdraw`           | JWT | Request a referral payout |
//! | GET  | `/admin/referral/withdrawals`  | Admin | List all withdrawal requests |
//! | PUT  | `/admin/referral/withdrawals/:id/approve` | Admin | Approve withdrawal |
//! | PUT  | `/admin/referral/withdrawals/:id/reject`  | Admin | Reject withdrawal |

use axum::{extract::{Path, State}, http::StatusCode, Json};
use tracing::info;

use crate::{
    auth_middleware::{AdminUser, AuthUser},
    db,
    models::WithdrawRequest,
    state::Shared,
};

type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

fn err(code: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (code, msg.into())
}

// ── User referral endpoints ───────────────────────────────────────────────────

/// `GET /referral/stats` — get current user's referral stats and balance.
pub async fn referral_stats(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
) -> ApiResult<serde_json::Value> {
    let stats = db::get_referral_stats(&s.pool, claims.sub)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(stats))
}

/// `POST /referral/withdraw` — request a withdrawal of referral earnings.
pub async fn request_withdrawal(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
    Json(req): Json<WithdrawRequest>,
) -> ApiResult<serde_json::Value> {
    if req.amount < 100.0 {
        return Err(err(StatusCode::BAD_REQUEST, "Minimum withdrawal is 100 RUB"));
    }
    if req.card_number.len() < 4 {
        return Err(err(StatusCode::BAD_REQUEST, "Invalid card number"));
    }

    let withdrawal = db::create_withdrawal(
        &s.pool,
        claims.sub,
        req.amount,
        &req.card_number,
        req.bank_name.as_deref(),
    )
    .await
    .map_err(|e| {
        if e.to_string().contains("Insufficient") {
            err(StatusCode::PAYMENT_REQUIRED, "Insufficient referral balance")
        } else {
            err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        }
    })?;

    info!("User {} requested withdrawal of {:.2} RUB to card {}", claims.sub, req.amount, req.card_number);
    s.push_log(format!("Withdrawal request #{}: {:.2} RUB from user {}", withdrawal.id, req.amount, claims.sub));

    Ok(Json(serde_json::json!({
        "withdrawal_id": withdrawal.id,
        "status": "pending",
        "amount": withdrawal.amount,
        "message": "Заявка на вывод принята. Администратор обработает её в течение 24 часов."
    })))
}

/// `GET /referral/withdrawals` — list current user's withdrawal requests.
pub async fn list_withdrawals(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
) -> ApiResult<serde_json::Value> {
    let withdrawals = db::list_user_withdrawals(&s.pool, claims.sub)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "withdrawals": withdrawals })))
}

// ── Admin withdrawal management ───────────────────────────────────────────────

/// `GET /admin/referral/withdrawals` — list all withdrawal requests (admin).
pub async fn admin_list_withdrawals(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
) -> ApiResult<serde_json::Value> {
    let withdrawals = db::list_all_withdrawals(&s.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "withdrawals": withdrawals, "total": withdrawals.len() })))
}

/// `PUT /admin/referral/withdrawals/:id/approve` — approve a withdrawal.
/// Triggers a real payout via Tochka API if configured.
pub async fn admin_approve_withdrawal(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
    Path(withdrawal_id): Path<i32>,
    Json(body): Json<serde_json::Value>,
) -> ApiResult<serde_json::Value> {
    let admin_note = body["note"].as_str();

    // Try Tochka payout API if configured
    let tochka_payout_id = if let Some(jwt) = &s.tochka_jwt {
        if !jwt.is_empty() {
            match initiate_tochka_payout(&s, withdrawal_id, jwt).await {
                Ok(payout_id) => {
                    info!("Tochka payout initiated: {payout_id}");
                    Some(payout_id)
                }
                Err(e) => {
                    tracing::warn!("Tochka payout failed: {e}. Marking approved without payout ID.");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };

    db::update_withdrawal_status(
        &s.pool,
        withdrawal_id,
        "completed",
        admin_note,
        tochka_payout_id.as_deref(),
    )
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!("Admin approved withdrawal #{}", withdrawal_id);
    s.push_log(format!("Withdrawal #{} approved", withdrawal_id));

    Ok(Json(serde_json::json!({
        "withdrawal_id": withdrawal_id,
        "status": "completed",
        "tochka_payout_id": tochka_payout_id,
    })))
}

/// `PUT /admin/referral/withdrawals/:id/reject` — reject a withdrawal (refunds balance).
pub async fn admin_reject_withdrawal(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
    Path(withdrawal_id): Path<i32>,
    Json(body): Json<serde_json::Value>,
) -> ApiResult<serde_json::Value> {
    let admin_note = body["note"].as_str();

    // Refund the referral balance
    db::refund_withdrawal(&s.pool, withdrawal_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    db::update_withdrawal_status(&s.pool, withdrawal_id, "rejected", admin_note, None)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!("Admin rejected withdrawal #{}", withdrawal_id);
    s.push_log(format!("Withdrawal #{} rejected, balance refunded", withdrawal_id));

    Ok(Json(serde_json::json!({
        "withdrawal_id": withdrawal_id,
        "status": "rejected",
        "message": "Баланс реферальных средств возвращён"
    })))
}

// ── Tochka payout ─────────────────────────────────────────────────────────────

/// Initiate a card payout via Tochka Bank API.
async fn initiate_tochka_payout(
    s: &Shared,
    withdrawal_id: i32,
    jwt: &str,
) -> anyhow::Result<String> {
    // Fetch withdrawal details
    let withdrawals = db::list_all_withdrawals(&s.pool).await?;
    let w = withdrawals
        .iter()
        .find(|w| w.id == withdrawal_id)
        .ok_or(anyhow::anyhow!("Withdrawal not found"))?;

    let amount_kopecks = (w.amount * 100.0).round() as u64;

    let body = serde_json::json!({
        "Data": {
            "merchantId": s.tochka_merchant_id.as_deref().unwrap_or(""),
            "legalEntityId": s.tochka_legal_id.as_deref().unwrap_or(""),
            "amount": amount_kopecks,
            "currency": "RUB",
            "cardNumber": w.card_number,
            "description": format!("Lowkey VPN реферальная выплата #{}", withdrawal_id),
            "orderId": format!("withdraw-{}", withdrawal_id),
        }
    });

    let client = reqwest::Client::new();
    let resp = client
        .post("https://enter.tochka.com/uapi/v1.0/payment/payout")
        .header("Authorization", format!("Bearer {}", jwt))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?;

    let status = resp.status();
    let data: serde_json::Value = resp.json().await?;

    if !status.is_success() {
        return Err(anyhow::anyhow!("Tochka payout error: {:?}", data));
    }

    let payout_id = data["Data"]["payoutId"]
        .as_str()
        .unwrap_or(&format!("local-{}", withdrawal_id))
        .to_string();

    Ok(payout_id)
}

// ── Admin stats endpoint ──────────────────────────────────────────────────────

/// `GET /admin/stats` — get financial summary (admin only).
pub async fn admin_stats(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
) -> ApiResult<serde_json::Value> {
    let stats = db::get_admin_stats(&s.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(stats))
}

/// `GET /admin/plans` — list subscription plans.
pub async fn admin_list_plans(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
) -> ApiResult<serde_json::Value> {
    let plans = db::list_db_plans(&s.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Json(serde_json::json!({ "plans": plans })))
}

/// `PUT /admin/plans/:key/price` — update plan price.
pub async fn admin_update_plan_price(
    State(s): State<Shared>,
    AdminUser(_): AdminUser,
    Path(plan_key): Path<String>,
    Json(body): Json<serde_json::Value>,
) -> ApiResult<serde_json::Value> {
    let price = body["price_rub"]
        .as_f64()
        .ok_or(err(StatusCode::BAD_REQUEST, "price_rub required"))?;

    if price < 1.0 {
        return Err(err(StatusCode::BAD_REQUEST, "Price must be at least 1 RUB"));
    }

    db::update_plan_price(&s.pool, &plan_key, price)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!("Admin updated plan {} price to {:.2}", plan_key, price);

    Ok(Json(serde_json::json!({
        "plan_key": plan_key,
        "price_rub": price,
        "status": "updated"
    })))
}
