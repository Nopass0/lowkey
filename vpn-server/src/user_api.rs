//! User-facing HTTP API handlers.
//!
//! | Method | Path | Auth | Description |
//! |--------|------|------|-------------|
//! | POST | `/auth/register`        | — | Create account, return JWT |
//! | POST | `/auth/login`           | — | Authenticate, return JWT |
//! | GET  | `/auth/me`              | JWT | Return current user profile |
//! | GET  | `/subscription/plans`   | — | List available plans |
//! | POST | `/subscription/buy`     | JWT | Purchase a subscription |
//! | GET  | `/subscription/status`  | JWT | Current subscription state |
//! | POST | `/promo/apply`          | JWT | Redeem a promo code |
//!
//! All handlers return `(StatusCode, String)` on error so axum can
//! serialise the message as the response body.

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

/// Convenience type alias for API handler results.
type ApiResult<T> = Result<Json<T>, (StatusCode, String)>;

/// Construct an error tuple from a status code and message string.
fn err(code: StatusCode, msg: impl Into<String>) -> (StatusCode, String) {
    (code, msg.into())
}

// ── Auth ──────────────────────────────────────────────────────────────────────

/// `POST /auth/register` — create a new user account.
///
/// Validates the login/password constraints, hashes the password with
/// Argon2id, creates the user row, and returns a JWT for immediate use.
///
/// # Validation rules
/// * Login: 3–50 characters.
/// * Password: minimum 6 characters.
/// * Login must be unique (returns `409 Conflict` otherwise).
pub async fn register(
    State(s): State<Shared>,
    Json(req): Json<RegisterRequest>,
) -> ApiResult<AuthResponse> {
    let login = req.login.trim();

    // Input validation
    if login.len() < 3 || login.len() > 50 {
        return Err(err(StatusCode::BAD_REQUEST, "Login must be 3–50 chars"));
    }
    if req.password.len() < 6 {
        return Err(err(StatusCode::BAD_REQUEST, "Password must be ≥ 6 chars"));
    }

    // Uniqueness check
    if db::find_user_by_login(&s.pool, login)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .is_some()
    {
        return Err(err(StatusCode::CONFLICT, "Login already taken"));
    }

    // Hash password (Argon2id, random salt)
    let hash = hash_password(&req.password)?;
    let user = db::create_user(&s.pool, login, &hash)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    info!("Registered user: {}", user.login);
    s.push_log(format!("New user registered: {}", user.login));

    // Issue a 30-day JWT
    let token = make_token(user.id, &user.role, &s.jwt_secret);
    Ok(Json(AuthResponse {
        token,
        user: user.into(),
    }))
}

/// `POST /auth/login` — authenticate with login + password, return JWT.
///
/// Looks up the user by login, verifies the Argon2id password hash, and
/// returns a fresh JWT on success.  Returns `401 Unauthorized` for both
/// "login not found" and "wrong password" to prevent user enumeration.
pub async fn login(
    State(s): State<Shared>,
    Json(req): Json<LoginRequest>,
) -> ApiResult<AuthResponse> {
    let user = db::find_user_by_login(&s.pool, &req.login)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or(err(StatusCode::UNAUTHORIZED, "Invalid login or password"))?;

    // Constant-time Argon2 verification
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

/// `GET /auth/me` — return the current user's profile.
///
/// Requires a valid JWT.  Re-fetches the user from the database so the
/// response always reflects the latest subscription state and balance.
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

/// `GET /subscription/plans` — list all available subscription tiers.
///
/// Public endpoint — no authentication required.  Returns the static
/// [`PLANS`] array as JSON.
pub async fn list_plans() -> Json<serde_json::Value> {
    Json(serde_json::json!({ "plans": PLANS }))
}

/// `POST /subscription/buy` — purchase a subscription plan.
///
/// Deducts the plan price from the user's balance and activates (or extends)
/// their subscription.  Returns `402 Payment Required` if the balance is
/// insufficient.
///
/// If the subscription is already active, the remaining time is preserved and
/// the new duration is added on top (no wasted days when renewing early).
pub async fn buy_subscription(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
    Json(req): Json<BuySubscriptionRequest>,
) -> ApiResult<serde_json::Value> {
    // Validate the requested plan ID
    let plan = PLANS
        .iter()
        .find(|p| p.id == req.plan_id)
        .ok_or(err(StatusCode::BAD_REQUEST, "Unknown plan"))?;

    let user = db::find_user_by_id(&s.pool, claims.sub)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or(err(StatusCode::NOT_FOUND, "User not found"))?;

    // Balance check before attempting the purchase
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
    s.push_log(format!(
        "User {} → plan {} until {}",
        user.login,
        plan.id,
        expires_at.format("%Y-%m-%d")
    ));

    Ok(Json(serde_json::json!({
        "status": "ok",
        "plan": plan.name,
        "expires_at": expires_at,
        // Note: balance_after is approximate — the real deduction happens in DB
        "balance_after": user.balance - plan.price_rub,
    })))
}

/// `GET /subscription/status` — return the current subscription state.
///
/// If the subscription has expired (expiry in the past) the returned `status`
/// is `"expired"` even if the DB still has `"active"` (eventual consistency —
/// the DB value is updated lazily on next subscription purchase).
pub async fn subscription_status(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
) -> ApiResult<serde_json::Value> {
    let user = db::find_user_by_id(&s.pool, claims.sub)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or(err(StatusCode::NOT_FOUND, "User not found"))?;

    // Compute effective status: treat as expired if past the expiry timestamp
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

/// `POST /promo/apply` — redeem a promo code.
///
/// Validates the code (exists, not expired, not over use limit, not already
/// used by this user), applies the effect to the user's account, records
/// the redemption, and returns a human-readable result.
///
/// # Promo effects by type
/// * `balance`   — adds rubles to the user's balance immediately.
/// * `free_days` — activates subscription for N days at current speed.
/// * `speed`     — activates subscription at a specific Mbit/s tier for N days.
/// * `discount`  — records a discount for the next subscription purchase
///                 (currently a stub).
pub async fn apply_promo(
    State(s): State<Shared>,
    AuthUser(claims): AuthUser,
    Json(req): Json<ApplyPromoRequest>,
) -> ApiResult<ApplyPromoResponse> {
    // Look up the code
    let promo = db::find_promo(&s.pool, &req.code)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
        .ok_or(err(StatusCode::NOT_FOUND, "Promo code not found"))?;

    // Check expiry
    if let Some(exp) = promo.expires_at {
        if exp < Utc::now() {
            return Err(err(StatusCode::GONE, "Promo code expired"));
        }
    }

    // Check global use limit
    if promo.used_count >= promo.max_uses {
        return Err(err(StatusCode::GONE, "Promo code already fully used"));
    }

    // Check per-user uniqueness
    if db::has_user_used_promo(&s.pool, claims.sub, promo.id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?
    {
        return Err(err(StatusCode::CONFLICT, "You already used this promo code"));
    }

    // Apply effects to the user's account
    let (new_balance, new_expires) = db::apply_promo_effects(&s.pool, claims.sub, &promo)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Record the redemption so the user cannot apply the same code again
    db::record_promo_use(&s.pool, claims.sub, promo.id)
        .await
        .map_err(|e| err(StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))?;

    // Human-readable confirmation message (Russian locale)
    let msg = match promo.r#type.as_str() {
        "balance"   => format!("Начислено {:.2} ₽", promo.value),
        "free_days" => format!("Добавлено {} дней VPN", promo.value as i64),
        "speed"     => format!(
            "Активирован VPN {:.0} Мбит/с на {} дней",
            promo.value, promo.extra as i64
        ),
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

/// Hash a plain-text password using Argon2id with a random salt.
///
/// Returns the encoded hash string in PHC format (e.g.
/// `$argon2id$v=19$m=19456,t=2,p=1$...`).
///
/// Argon2id is the recommended algorithm for password hashing as of 2024 —
/// it provides resistance against both GPU and side-channel attacks.
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

/// Verify a plain-text password against a stored Argon2id hash.
///
/// Returns `Ok(true)` if the password matches, `Ok(false)` if it does not.
/// Uses constant-time comparison internally to prevent timing attacks.
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
