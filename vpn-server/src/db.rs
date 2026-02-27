//! PostgreSQL database access layer.
//!
//! All queries use the **runtime** sqlx API (`query_as::<_, T>(sql)`) rather
//! than the compile-time `query!` macros so that the crate compiles without a
//! live `DATABASE_URL` in the environment.
//!
//! ## Connection pool
//! A single [`PgPool`] is created at startup (max 20 connections) and stored
//! in [`ServerState`](crate::state::ServerState).  Every async function here
//! borrows `&PgPool` — sqlx handles connection checkout/return automatically.
//!
//! ## Migrations
//! [`run_migrations`] executes `migrations/001_initial.sql` at startup using
//! `CREATE TABLE IF NOT EXISTS` so it is safe to re-run on restart.

use anyhow::{Context, Result};
use chrono::Utc;
use sqlx::{postgres::PgPoolOptions, PgPool};
use tracing::info;

use crate::models::{AppRelease, DbSubscriptionPlan, Payment, PromoCode, User, WithdrawalRequest};

// ── Connection pool ───────────────────────────────────────────────────────────

/// Create a PostgreSQL connection pool from a connection URL.
///
/// # Example URL
/// ```text
/// postgres://user:password@localhost:5432/lowkey
/// ```
///
/// Allows up to 20 simultaneous connections.  Returns an error if the
/// server is unreachable or credentials are rejected.
pub async fn create_pool(database_url: &str) -> Result<PgPool> {
    let pool = PgPoolOptions::new()
        .max_connections(20)
        .connect(database_url)
        .await
        .context("Failed to connect to PostgreSQL")?;
    Ok(pool)
}

/// Run all database migrations.
///
/// Reads migration files (embedded at compile time via `include_str!`) and
/// executes each one as a batch.  All `CREATE TABLE` statements use
/// `IF NOT EXISTS` so this is idempotent — safe to call on every server start.
pub async fn run_migrations(pool: &PgPool) -> Result<()> {
    info!("Running database migrations…");
    let sql1 = include_str!("../../migrations/001_initial.sql");
    sqlx::raw_sql(sql1)
        .execute(pool)
        .await
        .context("Migration 001 failed")?;

    let sql2 = include_str!("../../migrations/002_payments_referrals.sql");
    sqlx::raw_sql(sql2)
        .execute(pool)
        .await
        .context("Migration 002 failed")?;

    let sql3 = include_str!("../../migrations/003_promo_v2_releases.sql");
    sqlx::raw_sql(sql3)
        .execute(pool)
        .await
        .context("Migration 003 failed")?;

    // Ensure all users have referral codes
    sqlx::raw_sql(
        "UPDATE users SET referral_code = UPPER(SUBSTRING(MD5(id::TEXT || NOW()::TEXT), 1, 8)) WHERE referral_code IS NULL"
    )
    .execute(pool)
    .await
    .context("Referral code seeding failed")?;

    info!("Migrations OK");
    Ok(())
}

// ── User queries ──────────────────────────────────────────────────────────────

/// Look up a user by their login name.
///
/// Returns `Ok(None)` if no user with that login exists.  Used during
/// login to retrieve the password hash for verification.
pub async fn find_user_by_login(pool: &PgPool, login: &str) -> Result<Option<User>> {
    // CAST(balance AS FLOAT8) — sqlx cannot decode NUMERIC directly to f64
    let u = sqlx::query_as::<_, User>(
        "SELECT id, login, password_hash, CAST(balance AS FLOAT8), \
         sub_status, sub_expires_at, sub_speed_mbps, vpn_ip, role, created_at, \
         referral_code, CAST(referral_balance AS FLOAT8) AS referral_balance, first_purchase_done \
         FROM users WHERE login = $1",
    )
    .bind(login)
    .fetch_optional(pool)
    .await?;
    Ok(u)
}

/// Look up a user by their numeric database ID.
///
/// Returns `Ok(None)` if the ID does not exist.  Called by JWT-protected
/// endpoints to refresh the user's current subscription state.
pub async fn find_user_by_id(pool: &PgPool, id: i32) -> Result<Option<User>> {
    let u = sqlx::query_as::<_, User>(
        "SELECT id, login, password_hash, CAST(balance AS FLOAT8), \
         sub_status, sub_expires_at, sub_speed_mbps, vpn_ip, role, created_at, \
         referral_code, CAST(referral_balance AS FLOAT8) AS referral_balance, first_purchase_done \
         FROM users WHERE id = $1",
    )
    .bind(id)
    .fetch_optional(pool)
    .await?;
    Ok(u)
}

/// Insert a new user with default `"inactive"` subscription and `0.00` balance.
///
/// Generates a unique referral code. Optionally links a referrer by referral_code.
/// The `password_hash` must already be an Argon2 hash string (never store plain-text passwords).
/// Returns the fully populated [`User`] row.
pub async fn create_user(pool: &PgPool, login: &str, password_hash: &str) -> Result<User> {
    let referral_code = generate_referral_code(login);
    let u = sqlx::query_as::<_, User>(
        "INSERT INTO users (login, password_hash, referral_code) \
         VALUES ($1, $2, $3) \
         RETURNING id, login, password_hash, CAST(balance AS FLOAT8), \
         sub_status, sub_expires_at, sub_speed_mbps, vpn_ip, role, created_at, \
         referral_code, CAST(referral_balance AS FLOAT8) AS referral_balance, first_purchase_done",
    )
    .bind(login)
    .bind(password_hash)
    .bind(referral_code)
    .fetch_one(pool)
    .await?;
    Ok(u)
}

/// Create a new user linked to a referrer (by referral code).
/// Grants 50% first-purchase discount.
pub async fn create_user_with_referral(
    pool: &PgPool,
    login: &str,
    password_hash: &str,
    referrer_code: &str,
) -> Result<User> {
    let referral_code = generate_referral_code(login);

    // Look up referrer
    let referrer_id: Option<i32> = sqlx::query_scalar(
        "SELECT id FROM users WHERE referral_code = $1"
    )
    .bind(referrer_code)
    .fetch_optional(pool)
    .await?;

    let u = sqlx::query_as::<_, User>(
        "INSERT INTO users (login, password_hash, referral_code, referred_by) \
         VALUES ($1, $2, $3, $4) \
         RETURNING id, login, password_hash, CAST(balance AS FLOAT8), \
         sub_status, sub_expires_at, sub_speed_mbps, vpn_ip, role, created_at, \
         referral_code, CAST(referral_balance AS FLOAT8) AS referral_balance, first_purchase_done",
    )
    .bind(login)
    .bind(password_hash)
    .bind(&referral_code)
    .bind(referrer_id)
    .fetch_one(pool)
    .await?;
    Ok(u)
}

/// Generate a unique 8-char uppercase referral code from login + timestamp.
fn generate_referral_code(login: &str) -> String {
    use sha2::{Digest, Sha256};
    let input = format!("{}{}", login, std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos());
    let hash = Sha256::digest(input.as_bytes());
    let hex: String = hash.iter().map(|b| format!("{:02x}", b)).collect();
    hex[..8].to_uppercase()
}

/// Update the `vpn_ip` column for a user after a successful peer registration.
///
/// Persisting the VPN IP ensures the same address is reused on reconnect
/// (important for NAT rules and firewall whitelists on the client side).
pub async fn update_user_vpn_ip(pool: &PgPool, user_id: i32, vpn_ip: &str) -> Result<()> {
    sqlx::query("UPDATE users SET vpn_ip = $1 WHERE id = $2")
        .bind(vpn_ip)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Fetch all users ordered by ID.  Used by `GET /admin/users`.
pub async fn list_users(pool: &PgPool) -> Result<Vec<User>> {
    let users = sqlx::query_as::<_, User>(
        "SELECT id, login, password_hash, CAST(balance AS FLOAT8), \
         sub_status, sub_expires_at, sub_speed_mbps, vpn_ip, role, created_at, \
         referral_code, CAST(referral_balance AS FLOAT8) AS referral_balance, first_purchase_done \
         FROM users ORDER BY id",
    )
    .fetch_all(pool)
    .await?;
    Ok(users)
}

/// Set a user's subscription bandwidth cap (`sub_speed_mbps`).
///
/// Called by `PUT /admin/users/:id/limit`.  Setting `speed_mbps = 0.0`
/// removes the cap (unlimited).  The live peer limit is updated separately
/// in the API handler.
pub async fn set_user_limit(pool: &PgPool, user_id: i32, speed_mbps: f64) -> Result<()> {
    sqlx::query("UPDATE users SET sub_speed_mbps = $1 WHERE id = $2")
        .bind(speed_mbps)
        .bind(user_id)
        .execute(pool)
        .await?;
    Ok(())
}

// ── Subscription management ───────────────────────────────────────────────────

/// Activate (or extend) a subscription for a user and deduct the price.
///
/// This function is the core of the billing flow:
/// 1. Deducts `price_paid` from the user's balance.
/// 2. Sets `sub_status = 'active'` and `sub_speed_mbps`.
/// 3. Extends `sub_expires_at` by `duration_days` days — if the subscription
///    is still active the days are added on top of the current expiry, so
///    buying early never wastes time.
/// 4. Inserts a record into the `subscriptions` history table.
///
/// Returns the new `sub_expires_at` timestamp so the caller can show it to
/// the user.
pub async fn activate_subscription(
    pool: &PgPool,
    user_id: i32,
    plan_id: &str,
    price_paid: f64,
    speed_mbps: f64,
    duration_days: i64,
) -> Result<chrono::DateTime<Utc>> {
    // Step 1 — deduct balance (WHERE clause ensures we don't go negative)
    sqlx::query(
        "UPDATE users SET balance = balance - $1 WHERE id = $2 AND balance >= $1",
    )
    .bind(price_paid)
    .bind(user_id)
    .execute(pool)
    .await?;

    // Step 2+3 — update subscription fields and return new expiry.
    // GREATEST(COALESCE(sub_expires_at, NOW()), NOW()) ensures we extend
    // from the later of the current expiry or now.
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

    // Step 4 — audit log
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

// ── Promo code management ─────────────────────────────────────────────────────

/// Look up a promo code by its code string.
///
/// Returns `Ok(None)` if the code does not exist.
pub async fn find_promo(pool: &PgPool, code: &str) -> Result<Option<PromoCode>> {
    let p = sqlx::query_as::<_, PromoCode>(
        "SELECT id, code, \"type\", value, extra, max_uses, used_count, expires_at, created_at,
                COALESCE(target_user_id, NULL)    AS target_user_id,
                COALESCE(only_new_users, FALSE)   AS only_new_users,
                min_purchase_rub,
                second_type,
                COALESCE(second_value, 0.0)       AS second_value,
                COALESCE(max_uses_per_user, 1)    AS max_uses_per_user,
                description,
                COALESCE(created_by, 'admin')     AS created_by
         FROM promo_codes WHERE code = $1",
    )
    .bind(code)
    .fetch_optional(pool)
    .await?;
    Ok(p)
}

/// Count how many times a specific user has used a given promo code.
///
/// Used for per-user limit enforcement when `max_uses_per_user > 1`.
pub async fn count_user_promo_uses(pool: &PgPool, user_id: i32, promo_id: i32) -> Result<i64> {
    let count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM promo_uses WHERE user_id = $1 AND promo_id = $2",
    )
    .bind(user_id)
    .bind(promo_id)
    .fetch_one(pool)
    .await?;
    Ok(count)
}

/// Check whether a specific user has already used a given promo code.
///
/// Used to enforce the one-use-per-user rule before applying effects.
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

/// Record that a user has used a promo and increment the global use counter.
///
/// Uses `ON CONFLICT DO NOTHING` on the `(user_id, promo_id)` unique
/// constraint to make the insert idempotent.
pub async fn record_promo_use(pool: &PgPool, user_id: i32, promo_id: i32) -> Result<()> {
    // Insert use record (idempotent — duplicate inserts are ignored)
    sqlx::query(
        "INSERT INTO promo_uses (user_id, promo_id) VALUES ($1, $2) \
         ON CONFLICT DO NOTHING",
    )
    .bind(user_id)
    .bind(promo_id)
    .execute(pool)
    .await?;

    // Increment global counter
    sqlx::query("UPDATE promo_codes SET used_count = used_count + 1 WHERE id = $1")
        .bind(promo_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Apply the primary effects of a promo code to a user account.
///
/// | type           | effect                                                       |
/// |----------------|--------------------------------------------------------------|
/// | `balance`      | Credits `value` RUB to account balance.                     |
/// | `free_days`    | Extends subscription by `value` days at current speed.      |
/// | `speed`        | Sets subscription speed to `value` Mbit/s for `extra` days. |
/// | `subscription` | Grants a subscription: `value` days at `extra` Mbit/s.      |
/// | `combo`        | Credits `value` RUB **and** extends sub by `extra` days.    |
/// | `discount`     | No immediate DB change; applied at next purchase.           |
///
/// Also applies `second_type`/`second_value` combo effects if set.
///
/// Returns `(new_balance, new_sub_expires_at)`.
pub async fn apply_promo_effects(
    pool: &PgPool,
    user_id: i32,
    promo: &PromoCode,
) -> Result<(f64, Option<chrono::DateTime<Utc>>)> {
    apply_single_effect(pool, user_id, &promo.r#type, promo.value, promo.extra).await?;

    // Apply second/combo effect if configured
    if let Some(ref second) = promo.second_type {
        if !second.is_empty() {
            apply_single_effect(pool, user_id, second, promo.second_value, 0.0).await?;
        }
    }

    // Return updated state for API response
    let row: (f64, Option<chrono::DateTime<Utc>>) = sqlx::query_as(
        "SELECT CAST(balance AS FLOAT8), sub_expires_at FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Apply a single promo effect (balance credit, subscription extension, etc.).
async fn apply_single_effect(pool: &PgPool, user_id: i32, effect_type: &str, value: f64, extra: f64) -> Result<()> {
    match effect_type {
        "balance" => {
            sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
                .bind(value)
                .bind(user_id)
                .execute(pool)
                .await?;
        }
        "free_days" => {
            sqlx::query(
                "UPDATE users SET \
                   sub_status = 'active', \
                   sub_speed_mbps = CASE WHEN sub_speed_mbps = 0.0 THEN 0.0 ELSE sub_speed_mbps END, \
                   sub_expires_at = GREATEST(COALESCE(sub_expires_at, NOW()), NOW()) \
                                    + ($1 || ' days')::INTERVAL \
                 WHERE id = $2",
            )
            .bind((value as i64).to_string())
            .bind(user_id)
            .execute(pool)
            .await?;
        }
        "speed" => {
            sqlx::query(
                "UPDATE users SET \
                   sub_status = 'active', \
                   sub_speed_mbps = $1, \
                   sub_expires_at = GREATEST(COALESCE(sub_expires_at, NOW()), NOW()) \
                                    + ($2 || ' days')::INTERVAL \
                 WHERE id = $3",
            )
            .bind(value)                      // Mbit/s
            .bind((extra as i64).to_string()) // days
            .bind(user_id)
            .execute(pool)
            .await?;
        }
        "subscription" | "combo" if effect_type == "subscription" => {
            // Grant subscription: value=days, extra=speed (0=unlimited)
            sqlx::query(
                "UPDATE users SET \
                   sub_status = 'active', \
                   sub_speed_mbps = $1, \
                   sub_expires_at = GREATEST(COALESCE(sub_expires_at, NOW()), NOW()) \
                                    + ($2 || ' days')::INTERVAL \
                 WHERE id = $3",
            )
            .bind(extra)                      // Mbit/s (0 = unlimited)
            .bind((value as i64).to_string()) // days
            .bind(user_id)
            .execute(pool)
            .await?;
        }
        "combo" => {
            // combo: credit value RUB + extend by extra days
            sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
                .bind(value).bind(user_id).execute(pool).await?;
            if extra > 0.0 {
                sqlx::query(
                    "UPDATE users SET \
                       sub_status = 'active', \
                       sub_expires_at = GREATEST(COALESCE(sub_expires_at, NOW()), NOW()) \
                                        + ($1 || ' days')::INTERVAL \
                     WHERE id = $2",
                )
                .bind((extra as i64).to_string())
                .bind(user_id)
                .execute(pool)
                .await?;
            }
        }
        "discount" => { /* applied at purchase time, no immediate change */ }
        _ => {}
    }
    Ok(())
}

/// Create a new promo code and persist it to the database.
///
/// Returns the newly created [`PromoCode`] row including its auto-assigned ID.
#[allow(clippy::too_many_arguments)]
pub async fn create_promo(
    pool: &PgPool,
    code: &str,
    promo_type: &str,
    value: f64,
    extra: f64,
    max_uses: i32,
    expires_at: Option<chrono::DateTime<Utc>>,
    target_user_id: Option<i32>,
    only_new_users: bool,
    min_purchase_rub: Option<f64>,
    second_type: Option<&str>,
    second_value: f64,
    max_uses_per_user: i32,
    description: Option<&str>,
) -> Result<PromoCode> {
    let p = sqlx::query_as::<_, PromoCode>(
        "INSERT INTO promo_codes
            (code, \"type\", value, extra, max_uses, expires_at,
             target_user_id, only_new_users, min_purchase_rub,
             second_type, second_value, max_uses_per_user, description)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)
         RETURNING id, code, \"type\", value, extra, max_uses, used_count, expires_at, created_at,
                   target_user_id,
                   COALESCE(only_new_users, FALSE)   AS only_new_users,
                   min_purchase_rub,
                   second_type,
                   COALESCE(second_value, 0.0)       AS second_value,
                   COALESCE(max_uses_per_user, 1)    AS max_uses_per_user,
                   description,
                   COALESCE(created_by, 'admin')     AS created_by",
    )
    .bind(code)
    .bind(promo_type)
    .bind(value)
    .bind(extra)
    .bind(max_uses)
    .bind(expires_at)
    .bind(target_user_id)
    .bind(only_new_users)
    .bind(min_purchase_rub)
    .bind(second_type)
    .bind(second_value)
    .bind(max_uses_per_user)
    .bind(description)
    .fetch_one(pool)
    .await?;
    Ok(p)
}

/// List all promo codes ordered by creation date (newest first).
pub async fn list_promos(pool: &PgPool) -> Result<Vec<PromoCode>> {
    let promos = sqlx::query_as::<_, PromoCode>(
        "SELECT id, code, \"type\", value, extra, max_uses, used_count, expires_at, created_at,
                target_user_id,
                COALESCE(only_new_users, FALSE)   AS only_new_users,
                min_purchase_rub,
                second_type,
                COALESCE(second_value, 0.0)       AS second_value,
                COALESCE(max_uses_per_user, 1)    AS max_uses_per_user,
                description,
                COALESCE(created_by, 'admin')     AS created_by
         FROM promo_codes ORDER BY created_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(promos)
}

// ── App release management ────────────────────────────────────────────────────

/// Create a new app release record.
pub async fn create_release(
    pool: &PgPool,
    platform: &str,
    version: &str,
    download_url: &str,
    file_name: Option<&str>,
    file_size_bytes: Option<i64>,
    sha256_checksum: Option<&str>,
    changelog: Option<&str>,
    min_os_version: Option<&str>,
    set_latest: bool,
) -> Result<AppRelease> {
    // Parse semver parts
    let parts: Vec<i32> = version.split('.').filter_map(|p| p.parse().ok()).collect();
    let (major, minor, patch) = (
        parts.first().copied().unwrap_or(0),
        parts.get(1).copied().unwrap_or(0),
        parts.get(2).copied().unwrap_or(0),
    );

    if set_latest {
        sqlx::query("UPDATE app_releases SET is_latest = FALSE WHERE platform = $1")
            .bind(platform)
            .execute(pool)
            .await?;
    }

    let r = sqlx::query_as::<_, AppRelease>(
        "INSERT INTO app_releases
            (platform, version, version_major, version_minor, version_patch,
             is_latest, download_url, file_name, file_size_bytes, sha256_checksum,
             changelog, min_os_version)
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)
         RETURNING *",
    )
    .bind(platform)
    .bind(version)
    .bind(major).bind(minor).bind(patch)
    .bind(set_latest)
    .bind(download_url)
    .bind(file_name)
    .bind(file_size_bytes)
    .bind(sha256_checksum)
    .bind(changelog)
    .bind(min_os_version)
    .fetch_one(pool)
    .await?;
    Ok(r)
}

/// List all releases for all platforms, newest first.
pub async fn list_releases(pool: &PgPool) -> Result<Vec<AppRelease>> {
    let rs = sqlx::query_as::<_, AppRelease>(
        "SELECT * FROM app_releases WHERE is_active = TRUE
         ORDER BY platform, version_major DESC, version_minor DESC, version_patch DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(rs)
}

/// Get the latest release for a platform.
pub async fn get_latest_release(pool: &PgPool, platform: &str) -> Result<Option<AppRelease>> {
    let r = sqlx::query_as::<_, AppRelease>(
        "SELECT * FROM app_releases WHERE platform = $1 AND is_latest = TRUE AND is_active = TRUE",
    )
    .bind(platform)
    .fetch_optional(pool)
    .await?;
    Ok(r)
}

/// Set a release as the latest for its platform (clears previous latest).
pub async fn set_release_latest(pool: &PgPool, release_id: i32) -> Result<()> {
    let platform: Option<String> = sqlx::query_scalar(
        "SELECT platform FROM app_releases WHERE id = $1",
    )
    .bind(release_id)
    .fetch_optional(pool)
    .await?;

    if let Some(p) = platform {
        sqlx::query("UPDATE app_releases SET is_latest = FALSE WHERE platform = $1")
            .bind(&p)
            .execute(pool)
            .await?;
        sqlx::query("UPDATE app_releases SET is_latest = TRUE WHERE id = $1")
            .bind(release_id)
            .execute(pool)
            .await?;
    }
    Ok(())
}

/// Soft-delete a release (mark inactive).
pub async fn delete_release(pool: &PgPool, release_id: i32) -> Result<()> {
    sqlx::query("UPDATE app_releases SET is_active = FALSE WHERE id = $1")
        .bind(release_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// Get latest versions for all platforms (for auto-update checks).
pub async fn get_all_latest_releases(pool: &PgPool) -> Result<Vec<AppRelease>> {
    let rs = sqlx::query_as::<_, AppRelease>(
        "SELECT * FROM app_releases WHERE is_latest = TRUE AND is_active = TRUE",
    )
    .fetch_all(pool)
    .await?;
    Ok(rs)
}

// ── Admin OTP codes ───────────────────────────────────────────────────────────

/// Store a new 6-digit admin OTP code with a 5-minute TTL.
///
/// Marks any existing unused codes as used first to prevent accumulation.
/// Uses `ON CONFLICT DO UPDATE` in case the same code is generated twice
/// (astronomically unlikely with 6 digits but safe either way).
pub async fn create_admin_code(pool: &PgPool, code: &str) -> Result<()> {
    // Expire stale codes
    sqlx::query("UPDATE admin_codes SET used = TRUE WHERE expires_at < NOW()")
        .execute(pool)
        .await?;

    // Insert the new code with a 5-minute TTL
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

/// Verify a 6-digit admin OTP and mark it used in a single atomic UPDATE.
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

// ── Payment / SBP ─────────────────────────────────────────────────────────────

/// Create a new SBP payment order.
pub async fn create_payment(
    pool: &PgPool,
    user_id: i32,
    tochka_order_id: Option<&str>,
    amount: f64,
    purpose: &str,
    plan_id: Option<&str>,
    qr_payload: Option<&str>,
    qr_url: Option<&str>,
    expires_at: Option<chrono::DateTime<Utc>>,
) -> Result<Payment> {
    let p = sqlx::query_as::<_, Payment>(
        "INSERT INTO payments (user_id, tochka_order_id, amount, purpose, plan_id, \
         qr_payload, qr_url, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $8) \
         RETURNING id, user_id, tochka_order_id, \
         CAST(amount AS FLOAT8), purpose, plan_id, status, \
         qr_url, qr_payload, expires_at, paid_at, created_at",
    )
    .bind(user_id)
    .bind(tochka_order_id)
    .bind(amount)
    .bind(purpose)
    .bind(plan_id)
    .bind(qr_payload)
    .bind(qr_url)
    .bind(expires_at)
    .fetch_one(pool)
    .await?;
    Ok(p)
}

/// Get a payment by its internal ID.
pub async fn get_payment(pool: &PgPool, payment_id: i32) -> Result<Option<Payment>> {
    let p = sqlx::query_as::<_, Payment>(
        "SELECT id, user_id, tochka_order_id, \
         CAST(amount AS FLOAT8), purpose, plan_id, status, \
         qr_url, qr_payload, expires_at, paid_at, created_at \
         FROM payments WHERE id = $1",
    )
    .bind(payment_id)
    .fetch_optional(pool)
    .await?;
    Ok(p)
}

/// Get a payment by Tochka order ID.
pub async fn get_payment_by_tochka_id(pool: &PgPool, tochka_order_id: &str) -> Result<Option<Payment>> {
    let p = sqlx::query_as::<_, Payment>(
        "SELECT id, user_id, tochka_order_id, \
         CAST(amount AS FLOAT8), purpose, plan_id, status, \
         qr_url, qr_payload, expires_at, paid_at, created_at \
         FROM payments WHERE tochka_order_id = $1",
    )
    .bind(tochka_order_id)
    .fetch_optional(pool)
    .await?;
    Ok(p)
}

/// List all payments for a user (newest first).
pub async fn list_user_payments(pool: &PgPool, user_id: i32) -> Result<Vec<Payment>> {
    let payments = sqlx::query_as::<_, Payment>(
        "SELECT id, user_id, tochka_order_id, \
         CAST(amount AS FLOAT8), purpose, plan_id, status, \
         qr_url, qr_payload, expires_at, paid_at, created_at \
         FROM payments WHERE user_id = $1 ORDER BY created_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(payments)
}

/// Mark a payment as paid and update user balance / subscription.
/// Also credits 25% to the referrer's referral_balance.
/// Returns (new_user_balance, sub_expires_at).
pub async fn mark_payment_paid(
    pool: &PgPool,
    payment: &Payment,
) -> Result<(f64, Option<chrono::DateTime<Utc>>)> {
    // Mark payment paid
    sqlx::query(
        "UPDATE payments SET status = 'paid', paid_at = NOW() WHERE id = $1"
    )
    .bind(payment.id)
    .execute(pool)
    .await?;

    // If purpose = 'balance', credit user balance
    let mut sub_expires = None;
    if payment.purpose == "balance" {
        sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
            .bind(payment.amount)
            .bind(payment.user_id)
            .execute(pool)
            .await?;
    } else if payment.purpose == "subscription" {
        // Buy the subscription directly
        if let Some(ref plan_id) = payment.plan_id {
            let plan_row = get_plan_by_key(pool, plan_id).await?;
            if let Some(plan) = plan_row {
                let expires = activate_subscription(
                    pool,
                    payment.user_id,
                    plan_id,
                    0.0, // no balance deduction — already paid
                    plan.speed_mbps,
                    plan.duration_days as i64,
                )
                .await?;
                sub_expires = Some(expires);
                // Mark first purchase done
                sqlx::query("UPDATE users SET first_purchase_done = TRUE WHERE id = $1")
                    .bind(payment.user_id)
                    .execute(pool)
                    .await?;
            }
        }
    }

    // Credit 25% to referrer's referral_balance
    let referrer_id: Option<i32> = sqlx::query_scalar(
        "SELECT referred_by FROM users WHERE id = $1"
    )
    .bind(payment.user_id)
    .fetch_optional(pool)
    .await?
    .flatten();

    if let Some(referrer) = referrer_id {
        let commission = payment.amount * 0.25;
        sqlx::query(
            "UPDATE users SET referral_balance = referral_balance + $1 WHERE id = $2"
        )
        .bind(commission)
        .bind(referrer)
        .execute(pool)
        .await?;

        // Log the earning
        sqlx::query(
            "INSERT INTO referral_earnings (referrer_id, referral_id, payment_id, amount) \
             VALUES ($1, $2, $3, $4)"
        )
        .bind(referrer)
        .bind(payment.user_id)
        .bind(payment.id)
        .bind(commission)
        .execute(pool)
        .await?;
    }

    let row: (f64, Option<chrono::DateTime<Utc>>) = sqlx::query_as(
        "SELECT CAST(balance AS FLOAT8), sub_expires_at FROM users WHERE id = $1",
    )
    .bind(payment.user_id)
    .fetch_one(pool)
    .await?;

    Ok((row.0, sub_expires.or(row.1)))
}

/// Update payment with Tochka order info (after API call).
pub async fn update_payment_tochka(
    pool: &PgPool,
    payment_id: i32,
    tochka_order_id: &str,
    qr_payload: &str,
    qr_url: Option<&str>,
    expires_at: Option<chrono::DateTime<Utc>>,
) -> Result<()> {
    sqlx::query(
        "UPDATE payments SET tochka_order_id = $1, qr_payload = $2, qr_url = $3, expires_at = $4 \
         WHERE id = $5"
    )
    .bind(tochka_order_id)
    .bind(qr_payload)
    .bind(qr_url)
    .bind(expires_at)
    .bind(payment_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// Get all pending payments (for admin overview).
pub async fn list_all_payments(pool: &PgPool) -> Result<Vec<Payment>> {
    let payments = sqlx::query_as::<_, Payment>(
        "SELECT id, user_id, tochka_order_id, \
         CAST(amount AS FLOAT8), purpose, plan_id, status, \
         qr_url, qr_payload, expires_at, paid_at, created_at \
         FROM payments ORDER BY created_at DESC LIMIT 200",
    )
    .fetch_all(pool)
    .await?;
    Ok(payments)
}

// ── Referral / withdrawal ─────────────────────────────────────────────────────

/// Get user referral stats.
pub async fn get_referral_stats(pool: &PgPool, user_id: i32) -> Result<serde_json::Value> {
    // Count referrals
    let referral_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE referred_by = $1"
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    // Total earned
    let total_earned: f64 = sqlx::query_scalar::<_, Option<f64>>(
        "SELECT CAST(SUM(amount) AS FLOAT8) FROM referral_earnings WHERE referrer_id = $1"
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?
    .unwrap_or(0.0);

    // Current referral balance
    let referral_balance: f64 = sqlx::query_scalar(
        "SELECT CAST(referral_balance AS FLOAT8) FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    // Referral code
    let referral_code: Option<String> = sqlx::query_scalar(
        "SELECT referral_code FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(serde_json::json!({
        "referral_code": referral_code,
        "referral_count": referral_count,
        "total_earned": total_earned,
        "referral_balance": referral_balance,
    }))
}

/// Create a withdrawal request.
pub async fn create_withdrawal(
    pool: &PgPool,
    user_id: i32,
    amount: f64,
    card_number: &str,
    bank_name: Option<&str>,
) -> Result<WithdrawalRequest> {
    // Deduct from referral_balance
    let updated = sqlx::query_scalar::<_, Option<i32>>(
        "UPDATE users SET referral_balance = referral_balance - $1 \
         WHERE id = $2 AND referral_balance >= $1 RETURNING id"
    )
    .bind(amount)
    .bind(user_id)
    .fetch_optional(pool)
    .await?
    .flatten();

    if updated.is_none() {
        return Err(anyhow::anyhow!("Insufficient referral balance"));
    }

    let w = sqlx::query_as::<_, WithdrawalRequest>(
        "INSERT INTO withdrawal_requests (user_id, amount, card_number, bank_name) \
         VALUES ($1, $2, $3, $4) \
         RETURNING id, user_id, CAST(amount AS FLOAT8), card_number, bank_name, \
         tochka_payout_id, status, admin_note, requested_at, processed_at",
    )
    .bind(user_id)
    .bind(amount)
    .bind(card_number)
    .bind(bank_name)
    .fetch_one(pool)
    .await?;
    Ok(w)
}

/// List withdrawal requests for a user.
pub async fn list_user_withdrawals(pool: &PgPool, user_id: i32) -> Result<Vec<WithdrawalRequest>> {
    let ws = sqlx::query_as::<_, WithdrawalRequest>(
        "SELECT id, user_id, CAST(amount AS FLOAT8), card_number, bank_name, \
         tochka_payout_id, status, admin_note, requested_at, processed_at \
         FROM withdrawal_requests WHERE user_id = $1 ORDER BY requested_at DESC",
    )
    .bind(user_id)
    .fetch_all(pool)
    .await?;
    Ok(ws)
}

/// List all withdrawal requests (admin).
pub async fn list_all_withdrawals(pool: &PgPool) -> Result<Vec<WithdrawalRequest>> {
    let ws = sqlx::query_as::<_, WithdrawalRequest>(
        "SELECT id, user_id, CAST(amount AS FLOAT8), card_number, bank_name, \
         tochka_payout_id, status, admin_note, requested_at, processed_at \
         FROM withdrawal_requests ORDER BY requested_at DESC",
    )
    .fetch_all(pool)
    .await?;
    Ok(ws)
}

/// Update a withdrawal request status (admin).
pub async fn update_withdrawal_status(
    pool: &PgPool,
    withdrawal_id: i32,
    status: &str,
    admin_note: Option<&str>,
    tochka_payout_id: Option<&str>,
) -> Result<()> {
    sqlx::query(
        "UPDATE withdrawal_requests \
         SET status = $1, admin_note = $2, tochka_payout_id = $3, processed_at = NOW() \
         WHERE id = $4"
    )
    .bind(status)
    .bind(admin_note)
    .bind(tochka_payout_id)
    .bind(withdrawal_id)
    .execute(pool)
    .await?;
    Ok(())
}

/// If withdrawal is rejected, refund the referral_balance.
pub async fn refund_withdrawal(pool: &PgPool, withdrawal_id: i32) -> Result<()> {
    let row = sqlx::query_as::<_, (i32, f64)>(
        "SELECT user_id, CAST(amount AS FLOAT8) FROM withdrawal_requests WHERE id = $1"
    )
    .bind(withdrawal_id)
    .fetch_optional(pool)
    .await?;

    if let Some((user_id, amount)) = row {
        sqlx::query(
            "UPDATE users SET referral_balance = referral_balance + $1 WHERE id = $2"
        )
        .bind(amount)
        .bind(user_id)
        .execute(pool)
        .await?;
    }
    Ok(())
}

// ── Subscription plans (DB) ───────────────────────────────────────────────────

/// Get all active subscription plans from DB.
pub async fn list_db_plans(pool: &PgPool) -> Result<Vec<DbSubscriptionPlan>> {
    let plans = sqlx::query_as::<_, DbSubscriptionPlan>(
        "SELECT id, plan_key, name, \
         CAST(price_rub AS FLOAT8), duration_days, speed_mbps, \
         is_bundle, bundle_months, discount_pct, is_active, sort_order \
         FROM subscription_plans WHERE is_active = TRUE ORDER BY sort_order",
    )
    .fetch_all(pool)
    .await?;
    Ok(plans)
}

/// Get a specific plan by key.
pub async fn get_plan_by_key(pool: &PgPool, plan_key: &str) -> Result<Option<DbSubscriptionPlan>> {
    let plan = sqlx::query_as::<_, DbSubscriptionPlan>(
        "SELECT id, plan_key, name, \
         CAST(price_rub AS FLOAT8), duration_days, speed_mbps, \
         is_bundle, bundle_months, discount_pct, is_active, sort_order \
         FROM subscription_plans WHERE plan_key = $1",
    )
    .bind(plan_key)
    .fetch_optional(pool)
    .await?;
    Ok(plan)
}

/// Update a subscription plan's price (admin).
pub async fn update_plan_price(pool: &PgPool, plan_key: &str, price_rub: f64) -> Result<()> {
    sqlx::query("UPDATE subscription_plans SET price_rub = $1 WHERE plan_key = $2")
        .bind(price_rub)
        .bind(plan_key)
        .execute(pool)
        .await?;
    Ok(())
}

/// Get admin financial summary (for admin panel).
pub async fn get_admin_stats(pool: &PgPool) -> Result<serde_json::Value> {
    let total_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users")
        .fetch_one(pool)
        .await?;
    let active_subs: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM users WHERE sub_status = 'active' AND sub_expires_at > NOW()"
    )
    .fetch_one(pool)
    .await?;
    let total_paid: f64 = sqlx::query_scalar::<_, Option<f64>>(
        "SELECT CAST(SUM(amount) AS FLOAT8) FROM payments WHERE status = 'paid'"
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(0.0);
    let pending_referral_payouts: f64 = sqlx::query_scalar::<_, Option<f64>>(
        "SELECT CAST(SUM(amount) AS FLOAT8) FROM withdrawal_requests WHERE status = 'pending'"
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(0.0);
    let total_referral_balance: f64 = sqlx::query_scalar::<_, Option<f64>>(
        "SELECT CAST(SUM(referral_balance) AS FLOAT8) FROM users"
    )
    .fetch_one(pool)
    .await?
    .unwrap_or(0.0);

    Ok(serde_json::json!({
        "total_users": total_users,
        "active_subscriptions": active_subs,
        "total_revenue_rub": total_paid,
        "pending_referral_payouts_rub": pending_referral_payouts,
        "total_referral_balance_frozen_rub": total_referral_balance,
    }))
}

/// Check if a user has first-purchase discount (referred user, no purchases yet).
pub async fn has_first_purchase_discount(pool: &PgPool, user_id: i32) -> Result<bool> {
    let row: Option<(bool, Option<i32>)> = sqlx::query_as(
        "SELECT first_purchase_done, referred_by FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await?;

    Ok(match row {
        Some((false, Some(_))) => true, // referred and hasn't purchased yet
        _ => false,
    })
}
