use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::{postgres::PgPoolOptions, PgPool};
use tracing::info;

use crate::models::{PromoCode, User};

// ── Pool ──────────────────────────────────────────────────────────────────────

pub async fn create_pool(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(database_url)
        .await
        .context("Failed to connect to PostgreSQL")?;
    Ok(pool)
}

pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    info!("Running database migrations…");
    let sql = include_str!("../../migrations/001_initial.sql");
    sqlx::raw_sql(sql)
        .execute(pool)
        .await
        .context("Migration failed")?;
    info!("Migrations OK");
    Ok(())
}

// ── Users ─────────────────────────────────────────────────────────────────────

pub async fn find_user_by_login(pool: &PgPool, login: &str) -> Result<Option<User>> {
    let u = sqlx::query_as::<_, User>(
        "SELECT id, login, password_hash, CAST(balance AS FLOAT8), \
         sub_status, sub_expires_at, sub_speed_mbps, vpn_ip, role, created_at \
         FROM users WHERE login = $1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await?;
    Ok(u)
}

pub async fn find_user_by_id(pool: &PgPool, id: i32) -> Result<Option<User>> {
    let u = sqlx::query_as::<_, User>(
        "SELECT id, login, password_hash, CAST(balance AS FLOAT8), \
         sub_status, sub_expires_at, sub_speed_mbps, vpn_ip, role, created_at \
         FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(u)
}

pub async fn create_user(pool: &PgPool, login: &str, password_hash: &str) -> Result<User> {
    let u = sqlx::query_as::<_, User>(
        "INSERT INTO users (login, password_hash) \
         VALUES ($1, $2) \
         RETURNING id, login, password_hash, CAST(balance AS FLOAT8), \
         sub_status, sub_expires_at, sub_speed_mbps, vpn_ip, role, created_at",
    )
    .bind(login)
    .bind(password_hash)
    .fetch_one(pool)
    .await?;
    Ok(u)
}

pub async fn update_user_vpn_ip(pool: &PgPool, user_id: i32, vpn_ip: &str) -> Result<()> {
    sqlx::query("UPDATE users SET vpn_ip = $1 WHERE id = $2")
        .bind(vpn_ip)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

pub async fn list_users(pool: &PgPool) -> Result<Vec<User>> {
    let users = sqlx::query_as::<_, User>(
        "SELECT id, login, password_hash, CAST(balance AS FLOAT8), \
         sub_status, sub_expires_at, sub_speed_mbps, vpn_ip, role, created_at \
         FROM users ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    Ok(users)
}

pub async fn set_user_limit(pool: &PgPool, user_id: i32, speed_mbps: f64) -> Result<()> {
    sqlx::query("UPDATE users SET sub_speed_mbps = $1 WHERE id = $2")
        .bind(speed_mbps)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ── Subscriptions ─────────────────────────────────────────────────────────────

/// Activate subscription for a user. Returns new sub_expires_at.
pub async fn activate_subscription(
    pool: &PgPool,
    user_id: i32,
    plan_id: &str,
    price_paid: f64,
    speed_mbps: f64,
    duration_days: i64,
) -> Result<chrono::DateTime<Utc>> {
    // Deduct balance
    sqlx::query(
        "UPDATE users SET balance = balance - $1 WHERE id = $2 AND balance >= $1",
    )
    .bind(price_paid)
    .bind(user_id)
    .execute(pool)
    .await?;

    // Calculate new expiry (extend if already active)
    let expires_at: chrono::DateTime<Utc> = sqlx::query_scalar(
        "UPDATE users SET \
           sub_status = 'active', \
           sub_speed_mbps = $3, \
           sub_expires_at = GREATEST(COALESCE(sub_expires_at, NOW()), NOW()) \
                            + ($4 || ' days')::INTERVAL \
         WHERE id = $2 \
         RETURNING sub_expires_at",
    )
    .bind(price_paid)
    .bind(user_id)
    .bind(speed_mbps)
    .bind(duration_days.to_string())
    .fetch_one(pool)
    .await?;

    // Record subscription
    sqlx::query(
        "INSERT INTO subscriptions (user_id, plan_id, price_paid, speed_mbps, expires_at) \
         VALUES ($1, $2, $3, $4, $5)",
    )
    .bind(user_id)
    .bind(plan_id)
    .bind(price_paid)
    .bind(speed_mbps)
    .bind(expires_at)
    .execute(pool)
    .await?;

    Ok(expires_at)
}

// ── Promo codes ───────────────────────────────────────────────────────────────

pub async fn find_promo(pool: &PgPool, code: &str) -> Result<Option<PromoCode>> {
    let p = sqlx::query_as::<_, PromoCode>(
        "SELECT id, code, type, value, extra, max_uses, used_count, expires_at, created_at \
         FROM promo_codes WHERE code = $1",
    )
    .bind(code)
    .fetch_optional(pool)
    .await?;
    Ok(p)
}

pub async fn has_user_used_promo(pool: &PgPool, user_id: i32, promo_id: i32) -> Result<bool> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM promo_uses WHERE user_id = $1 AND promo_id = $2",
    )
    .bind(user_id)
    .bind(promo_id)
    .fetch_one(pool)
    .await?;
    Ok(count > 0)
}

pub async fn record_promo_use(pool: &PgPool, user_id: i32, promo_id: i32) -> Result<()> {
    sqlx::query(
        "INSERT INTO promo_uses (user_id, promo_id) VALUES ($1, $2) \
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(promo_id)
    .execute(pool)
    .await?;
    sqlx::query("UPDATE promo_codes SET used_count = used_count + 1 WHERE id = $1")
        .bind(promo_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Apply effects of a promo to a user. Returns (new_balance, new_sub_expires_at).
pub async fn apply_promo_effects(
    pool: &PgPool,
    user_id: i32,
    promo: &PromoCode,
) -> Result<(f64, Option<chrono::DateTime<Utc>>)> {
    match promo.r#type.as_str() {
        "balance" => {
            sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
                .bind(promo.value)
                .bind(user_id)
                .execute(pool)
                .await?;
        }
        "free_days" => {
            sqlx::query(
                "UPDATE users SET \
                   sub_status = 'active', \
                   sub_expires_at = GREATEST(COALESCE(sub_expires_at, NOW()), NOW()) \
                                    + ($1 || ' days')::INTERVAL, \
                   sub_speed_mbps = CASE WHEN sub_speed_mbps = 0 THEN 0 ELSE sub_speed_mbps END \
                 WHERE id = $2",
            )
            .bind((promo.value as i64).to_string())
            .bind(user_id)
            .execute(pool)
            .await?;
        }
        "speed" => {
            // value = Mbps, extra = days
            sqlx::query(
                "UPDATE users SET \
                   sub_status = 'active', \
                   sub_speed_mbps = $1, \
                   sub_expires_at = GREATEST(COALESCE(sub_expires_at, NOW()), NOW()) \
                                    + ($2 || ' days')::INTERVAL \
                 WHERE id = $3",
            )
            .bind(promo.value)
            .bind((promo.extra as i64).to_string())
            .bind(user_id)
            .execute(pool)
            .await?;
        }
        "discount" => {
            // Discount is applied at purchase time; no immediate effect.
        }
        _ => {}
    }

    let row: (f64, Option<chrono::DateTime<Utc>>) = sqlx::query_as(
        "SELECT CAST(balance AS FLOAT8), sub_expires_at FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

pub async fn create_promo(
    pool: &PgPool,
    code: &str,
    promo_type: &str,
    value: f64,
    extra: f64,
    max_uses: i32,
    expires_at: Option<chrono::DateTime<Utc>>,
) -> Result<PromoCode> {
    let p = sqlx::query_as::<_, PromoCode>(
        "INSERT INTO promo_codes (code, type, value, extra, max_uses, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id, code, type, value, extra, max_uses, used_count, expires_at, created_at",
    )
    .bind(code)
    .bind(promo_type)
    .bind(value)
    .bind(extra)
    .bind(max_uses)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;
    Ok(p)
}

// ── Admin OTP codes ───────────────────────────────────────────────────────────

pub async fn create_admin_code(pool: &PgPool, code: &str) -> Result<()> {
    // Expire old unused codes first
    sqlx::query("UPDATE admin_codes SET used = TRUE WHERE expires_at < NOW()")
        .execute(pool)
        .await?;

    sqlx::query(
        "INSERT INTO admin_codes (code, expires_at) \
         VALUES ($1, NOW() + INTERVAL '5 minutes') \
         ON CONFLICT (code) DO UPDATE SET expires_at = NOW() + INTERVAL '5 minutes', used = FALSE",
    )
    .bind(code)
    .execute(pool)
    .await?;
    Ok(())
}

pub async fn verify_admin_code(pool: &PgPool, code: &str) -> Result<bool> {
    let valid: Option<i32> = sqlx::query_scalar(
        "UPDATE admin_codes SET used = TRUE \
         WHERE code = $1 AND used = FALSE AND expires_at > NOW() \
         RETURNING id",
    )
    .bind(code)
    .fetch_optional(pool)
    .await?;
    Ok(valid.is_some())
}
