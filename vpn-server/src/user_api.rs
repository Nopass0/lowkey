use axum::{extract::State, http::StatusCode, Json};
use chrono::Utc;
use tracing::info;

use crate::{
    auth_middleware::{make_token, AuthUser},
    db,
    models::{
        ApplyPromoRequest, ApplyPromoResponse, AuthResponse, BuySubscriptionRequest,
        LoginRequest, RegisterRequest, UserPublic, PLANS,
    },
    state::Shared,
};

type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

fn err(code: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (code, msg.into())
}

// ── Auth ──────────────────────────────────────────────────────────────────────

pub async fn register(
    State(s): State<Shared>,
    Json(req): Json<RegisterRequest>,
) -> ApiResult<AuthResponse> {
    let login = req.login.trim();
    if login.len() < 3 || login.len() > 50 {
        return Err(err(StatusCode::BAD_REQUEST, "Login must be 3–50 chars"));
    }
    if req.password.len() < 6 {
        return Err(err(StatusCode::BAD_REQUEST, "Password must be ≥ 6 chars"));
    }

    // Check duplicate
    if db::find_user_by_login(&s.pool, login).await.map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?.is_some() {
        return Err(err(StatusCode::CONFLICT, "Login already taken"));
    }

    let hash = hash_password(&req.password)?;
    let user = db::create_user(&s.pool, login, &hash)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!("Registered user: {}", user.login);
    s.push_log(format!("New user registered: {}", user.login));

    let token = make_token(user.id, &user.role, &s.jwt_secret);
    Ok(Json(AuthResponse {
        token,
        user: user.into(),
    }))
}

pub async fn login(
    State(s): State<Shared>,
    Json(req): Json<LoginRequest>,
) -> ApiResult<AuthResponse> {
    let user = db::find_user_by_login(&s.pool, &req.login)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or(err(StatusCode::UNAUTHORIZED, "Invalid login or password"))?;

    if !verify_password(&req.password, &user.password_hash)? {
        return Err(err(StatusCode::UNAUTHORIZED, "Invalid login or password"));
    }

    let token = make_token(user.id, &user.role, &s.jwt_secret);
    info!("User logged in: {}", user.login);

    Ok(Json(AuthResponse {
        token,
        user: user.into(),
    }))
}

pub async fn me(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
) -> ApiResult<UserPublic> {
    let user = db::find_user_by_id(&s.pool, claims.sub)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or(err(StatusCode::NOT_FOUND, "User not found"))?;
    Ok(Json(user.into()))
}

// ── Subscription ──────────────────────────────────────────────────────────────

pub async fn list_plans() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "plans": PLANS }))
}

pub async fn buy_subscription(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
    Json(req): Json<BuySubscriptionRequest>,
) -> ApiResult<serde_json::Value> {
    let plan = PLANS
        .iter()
        .find(|p| p.id == req.plan_id)
        .ok_or(err(StatusCode::BAD_REQUEST, "Unknown plan"))?;

    let user = db::find_user_by_id(&s.pool, claims.sub)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or(err(StatusCode::NOT_FOUND, "User not found"))?;

    if user.balance < plan.price_rub {
        return Err(err(
            StatusCode::PAYMENT_REQUIRED,
            format!(
                "Insufficient balance: {:.2} RUB needed, {:.2} RUB available",
                plan.price_rub, user.balance
            ),
        ));
    }

    let expires_at = db::activate_subscription(
        &s.pool,
        claims.sub,
        plan.id,
        plan.price_rub,
        plan.speed_mbps,
        plan.duration_days,
    )
    .await
    .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!("User {} bought plan {}", user.login, plan.id);
    s.push_log(format!("User {} → plan {} until {}", user.login, plan.id, expires_at.format("%Y-%m-%d")));

    Ok(Json(serde_json::json!({
        "status": "ok",
        "plan": plan.name,
        "expires_at": expires_at,
        "balance_after": user.balance - plan.price_rub,
    })))
}

pub async fn subscription_status(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
) -> ApiResult<serde_json::Value> {
    let user = db::find_user_by_id(&s.pool, claims.sub)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or(err(StatusCode::NOT_FOUND, "User not found"))?;

    // Auto-expire check
    let status = if let Some(exp) = user.sub_expires_at {
        if exp < Utc::now() { "expired" } else { &user.sub_status }
    } else {
        &user.sub_status
    };

    Ok(Json(serde_json::json!({
        "status": status,
        "expires_at": user.sub_expires_at,
        "speed_mbps": user.sub_speed_mbps,
        "balance": user.balance,
    })))
}

// ── Promo codes ───────────────────────────────────────────────────────────────

pub async fn apply_promo(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
    Json(req): Json<ApplyPromoRequest>,
) -> ApiResult<ApplyPromoResponse> {
    let promo = db::find_promo(&s.pool, &req.code)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or(err(StatusCode::NOT_FOUND, "Promo code not found"))?;

    // Validate
    if let Some(exp) = promo.expires_at {
        if exp < Utc::now() {
            return Err(err(StatusCode::GONE, "Promo code expired"));
        }
    }
    if promo.used_count >= promo.max_uses {
        return Err(err(StatusCode::GONE, "Promo code already fully used"));
    }

    if db::has_user_used_promo(&s.pool, claims.sub, promo.id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    {
        return Err(err(StatusCode::CONFLICT, "You already used this promo code"));
    }

    // Apply effects
    let (new_balance, new_expires) = db::apply_promo_effects(&s.pool, claims.sub, &promo)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    db::record_promo_use(&s.pool, claims.sub, promo.id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    let msg = match promo.r#type.as_str() {
        "balance"   => format!("Начислено {:.2} ₽", promo.value),
        "free_days" => format!("Добавлено {} дней VPN", promo.value as i64),
        "speed"     => format!("Активирован VPN {:.0} Мбит/с на {} дней", promo.value, promo.extra as i64),
        "discount"  => format!("Скидка {:.0}% на следующую подписку", promo.value),
        _           => "Промокод применён".into(),
    };

    s.push_log(format!("Promo '{}' applied by user {}", promo.code, claims.sub));

    Ok(Json(ApplyPromoResponse {
        message: msg,
        new_balance,
        sub_expires_at: new_expires,
    }))
}

// ── Password helpers ──────────────────────────────────────────────────────────

fn hash_password(password: &str) -> Result<String, (StatusCode, String)> {
    use argon2::{
        password_hash::{rand_core::OsRng, PasswordHasher, SaltString},
        Argon2,
    };
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .map(|h| h.to_string())
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

fn verify_password(
    password: &str,
    hash: &str,
) -> Result<bool, (StatusCode, String)> {
    use argon2::{
        password_hash::{PasswordHash, PasswordVerifier},
        Argon2,
    };
    let parsed = PasswordHash::new(hash)
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;
    Ok(Argon2::default()
        .verify_password(password.as_bytes(), &parsed)
        .is_ok())
}
