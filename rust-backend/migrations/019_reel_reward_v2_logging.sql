-- =============================================================================
-- Migration 019: Reel reward v2 — precise logging fields + reward versioning
-- =============================================================================

-- Extended fields for reel_engagement_events (ML training log)
ALTER TABLE reel_engagement_events
    ADD COLUMN IF NOT EXISTS watch_duration_ms BIGINT  NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS same_city         BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS liked             BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS messaged          BOOLEAN NOT NULL DEFAULT FALSE,
    ADD COLUMN IF NOT EXISTS message_length    INT     NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS reward_version    TEXT    NOT NULL DEFAULT 'v2';

-- Index for querying by reward version during shadow-bucket analysis
CREATE INDEX IF NOT EXISTS idx_reel_events_reward_version
    ON reel_engagement_events(reward_version, created_at DESC);

-- Index for daily reward distribution monitoring (p50/p90/p99)
CREATE INDEX IF NOT EXISTS idx_reel_events_created_reward
    ON reel_engagement_events(created_at DESC, reward);
