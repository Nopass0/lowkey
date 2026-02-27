//! SBP payment endpoints using Tochka Bank API.
//!
//! | Method | Path | Auth | Description |
//! |--------|------|------|-------------|
//! | POST | `/payment/sbp/create`       | JWT | Create SBP QR payment order |
//! | GET  | `/payment/sbp/status/:id`   | JWT | Poll payment status |
//! | POST | `/payment/webhook`          | — | Tochka webhook (payment confirmed) |
//! | GET  | `/payment/history`          | JWT | List user payment history |

use axum::{extract::{Path, State}, http::StatusCode, Json};
use chrono::{Duration, Utc};
use serde::{Deserialize, Serialize};
use tracing::info;

use crate::{
    auth_middleware::AuthUser,
    db,
    models::{CreatePaymentRequest, CreatePaymentResponse, PaymentStatusResponse},
    state::Shared,
};

type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

fn err(code: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (code, msg.into())
}

// ── Create SBP payment ────────────────────────────────────────────────────────

/// `POST /payment/sbp/create`
///
/// Creates a SBP QR payment order via Tochka Bank API.
/// Returns a QR payload string that clients render as a QR code.
pub async fn create_sbp_payment(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
    Json(req): Json<CreatePaymentRequest>,
) -> ApiResult<CreatePaymentResponse> {
    if req.amount < 10.0 {
        return Err(err(StatusCode::BAD_REQUEST, "Minimum payment is 10 RUB"));
    }
    if req.amount > 100_000.0 {
        return Err(err(StatusCode::BAD_REQUEST, "Maximum payment is 100,000 RUB"));
    }
    if req.purpose != "balance" && req.purpose != "subscription" {
        return Err(err(StatusCode::BAD_REQUEST, "purpose must be 'balance' or 'subscription'"));
    }
    if req.purpose == "subscription" && req.plan_id.is_none() {
        return Err(err(StatusCode::BAD_REQUEST, "plan_id required for subscription purpose"));
    }

    // Check for first-purchase discount (referred user)
    let has_discount = db::has_first_purchase_discount(&s.pool, claims.sub)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let mut actual_amount = req.amount;
    if has_discount && req.purpose == "subscription" {
        actual_amount = req.amount * 0.5; // 50% off first purchase
    }

    // Create payment record (pending)
    let expires_at = Utc::now() + Duration::minutes(30);
    let payment = db::create_payment(
        &s.pool,
        claims.sub,
        None,
        actual_amount,
        &req.purpose,
        req.plan_id.as_deref(),
        None,
        None,
        Some(expires_at),
    )
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Try Tochka Bank API
    let (qr_payload, qr_url, tochka_order_id) = match &s.tochka_jwt {
        Some(jwt) if !jwt.is_empty() => {
            match call_tochka_sbp(&s, payment.id, actual_amount, jwt).await {
                Ok(result) => result,
                Err(e) => {
                    // Fallback to simple SBP link if Tochka API fails
                    tracing::warn!("Tochka API error: {e}. Using fallback SBP link.");
                    let payload = generate_sbp_fallback(actual_amount, payment.id);
                    (payload, None, None)
                }
            }
        }
        _ => {
            // No Tochka credentials — use fallback SBP deep link
            let payload = generate_sbp_fallback(actual_amount, payment.id);
            (payload, None, None)
        }
    };

    // Update payment with Tochka order info
    if let Some(ref order_id) = tochka_order_id {
        db::update_payment_tochka(
            &s.pool,
            payment.id,
            order_id,
            &qr_payload,
            qr_url.as_deref(),
            Some(expires_at),
        )
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    } else {
        // Update just the QR payload without a Tochka order ID
        db::update_payment_tochka(
            &s.pool,
            payment.id,
            &format!("local-{}", payment.id),
            &qr_payload,
            qr_url.as_deref(),
            Some(expires_at),
        )
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    }

    let discount_note = if has_discount && req.purpose == "subscription" {
        " (50% скидка для новых клиентов)"
    } else {
        ""
    };
    info!("Created SBP payment {} for user {} amount={:.2}{}", payment.id, claims.sub, actual_amount, discount_note);
    s.push_log(format!("SBP payment #{} created: {:.2} RUB (user {})", payment.id, actual_amount, claims.sub));

    Ok(Json(CreatePaymentResponse {
        payment_id: payment.id,
        qr_payload,
        qr_url,
        amount: actual_amount,
        expires_at: Some(expires_at),
    }))
}

/// Call Tochka Bank SBP API to create a payment order.
/// Returns (qr_payload, qr_url, order_id).
async fn call_tochka_sbp(
    s: &Shared,
    payment_id: i32,
    amount: f64,
    jwt: &str,
) -> anyhow::Result<(String, Option<String>, Option<String>)> {
    let merchant_id = s.tochka_merchant_id.as_deref().unwrap_or("");
    let legal_id = s.tochka_legal_id.as_deref().unwrap_or("");

    // Amount in kopecks (integer)
    let amount_kopecks = (amount * 100.0).round() as u64;

    let order_id = format!("lowkey-{}-{}", payment_id, Utc::now().timestamp());

    let body = serde_json::json!({
        "Data": {
            "merchantId": merchant_id,
            "legalEntityId": legal_id,
            "orderId": order_id,
            "amount": amount_kopecks,
            "currency": "RUB",
            "description": format!("Lowkey VPN — оплата #{}", payment_id),
            "paymentType": "QR",
            "redirectUrl": ""
        }
    });

    let client = reqwest::Client::new();
    let resp = client
        .post("https://enter.tochka.com/uapi/sbp/v1.0/qr-code/dynamic")
        .header("Authorization", format!("Bearer {}", jwt))
        .header("Content-Type", "application/json")
        .json(&body)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?;

    if !resp.status().is_success() {
        let status = resp.status();
        let text = resp.text().await.unwrap_or_default();
        return Err(anyhow::anyhow!("Tochka API error {}: {}", status, text));
    }

    let data: serde_json::Value = resp.json().await?;

    let qr_payload = data["Data"]["payload"]
        .as_str()
        .unwrap_or("")
        .to_string();
    let qr_url = data["Data"]["qrcUrl"]
        .as_str()
        .map(|s| s.to_string());
    let tochka_order_id = data["Data"]["orderId"]
        .as_str()
        .map(|s| s.to_string())
        .unwrap_or(order_id);

    Ok((qr_payload, qr_url, Some(tochka_order_id)))
}

/// Generate a fallback SBP payment link (without Tochka API).
/// Uses SBP URI scheme that works with most Russian banking apps.
fn generate_sbp_fallback(amount: f64, payment_id: i32) -> String {
    // Simple SBP link format — apps can parse this
    format!(
        "https://qr.nspk.ru/pay?amount={:.0}&purpose=LowkeyVPN%20%23{}&currency=RUB",
        amount * 100.0,
        payment_id
    )
}

// ── Poll payment status ────────────────────────────────────────────────────────

/// `GET /payment/sbp/status/:id`
///
/// Polls the payment status. If Tochka is configured, re-checks with their API.
/// Long-polls for up to 30s in practice (clients should poll every 2–3s).
pub async fn get_payment_status(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
    Path(payment_id): Path<i32>,
) -> ApiResult<PaymentStatusResponse> {
    let payment = db::get_payment(&s.pool, payment_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or(err(StatusCode::NOT_FOUND, "Payment not found"))?;

    // Security: only own payments
    if payment.user_id != claims.sub {
        return Err(err(StatusCode::FORBIDDEN, "Access denied"));
    }

    // If still pending, check with Tochka
    let mut status = payment.status.clone();
    let mut paid_at = payment.paid_at;

    if status == "pending" {
        // Check if expired
        if let Some(exp) = payment.expires_at {
            if exp < Utc::now() {
                sqlx::query("UPDATE payments SET status = 'expired' WHERE id = $1")
                    .bind(payment_id)
                    .execute(&s.pool)
                    .await
                    .ok();
                status = "expired".to_string();
            }
        }

        // Check with Tochka API if we have credentials and order ID
        if status == "pending" {
            if let (Some(jwt), Some(ref order_id)) = (&s.tochka_jwt, &payment.tochka_order_id) {
                if !jwt.is_empty() && !order_id.starts_with("local-") {
                    match check_tochka_payment_status(jwt, order_id).await {
                        Ok(paid) if paid => {
                            // Mark as paid and credit user
                            match db::mark_payment_paid(&s.pool, &payment).await {
                                Ok((balance, sub_exp)) => {
                                    status = "paid".to_string();
                                    paid_at = Some(Utc::now());
                                    info!("Payment {} confirmed via Tochka API", payment_id);
                                    s.push_log(format!("Payment #{} paid: {:.2} RUB", payment_id, payment.amount));

                                    return Ok(Json(PaymentStatusResponse {
                                        payment_id,
                                        status,
                                        amount: payment.amount,
                                        paid_at,
                                        balance_after: Some(balance),
                                        sub_expires_at: sub_exp,
                                    }));
                                }
                                Err(e) => tracing::error!("Failed to mark payment paid: {e}"),
                            }
                        }
                        _ => {}
                    }
                }
            }
        }
    }

    // If already paid, get current user state
    let (balance_after, sub_expires_at) = if status == "paid" {
        let user = db::find_user_by_id(&s.pool, claims.sub).await
            .ok().flatten();
        (
            user.as_ref().map(|u| u.balance),
            user.as_ref().and_then(|u| u.sub_expires_at),
        )
    } else {
        (None, None)
    };

    Ok(Json(PaymentStatusResponse {
        payment_id,
        status,
        amount: payment.amount,
        paid_at,
        balance_after,
        sub_expires_at,
    }))
}

/// Check payment status with Tochka API.
async fn check_tochka_payment_status(jwt: &str, order_id: &str) -> anyhow::Result<bool> {
    let client = reqwest::Client::new();
    let url = format!(
        "https://enter.tochka.com/uapi/sbp/v1.0/qr-code/dynamic/{}",
        order_id
    );
    let resp = client
        .get(&url)
        .header("Authorization", format!("Bearer {}", jwt))
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await?;

    let data: serde_json::Value = resp.json().await?;
    let status = data["Data"]["status"].as_str().unwrap_or("");
    Ok(status == "PAID" || status == "ACSC")
}

// ── Tochka webhook ────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct TochkaWebhook {
    #[serde(rename = "Data")]
    pub data: TochkaWebhookData,
}

#[derive(Debug, Deserialize)]
pub struct TochkaWebhookData {
    #[serde(rename = "orderId")]
    pub order_id: String,
    pub status: String,
}

/// `POST /payment/webhook` — Tochka Bank payment notification.
pub async fn payment_webhook(
    State(s): State<Shared>,
    Json(webhook): Json<TochkaWebhook>,
) -> (StatusCode, String) {
    let order_id = &webhook.data.order_id;
    let status = &webhook.data.status;

    if status == "PAID" || status == "ACSC" {
        match db::get_payment_by_tochka_id(&s.pool, order_id).await {
            Ok(Some(payment)) if payment.status == "pending" => {
                match db::mark_payment_paid(&s.pool, &payment).await {
                    Ok(_) => {
                        info!("Webhook: payment {} marked paid", payment.id);
                        s.push_log(format!("Webhook: payment #{} paid", payment.id));
                    }
                    Err(e) => tracing::error!("Webhook: failed to mark payment paid: {e}"),
                }
            }
            Ok(_) => {}
            Err(e) => tracing::error!("Webhook: DB error: {e}"),
        }
    }

    (StatusCode::OK, "ok".to_string())
}

// ── Manual payment confirmation (admin) ───────────────────────────────────────

/// `POST /admin/payment/:id/confirm` — manually confirm a payment (admin only).
pub async fn admin_confirm_payment(
    State(s): State<Shared>,
    crate::auth_middleware::AdminUser(_): crate::auth_middleware::AdminUser,
    Path(payment_id): Path<i32>,
) -> ApiResult<serde_json::Value> {
    let payment = db::get_payment(&s.pool, payment_id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or(err(StatusCode::NOT_FOUND, "Payment not found"))?;

    if payment.status != "pending" {
        return Err(err(StatusCode::CONFLICT, format!("Payment already {}", payment.status)));
    }

    let (balance, sub_exp) = db::mark_payment_paid(&s.pool, &payment)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!("Admin manually confirmed payment {}", payment_id);
    s.push_log(format!("Admin confirmed payment #{}", payment_id));

    Ok(Json(serde_json::json!({
        "payment_id": payment_id,
        "status": "paid",
        "balance_after": balance,
        "sub_expires_at": sub_exp,
    })))
}

// ── Payment history ───────────────────────────────────────────────────────────

/// `GET /payment/history` — list current user's payment history.
pub async fn payment_history(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
) -> ApiResult<serde_json::Value> {
    let payments = db::list_user_payments(&s.pool, claims.sub)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({ "payments": payments })))
}

/// `GET /admin/payments` — list all payments (admin only).
pub async fn admin_list_payments(
    State(s): State<Shared>,
    crate::auth_middleware::AdminUser(_): crate::auth_middleware::AdminUser,
) -> ApiResult<serde_json::Value> {
    let payments = db::list_all_payments(&s.pool)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    Ok(Json(serde_json::json!({ "payments": payments, "total": payments.len() })))
}
