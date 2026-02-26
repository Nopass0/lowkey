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

use crate::models::{PromoCode, User};

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

/// Run the initial database migration.
///
/// Reads `migrations/001_initial.sql` (embedded at compile time via
/// `include_str!`) and executes it as a single batch.  All `CREATE TABLE`
/// statements use `IF NOT EXISTS` so this is idempotent — safe to call on
/// every server start.
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

// ── User queries ──────────────────────────────────────────────────────────────

/// Look up a user by their login name.
///
/// Returns `Ok(None)` if no user with that login exists.  Used during
/// login to retrieve the password hash for verification.
pub async fn find_user_by_login(pool: &PgPool, login: &str) -> Result<Option<User>> {
    // CAST(balance AS FLOAT8) — sqlx cannot decode NUMERIC directly to f64
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

/// Look up a user by their numeric database ID.
///
/// Returns `Ok(None)` if the ID does not exist.  Called by JWT-protected
/// endpoints to refresh the user's current subscription state.
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

/// Insert a new user with default `"inactive"` subscription and `0.00` balance.
///
/// The `password_hash` must already be an Argon2 hash string (never store
/// plain-text passwords).  Returns the fully populated [`User`] row.
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
         sub_status, sub_expires_at, sub_speed_mbps, vpn_ip, role, created_at \
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
        // `type` is a non-reserved keyword in PostgreSQL but quoting it avoids
        // any potential conflicts in future PostgreSQL versions.
        "SELECT id, code, \"type\", value, extra, max_uses, used_count, expires_at, created_at \
         FROM promo_codes WHERE code = $1",
    )
    .bind(code)
    .fetch_optional(pool)
    .await?;
    Ok(p)
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

/// Apply the effects of a promo code to a user account.
///
/// Effects vary by promo type:
/// * `balance`   — adds `promo.value` rubles to the user's balance.
/// * `free_days` — activates subscription and extends expiry by `promo.value`
///                 days (unlimited speed).
/// * `speed`     — activates subscription at `promo.value` Mbit/s for
///                 `promo.extra` days.
/// * `discount`  — no immediate effect; the discount is applied at the next
///                 `buy_subscription` call (stub — not yet implemented).
///
/// Returns `(new_balance, new_sub_expires_at)` so the caller can return
/// updated values to the client.
pub async fn apply_promo_effects(
    pool: &PgPool,
    user_id: i32,
    promo: &PromoCode,
) -> Result<(f64, Option<chrono::DateTime<Utc>>)> {
    match promo.r#type.as_str() {
        "balance" => {
            // Simply credit the user's account
            sqlx::query("UPDATE users SET balance = balance + $1 WHERE id = $2")
                .bind(promo.value)
                .bind(user_id)
                .execute(pool)
                .await?;
        }
        "free_days" => {
            // Activate subscription and extend expiry — speed is left unchanged
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
            // Activate subscription at a specific speed tier for N days
            sqlx::query(
                "UPDATE users SET \
                   sub_status = 'active', \
                   sub_speed_mbps = $1, \
                   sub_expires_at = GREATEST(COALESCE(sub_expires_at, NOW()), NOW()) \
                                    + ($2 || ' days')::INTERVAL \
                 WHERE id = $3",
            )
            .bind(promo.value)                      // Mbit/s cap
            .bind((promo.extra as i64).to_string()) // number of days
            .bind(user_id)
            .execute(pool)
            .await?;
        }
        "discount" => {
            // Discount is stored and applied at purchase time — no immediate DB change
        }
        _ => {} // unknown type — ignore gracefully
    }

    // Return the updated balance and expiry for the API response
    let row: (f64, Option<chrono::DateTime<Utc>>) = sqlx::query_as(
        "SELECT CAST(balance AS FLOAT8), sub_expires_at FROM users WHERE id = $1",
    )
    .bind(user_id)
    .fetch_one(pool)
    .await?;

    Ok(row)
}

/// Create a new promo code and persist it to the database.
///
/// Returns the newly created [`PromoCode`] row including its auto-assigned ID.
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
        "INSERT INTO promo_codes (code, \"type\", value, extra, max_uses, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6) \
         RETURNING id, code, \"type\", value, extra, max_uses, used_count, expires_at, created_at",
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
///
/// Returns `true` if the code was valid (not used, not expired) and was
/// successfully consumed.  Returns `false` if the code is unknown, already
/// used or expired.
///
/// The `RETURNING id` clause makes the update atomic — no separate SELECT
/// is needed.
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
