//! Database row types, API request/response types and subscription plans.
//!
//! All structs that map to SQL rows derive [`sqlx::FromRow`] so sqlx can
//! deserialise them directly.  Structs used in HTTP bodies derive
//! [`serde::Deserialize`] / [`serde::Serialize`] as needed.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

// ── Database row types ────────────────────────────────────────────────────────

/// A registered user record, as stored in the `users` table.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct User {
    /// Auto-incremented primary key.
    pub id: i32,
    /// Unique login name (3–50 characters).
    pub login: String,
    /// Argon2id password hash.  Never serialised to API responses.
    #[serde(skip)]
    pub password_hash: String,
    /// Account balance in Russian rubles (NUMERIC(12,2) cast to f64).
    pub balance: f64,
    /// Subscription status: `"inactive"` | `"active"` | `"expired"`.
    pub sub_status: String,
    /// UTC timestamp when the current subscription expires (`None` if never subscribed).
    pub sub_expires_at: Option<DateTime<Utc>>,
    /// Bandwidth cap for this user's VPN session in Mbit/s. `0.0` means unlimited.
    pub sub_speed_mbps: f64,
    /// Last assigned VPN IP as a string (e.g. `"10.0.0.5"`).
    pub vpn_ip: Option<String>,
    /// Role: `"user"` or `"admin"`.
    pub role: String,
    /// UTC timestamp of account creation.
    pub created_at: DateTime<Utc>,
    /// Unique referral code for this user.
    pub referral_code: Option<String>,
    /// Accumulated referral earnings balance in rubles.
    pub referral_balance: f64,
    /// Whether the user has made their first subscription purchase.
    pub first_purchase_done: bool,
}

/// A promo code record from the `promo_codes` table.
///
/// # Promo types
/// | type           | value              | extra                  | second_type/second_value        |
/// |----------------|--------------------|------------------------|---------------------------------|
/// | `balance`      | RUB credited       | —                      | any combo                       |
/// | `discount`     | % off subscription | —                      | —                               |
/// | `free_days`    | days of free VPN   | —                      | `balance` for referral bonuses  |
/// | `speed`        | Mbit/s cap         | days active            | `balance` for bonus cash        |
/// | `subscription` | plan duration days | speed Mbit/s (0=∞)     | —                               |
/// | `combo`        | RUB credited       | free VPN days          | —                               |
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct PromoCode {
    pub id: i32,
    /// The code string users enter (case-sensitive, unique).
    pub code: String,
    /// Promo type — see table above.
    pub r#type: String,
    /// Primary numeric value (meaning depends on `type`).
    pub value: f64,
    /// Secondary value (meaning depends on `type`).
    pub extra: f64,
    /// Maximum total redemptions across all users (0 = unlimited).
    pub max_uses: i32,
    /// How many times the code has already been used.
    pub used_count: i32,
    /// Optional UTC expiry timestamp.  `None` = never expires.
    pub expires_at: Option<DateTime<Utc>>,
    pub created_at: DateTime<Utc>,
    // ── New fields from migration 003 ─────────────────────────────────────────
    /// If set, only this specific user ID can redeem the code (individual promo).
    pub target_user_id: Option<i32>,
    /// If true, only users who have never subscribed before can use this code.
    pub only_new_users: bool,
    /// For `discount` type: minimum payment amount in RUB before discount applies.
    pub min_purchase_rub: Option<f64>,
    /// Optional second effect type applied together with the primary (combo).
    pub second_type: Option<String>,
    /// Numeric value for the second effect.
    pub second_value: f64,
    /// Maximum times a single user can redeem this code (1 = once per user).
    pub max_uses_per_user: i32,
    /// Human-readable description shown in admin panel.
    pub description: Option<String>,
    /// Login of the admin who created this code.
    pub created_by: String,
}

/// A client application release stored in `app_releases`.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct AppRelease {
    pub id: i32,
    /// Platform identifier: `"windows"` | `"linux"` | `"android"` | `"macos"`.
    pub platform: String,
    /// Semantic version string, e.g. `"1.2.3"`.
    pub version: String,
    pub version_major: i32,
    pub version_minor: i32,
    pub version_patch: i32,
    /// Whether this is the recommended (latest) release for this platform.
    pub is_latest: bool,
    /// Whether this release is publicly accessible.
    pub is_active: bool,
    /// Direct URL to download the binary.
    pub download_url: String,
    /// Suggested filename for the download (e.g. `"lowkey-setup-1.2.3.exe"`).
    pub file_name: Option<String>,
    /// Binary size in bytes, shown in the downloads page.
    pub file_size_bytes: Option<i64>,
    /// Optional SHA-256 checksum of the binary for integrity verification.
    pub sha256_checksum: Option<String>,
    /// Markdown-formatted release notes / changelog.
    pub changelog: Option<String>,
    /// Minimum supported OS version (e.g. `"10"` for Windows 10).
    pub min_os_version: Option<String>,
    pub released_at: DateTime<Utc>,
    pub created_at: DateTime<Utc>,
}

/// An admin one-time password record from the `admin_codes` table.
///
/// OTP codes are generated by `POST /admin/request-code` and consumed
/// (marked used) by `POST /admin/verify-code`.
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct AdminCode {
    pub id: i32,
    /// The 6-digit OTP code as a string.
    pub code: String,
    /// UTC timestamp after which the code is no longer valid (5 minutes).
    pub expires_at: DateTime<Utc>,
    /// `true` once the code has been verified or expired.
    pub used: bool,
}

// ── API request / response types ──────────────────────────────────────────────

/// Body for `POST /auth/register`.
#[derive(Debug, Deserialize)]
pub struct RegisterRequest {
    pub login: String,
    pub password: String,
    /// Optional referral code — links this user to a referrer.
    pub referral_code: Option<String>,
}

/// Body for `POST /auth/login`.
#[derive(Debug, Deserialize)]
pub struct LoginRequest {
    pub login: String,
    pub password: String,
}

/// Response from `POST /auth/register` and `POST /auth/login`.
#[derive(Debug, Serialize)]
pub struct AuthResponse {
    /// Signed JWT — attach as `Authorization: Bearer <token>` on subsequent
    /// requests.  Valid for 30 days.
    pub token: String,
    /// Public user profile (no password hash).
    pub user: UserPublic,
}

/// Public user profile returned by the API (password hash excluded).
#[derive(Debug, Serialize)]
pub struct UserPublic {
    pub id: i32,
    pub login: String,
    pub balance: f64,
    pub sub_status: String,
    pub sub_expires_at: Option<DateTime<Utc>>,
    pub sub_speed_mbps: f64,
    pub role: String,
    pub referral_code: Option<String>,
    pub referral_balance: f64,
    pub first_purchase_done: bool,
}

/// Convert a full [`User`] row into the public-facing subset.
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
            referral_code: u.referral_code,
            referral_balance: u.referral_balance,
            first_purchase_done: u.first_purchase_done,
        }
    }
}

/// Body for `POST /promo/apply`.
#[derive(Debug, Deserialize)]
pub struct ApplyPromoRequest {
    /// The promo code string to apply.
    pub code: String,
}

/// Response from `POST /promo/apply`.
#[derive(Debug, Serialize)]
pub struct ApplyPromoResponse {
    /// Human-readable description of what was applied.
    pub message: String,
    /// Updated account balance after applying the promo.
    pub new_balance: f64,
    /// Updated subscription expiry date (if changed by this promo).
    pub sub_expires_at: Option<DateTime<Utc>>,
}

/// Body for `POST /subscription/buy`.
#[derive(Debug, Deserialize)]
pub struct BuySubscriptionRequest {
    /// One of `"basic"`, `"standard"`, `"premium"` (see [`PLANS`]).
    pub plan_id: String,
}

// ── Subscription plans ────────────────────────────────────────────────────────

/// Static definition of an available subscription tier.
#[derive(Debug, Serialize, Clone)]
pub struct SubscriptionPlan {
    /// Machine-readable identifier (used in API requests and DB records).
    pub id: &'static str,
    /// Display name shown in the `GET /subscription/plans` response.
    pub name: &'static str,
    /// Price in Russian rubles, deducted from the user's balance.
    pub price_rub: f64,
    /// Subscription duration in days.
    pub duration_days: i64,
    /// Bandwidth cap in Mbit/s.  `0.0` = unlimited.
    pub speed_mbps: f64,
}

/// All available subscription plans.
///
/// Plans are intentionally stored as a static slice to avoid a DB round-trip
/// on every `GET /subscription/plans` request.
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
        speed_mbps: 0.0, // 0 = unlimited
    },
];

// ── Admin API request types ───────────────────────────────────────────────────

/// Body for `POST /admin/verify-code`.
#[derive(Debug, Deserialize)]
pub struct AdminVerifyRequest {
    /// The 6-digit OTP that was sent to the admin's Telegram DM.
    pub code: String,
}

/// Body for `POST /admin/promos` — create a new promo code.
///
/// All optional fields default to the most restrictive / simplest behaviour
/// when omitted so admins can create simple codes with minimal input.
#[derive(Debug, Deserialize)]
pub struct CreatePromoRequest {
    /// Unique promo code string (auto-generated if empty).
    pub code: String,
    /// Promo type: `"balance"` | `"discount"` | `"free_days"` | `"speed"` |
    /// `"subscription"` | `"combo"`.
    pub r#type: String,
    /// Primary numeric value (see [`PromoCode`] type table).
    pub value: f64,
    /// Secondary value (days for `speed`/`subscription`, extra RUB for `combo`).
    pub extra: Option<f64>,
    /// Total maximum redemptions across all users (default: 1, 0 = unlimited).
    pub max_uses: Option<i32>,
    /// Days from now until the code expires (`None` = never expires).
    pub expires_days: Option<i64>,
    // ── New condition fields ──────────────────────────────────────────────────
    /// If set, only this user ID can redeem the code (individual one-time code).
    pub target_user_id: Option<i32>,
    /// Restrict to users who have never purchased a subscription.
    pub only_new_users: Option<bool>,
    /// For `discount` type: minimum purchase amount in RUB required.
    pub min_purchase_rub: Option<f64>,
    /// Second effect type to apply together with the primary (e.g. `"balance"`).
    pub second_type: Option<String>,
    /// Value for the second effect.
    pub second_value: Option<f64>,
    /// How many times a single user may apply this code (default: 1).
    pub max_uses_per_user: Option<i32>,
    /// Human-readable note for admin panel (not shown to users).
    pub description: Option<String>,
}

/// Body for `POST /admin/releases` — publish a new app release.
#[derive(Debug, Deserialize)]
pub struct CreateReleaseRequest {
    /// Target platform: `"windows"` | `"linux"` | `"android"` | `"macos"`.
    pub platform: String,
    /// Semantic version string, e.g. `"1.2.3"`.
    pub version: String,
    /// Direct download URL for the binary.
    pub download_url: String,
    /// Suggested filename shown to the user.
    pub file_name: Option<String>,
    /// Binary size in bytes.
    pub file_size_bytes: Option<i64>,
    /// SHA-256 hex checksum for integrity verification.
    pub sha256_checksum: Option<String>,
    /// Markdown release notes.
    pub changelog: Option<String>,
    /// Minimum supported OS version.
    pub min_os_version: Option<String>,
    /// Whether to immediately set this as the latest release for this platform.
    pub set_latest: Option<bool>,
}

/// Body for `PUT /admin/users/:id/limit`.
#[derive(Debug, Deserialize)]
pub struct SetLimitRequest {
    /// New bandwidth cap in Mbit/s (`0.0` = unlimited).
    pub limit_mbps: f64,
}

// ── Payment / SBP types ───────────────────────────────────────────────────────

/// A payment order from the `payments` table.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct Payment {
    pub id: i32,
    pub user_id: i32,
    pub tochka_order_id: Option<String>,
    pub amount: f64,
    pub purpose: String,   // 'balance' | 'subscription'
    pub plan_id: Option<String>,
    pub status: String,    // 'pending' | 'paid' | 'expired' | 'failed'
    pub qr_url: Option<String>,
    pub qr_payload: Option<String>,
    pub expires_at: Option<chrono::DateTime<Utc>>,
    pub paid_at: Option<chrono::DateTime<Utc>>,
    pub created_at: chrono::DateTime<Utc>,
}

/// Body for `POST /payment/sbp/create`.
#[derive(Debug, Deserialize)]
pub struct CreatePaymentRequest {
    /// Amount in rubles.
    pub amount: f64,
    /// `"balance"` — top up balance; `"subscription"` — buy directly.
    pub purpose: String,
    /// Required when purpose = "subscription".
    pub plan_id: Option<String>,
}

/// Response for `POST /payment/sbp/create`.
#[derive(Debug, Serialize)]
pub struct CreatePaymentResponse {
    pub payment_id: i32,
    pub qr_payload: String,
    pub qr_url: Option<String>,
    pub amount: f64,
    pub expires_at: Option<chrono::DateTime<Utc>>,
}

/// Response for `GET /payment/sbp/status/:id`.
#[derive(Debug, Serialize)]
pub struct PaymentStatusResponse {
    pub payment_id: i32,
    pub status: String,
    pub amount: f64,
    pub paid_at: Option<chrono::DateTime<Utc>>,
    pub balance_after: Option<f64>,
    pub sub_expires_at: Option<chrono::DateTime<Utc>>,
}

// ── Referral types ────────────────────────────────────────────────────────────

/// A withdrawal request from `withdrawal_requests` table.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct WithdrawalRequest {
    pub id: i32,
    pub user_id: i32,
    pub amount: f64,
    pub card_number: String,
    pub bank_name: Option<String>,
    pub tochka_payout_id: Option<String>,
    pub status: String,    // 'pending' | 'processing' | 'completed' | 'rejected'
    pub admin_note: Option<String>,
    pub requested_at: chrono::DateTime<Utc>,
    pub processed_at: Option<chrono::DateTime<Utc>>,
}

/// Body for `POST /referral/withdraw`.
#[derive(Debug, Deserialize)]
pub struct WithdrawRequest {
    pub amount: f64,
    pub card_number: String,
    pub bank_name: Option<String>,
}

/// A subscription plan from the DB.
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct DbSubscriptionPlan {
    pub id: i32,
    pub plan_key: String,
    pub name: String,
    pub price_rub: f64,
    pub duration_days: i32,
    pub speed_mbps: f64,
    pub is_bundle: bool,
    pub bundle_months: i32,
    pub discount_pct: f64,
    pub is_active: bool,
    pub sort_order: i32,
}

// ── JWT claims ────────────────────────────────────────────────────────────────

/// Payload embedded in every signed JWT.
///
/// Tokens are issued by `POST /auth/login`, `POST /auth/register` and
/// `POST /admin/verify-code`.  Clients must include the token in every
/// protected request as `Authorization: Bearer <token>`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    /// Subject — the user's database ID.  Admin sessions use `0`.
    pub sub: i32,
    /// Role string: `"user"` or `"admin"`.
    pub role: String,
    /// Expiry as a Unix timestamp (seconds).  30 days from issuance.
    pub exp: usize,
}
