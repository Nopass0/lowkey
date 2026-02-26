-- 001_initial.sql
CREATE TABLE IF NOT EXISTS users (
    id           SERIAL PRIMARY KEY,
    login        VARCHAR(50)  UNIQUE NOT NULL,
    password_hash TEXT        NOT NULL,
    balance      NUMERIC(12,2) NOT NULL DEFAULT 0.00,
    sub_status   VARCHAR(20)  NOT NULL DEFAULT 'inactive',   -- 'inactive' | 'active' | 'expired'
    sub_expires_at TIMESTAMPTZ,
    sub_speed_mbps FLOAT8     NOT NULL DEFAULT 0.0,          -- 0 = unlimited
    vpn_ip       VARCHAR(20),
    role         VARCHAR(10)  NOT NULL DEFAULT 'user',        -- 'user' | 'admin'
    created_at   TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

-- Promo codes
-- type: 'balance'    → value = rub added to balance
--       'discount'   → value = percent off next subscription
--       'free_days'  → value = days of free VPN (unlimited speed)
--       'speed'      → value = Mbps cap for N days (speed, days packed as value|days)
CREATE TABLE IF NOT EXISTS promo_codes (
    id          SERIAL PRIMARY KEY,
    code        VARCHAR(50)  UNIQUE NOT NULL,
    type        VARCHAR(20)  NOT NULL
                    CHECK (type IN ('balance','discount','free_days','speed')),
    value       FLOAT8       NOT NULL,
    extra       FLOAT8       NOT NULL DEFAULT 0,   -- for 'speed': days count
    max_uses    INT          NOT NULL DEFAULT 1,
    used_count  INT          NOT NULL DEFAULT 0,
    expires_at  TIMESTAMPTZ,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);

CREATE TABLE IF NOT EXISTS promo_uses (
    id        SERIAL PRIMARY KEY,
    user_id   INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    promo_id  INT NOT NULL REFERENCES promo_codes(id) ON DELETE CASCADE,
    UNIQUE (user_id, promo_id),
    used_at   TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Subscriptions (stub — future billing)
CREATE TABLE IF NOT EXISTS subscriptions (
    id          SERIAL PRIMARY KEY,
    user_id     INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    plan_id     VARCHAR(30) NOT NULL,
    price_paid  NUMERIC(12,2) NOT NULL,
    speed_mbps  FLOAT8 NOT NULL DEFAULT 0.0,
    started_at  TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at  TIMESTAMPTZ NOT NULL,
    status      VARCHAR(20) NOT NULL DEFAULT 'active'
);

-- Admin OTP codes sent via Telegram
CREATE TABLE IF NOT EXISTS admin_codes (
    id          SERIAL PRIMARY KEY,
    code        VARCHAR(10)  UNIQUE NOT NULL,
    expires_at  TIMESTAMPTZ  NOT NULL,
    used        BOOLEAN      NOT NULL DEFAULT FALSE,
    created_at  TIMESTAMPTZ  NOT NULL DEFAULT NOW()
);
