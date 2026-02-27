-- 002_payments_referrals.sql
-- SBP payments, referral system, withdrawal requests

-- Add referral fields to users
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS referral_code    VARCHAR(20) UNIQUE,
    ADD COLUMN IF NOT EXISTS referred_by      INT REFERENCES users(id) ON DELETE SET NULL,
    ADD COLUMN IF NOT EXISTS referral_balance NUMERIC(12,2) NOT NULL DEFAULT 0.00,
    ADD COLUMN IF NOT EXISTS first_purchase_done BOOLEAN NOT NULL DEFAULT FALSE;

-- Generate referral codes for existing users (will also be done in app code)
UPDATE users SET referral_code = UPPER(SUBSTRING(MD5(RANDOM()::TEXT), 1, 8)) WHERE referral_code IS NULL;

-- SBP payment orders (Tochka Bank)
CREATE TABLE IF NOT EXISTS payments (
    id              SERIAL PRIMARY KEY,
    user_id         INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    tochka_order_id VARCHAR(100) UNIQUE,     -- Tochka Bank order ID
    amount          NUMERIC(12,2) NOT NULL,
    purpose         VARCHAR(50) NOT NULL DEFAULT 'balance', -- 'balance' | 'subscription'
    plan_id         VARCHAR(30),             -- if purpose='subscription'
    status          VARCHAR(20) NOT NULL DEFAULT 'pending', -- 'pending'|'paid'|'expired'|'failed'
    qr_url          TEXT,                    -- SBP QR code data URL or link
    qr_payload      TEXT,                    -- raw QR payload string
    expires_at      TIMESTAMPTZ,
    paid_at         TIMESTAMPTZ,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Referral earnings log (25% of each referred user's top-up)
CREATE TABLE IF NOT EXISTS referral_earnings (
    id              SERIAL PRIMARY KEY,
    referrer_id     INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    referral_id     INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    payment_id      INT NOT NULL REFERENCES payments(id) ON DELETE CASCADE,
    amount          NUMERIC(12,2) NOT NULL,  -- 25% of payment amount
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Withdrawal requests (referral earnings → card via Tochka API)
CREATE TABLE IF NOT EXISTS withdrawal_requests (
    id              SERIAL PRIMARY KEY,
    user_id         INT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    amount          NUMERIC(12,2) NOT NULL,
    card_number     VARCHAR(20) NOT NULL,    -- last 4 or full card number
    bank_name       VARCHAR(100),
    tochka_payout_id VARCHAR(100),           -- Tochka payout request ID
    status          VARCHAR(20) NOT NULL DEFAULT 'pending', -- 'pending'|'processing'|'completed'|'rejected'
    admin_note      TEXT,
    requested_at    TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    processed_at    TIMESTAMPTZ
);

-- Subscription plans (configurable from admin panel)
CREATE TABLE IF NOT EXISTS subscription_plans (
    id              SERIAL PRIMARY KEY,
    plan_key        VARCHAR(30) UNIQUE NOT NULL,  -- 'basic', 'standard', 'premium'
    name            VARCHAR(100) NOT NULL,
    price_rub       NUMERIC(12,2) NOT NULL,
    duration_days   INT NOT NULL DEFAULT 30,
    speed_mbps      FLOAT8 NOT NULL DEFAULT 0.0,
    is_bundle       BOOLEAN NOT NULL DEFAULT FALSE,  -- "абонемент" со скидкой
    bundle_months   INT NOT NULL DEFAULT 1,
    discount_pct    FLOAT8 NOT NULL DEFAULT 0.0,
    is_active       BOOLEAN NOT NULL DEFAULT TRUE,
    sort_order      INT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Seed default subscription plans
INSERT INTO subscription_plans (plan_key, name, price_rub, duration_days, speed_mbps, is_bundle, bundle_months, discount_pct, sort_order)
VALUES
    ('basic',        'Базовый (10 Мбит/с)',           199.00, 30,  10.0, FALSE, 1, 0,   10),
    ('standard',     'Стандарт (50 Мбит/с)',          299.00, 30,  50.0, FALSE, 1, 0,   20),
    ('premium',      'Премиум (без ограничений)',      499.00, 30,   0.0, FALSE, 1, 0,   30),
    ('premium_3m',   'Премиум 3 месяца (−20%)',       1197.00, 90,  0.0, TRUE,  3, 20,  40),
    ('premium_6m',   'Премиум 6 месяцев (−30%)',      2094.00, 180, 0.0, TRUE,  6, 30,  50),
    ('premium_12m',  'Премиум 12 месяцев (−40%)',     3588.00, 365, 0.0, TRUE, 12, 40,  60)
ON CONFLICT (plan_key) DO NOTHING;

-- App versions (for download links)
CREATE TABLE IF NOT EXISTS app_versions (
    id              SERIAL PRIMARY KEY,
    platform        VARCHAR(20) NOT NULL UNIQUE, -- 'windows', 'linux', 'android', 'macos'
    version         VARCHAR(20) NOT NULL,
    download_url    TEXT NOT NULL,
    release_notes   TEXT,
    released_at     TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

-- Indexes for performance
CREATE INDEX IF NOT EXISTS idx_payments_user_id     ON payments(user_id);
CREATE INDEX IF NOT EXISTS idx_payments_status      ON payments(status);
CREATE INDEX IF NOT EXISTS idx_payments_tochka      ON payments(tochka_order_id);
CREATE INDEX IF NOT EXISTS idx_ref_earnings_referrer ON referral_earnings(referrer_id);
CREATE INDEX IF NOT EXISTS idx_withdrawals_user     ON withdrawal_requests(user_id);
CREATE INDEX IF NOT EXISTS idx_withdrawals_status   ON withdrawal_requests(status);
