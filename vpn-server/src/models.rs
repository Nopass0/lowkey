use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Database row types ────────────────────────────────────────────────────────

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct User {
    pub id: i32,
    pub login: String,
    #[serde(skip)]
    pub password_hash: String,
    pub balance: f64,
    pub sub_status: String,
    pub sub_expires_at: Option<DateTime<Utc>>,
    pub sub_speed_mbps: f64,
    pub vpn_ip: Option<String>,
    pub role: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct PromoCode {
    pub id: i32,
    pub code: String,
    pub r#type: String,
    pub value: f64,
    pub extra: f64,
    pub max_uses: i32,
    pub used_count: i32,
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AdminCode {
    pub id: i32,
    pub code: String,
    pub expires_at: DateTime<Utc>,
    pub used: bool,
}

// ── API request / response types ──────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub login: String,
    pub password: String,
}

#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub login: String,
    pub password: String,
}

#[derive(Debug, Serialize)]
pub struct AuthResponse {
    pub token: String,
    pub user: UserPublic,
}

/// Public user info (no password hash)
#[derive(Debug, Serialize)]
pub struct UserPublic {
    pub id: i32,
    pub login: String,
    pub balance: f64,
    pub sub_status: String,
    pub sub_expires_at: Option<DateTime<Utc>>,
    pub sub_speed_mbps: f64,
    pub role: String,
}

impl From<User> for UserPublic {
    fn from(u: User) -> Self {
        UserPublic {
            id: u.id,
            login: u.login,
            balance: u.balance,
            sub_status: u.sub_status,
            sub_expires_at: u.sub_expires_at,
            sub_speed_mbps: u.sub_speed_mbps,
            role: u.role,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct ApplyPromoRequest {
    pub code: String,
}

#[derive(Debug, Serialize)]
pub struct ApplyPromoResponse {
    pub message: String,
    pub new_balance: f64,
    pub sub_expires_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Deserialize)]
pub struct BuySubscriptionRequest {
    pub plan_id: String,
}

// ── Subscription plans ────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Clone)]
pub struct SubscriptionPlan {
    pub id: &'static str,
    pub name: &'static str,
    pub price_rub: f64,
    pub duration_days: i64,
    pub speed_mbps: f64, // 0 = unlimited
}

pub const PLANS: &[SubscriptionPlan] = &[
    SubscriptionPlan {
        id: "basic",
        name: "Базовый (10 Мбит/с)",
        price_rub: 199.0,
        duration_days: 30,
        speed_mbps: 10.0,
    },
    SubscriptionPlan {
        id: "standard",
        name: "Стандарт (50 Мбит/с)",
        price_rub: 299.0,
        duration_days: 30,
        speed_mbps: 50.0,
    },
    SubscriptionPlan {
        id: "premium",
        name: "Премиум (без ограничений)",
        price_rub: 499.0,
        duration_days: 30,
        speed_mbps: 0.0,
    },
];

// ── Admin API ─────────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub struct AdminVerifyRequest {
    pub code: String,
}

#[derive(Debug, Deserialize)]
pub struct CreatePromoRequest {
    pub code: String,
    pub r#type: String,   // balance | discount | free_days | speed
    pub value: f64,
    pub extra: Option<f64>, // for 'speed': duration in days
    pub max_uses: Option<i32>,
    pub expires_days: Option<i64>, // days from now until expiry
}

#[derive(Debug, Deserialize)]
pub struct SetLimitRequest {
    pub limit_mbps: f64,
}

// ── JWT claims ────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: i32,        // user id
    pub role: String,    // "user" | "admin"
    pub exp: usize,      // unix timestamp
}
