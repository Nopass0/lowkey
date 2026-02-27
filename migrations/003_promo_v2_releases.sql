-- 003_promo_v2_releases.sql
-- Enhanced promo code system and app release management

-- ── Promo code enhancements ───────────────────────────────────────────────────
-- New columns add fine-grained conditions and combo effects

ALTER TABLE promo_codes
    -- Target a specific user (individual/one-time gift code)
    ADD COLUMN IF NOT EXISTS target_user_id   INT     REFERENCES users(id) ON DELETE SET NULL,
    -- Restrict to users who haven't subscribed yet (first-purchase promos)
    ADD COLUMN IF NOT EXISTS only_new_users   BOOLEAN NOT NULL DEFAULT FALSE,
    -- Minimum purchase amount required for 'discount' type promos
    ADD COLUMN IF NOT EXISTS min_purchase_rub NUMERIC(12,2),
    -- Combo: apply a second effect together with the primary one
    -- e.g. type='balance' + second_type='free_days' gives both cash and time
    ADD COLUMN IF NOT EXISTS second_type      VARCHAR(20),
    ADD COLUMN IF NOT EXISTS second_value     FLOAT8  NOT NULL DEFAULT 0,
    -- Per-user redemption limit (default 1, -1 = unlimited per user)
    ADD COLUMN IF NOT EXISTS max_uses_per_user INT    NOT NULL DEFAULT 1,
    -- Human-readable description shown in admin panel
    ADD COLUMN IF NOT EXISTS description      TEXT,
    -- Who created this promo (audit log)
    ADD COLUMN IF NOT EXISTS created_by       VARCHAR(50) NOT NULL DEFAULT 'admin';

-- Extend the type CHECK constraint to include new combo type
ALTER TABLE promo_codes DROP CONSTRAINT IF EXISTS promo_codes_type_check;
ALTER TABLE promo_codes
    ADD CONSTRAINT promo_codes_type_check
    CHECK (type IN ('balance','discount','free_days','speed','subscription','combo'));

-- Add timestamp to promo_uses for analytics
ALTER TABLE promo_uses
    ADD COLUMN IF NOT EXISTS used_at TIMESTAMPTZ NOT NULL DEFAULT NOW();

-- ── App releases table ────────────────────────────────────────────────────────
-- Stores released versions of client apps. The latest release per platform
-- is served by GET /api/version/:platform for auto-update checks.

CREATE TABLE IF NOT EXISTS app_releases (
    id              SERIAL PRIMARY KEY,
    platform        VARCHAR(20)  NOT NULL,           -- 'windows' | 'linux' | 'android' | 'macos'
    version         VARCHAR(20)  NOT NULL,           -- semver e.g. "1.2.3"
    -- Semantic version as integer tuple for easy comparison
    version_major   INT          NOT NULL DEFAULT 0,
    version_minor   INT          NOT NULL DEFAULT 0,
    version_patch   INT          NOT NULL DEFAULT 0,
    is_latest       BOOLEAN      NOT NULL DEFAULT FALSE,
    is_active       BOOLEAN      NOT NULL DEFAULT TRUE,
    download_url    TEXT         NOT NULL,           -- direct download link
    file_name       VARCHAR(200),                    -- original filename shown to user
    file_size_bytes BIGINT,                          -- for display in downloads page
    sha256_checksum VARCHAR(64),                     -- optional integrity check
    changelog       TEXT,                            -- markdown release notes
    min_os_version  VARCHAR(20),                     -- e.g. "10" for Windows 10
    released_at     TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    created_at      TIMESTAMPTZ  NOT NULL DEFAULT NOW(),
    UNIQUE(platform, version)
);

-- Only one release per platform can be marked as latest
CREATE UNIQUE INDEX IF NOT EXISTS idx_releases_latest
    ON app_releases(platform) WHERE is_latest = TRUE;

CREATE INDEX IF NOT EXISTS idx_releases_platform    ON app_releases(platform);
CREATE INDEX IF NOT EXISTS idx_releases_version     ON app_releases(platform, version_major DESC, version_minor DESC, version_patch DESC);

-- ── Promo analytics view ──────────────────────────────────────────────────────
-- Convenience view for admin dashboard

CREATE OR REPLACE VIEW promo_usage_stats AS
SELECT
    pc.id,
    pc.code,
    pc.type,
    pc.value,
    pc.used_count,
    pc.max_uses,
    pc.expires_at,
    pc.target_user_id,
    pc.only_new_users,
    pc.description,
    COUNT(pu.id)                              AS actual_uses,
    MAX(pu.used_at)                           AS last_used_at
FROM promo_codes pc
LEFT JOIN promo_uses pu ON pu.promo_id = pc.id
GROUP BY pc.id;
