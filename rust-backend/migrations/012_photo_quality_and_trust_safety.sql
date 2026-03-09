-- =============================================================================
-- Migration 012: Photo Quality, Trust & Safety, Content Moderation,
--                Freshness, Media Optimization, Moderation Transparency
-- =============================================================================

-- Photo quality log for audit
CREATE TABLE IF NOT EXISTS photo_quality_log (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    photo_slot VARCHAR(30) NOT NULL,
    composite_score DOUBLE PRECISION NOT NULL,
    blur_score DOUBLE PRECISION NOT NULL,
    low_light_score DOUBLE PRECISION NOT NULL,
    face_ratio DOUBLE PRECISION NOT NULL,
    flags JSONB DEFAULT '[]',
    created_at TIMESTAMP DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_photo_quality_user ON photo_quality_log(user_id);

-- Device fingerprints for multi-account / ban evasion detection
CREATE TABLE IF NOT EXISTS device_fingerprints (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    fingerprint_hash VARCHAR(64) NOT NULL,
    features JSONB DEFAULT '{}',
    first_seen TIMESTAMP DEFAULT NOW(),
    last_seen TIMESTAMP DEFAULT NOW(),
    is_suspicious BOOLEAN DEFAULT FALSE,
    UNIQUE(user_id, fingerprint_hash)
);
CREATE INDEX IF NOT EXISTS idx_device_fp_hash ON device_fingerprints(fingerprint_hash);
CREATE INDEX IF NOT EXISTS idx_device_fp_user ON device_fingerprints(user_id);

-- Trust & safety events for audit trail
CREATE TABLE IF NOT EXISTS trust_safety_events (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    risk_score DOUBLE PRECISION NOT NULL,
    signals JSONB DEFAULT '[]',
    action_taken VARCHAR(30) NOT NULL,
    device_fingerprint_hash VARCHAR(64),
    trace_id VARCHAR(64),
    created_at TIMESTAMP DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_trust_events_user ON trust_safety_events(user_id);
CREATE INDEX IF NOT EXISTS idx_trust_events_action ON trust_safety_events(action_taken);

-- Moderation decisions (content moderation audit trail)
CREATE TABLE IF NOT EXISTS moderation_decisions (
    id BIGSERIAL PRIMARY KEY,
    trace_id VARCHAR(64) NOT NULL,
    user_id BIGINT NOT NULL,
    content_type VARCHAR(30) NOT NULL,
    content_id VARCHAR(100),
    action VARCHAR(30) NOT NULL,
    reason_code VARCHAR(30) NOT NULL,
    user_facing_reason TEXT,
    confidence DOUBLE PRECISION DEFAULT 0.0,
    model_scores JSONB DEFAULT '{}',
    created_at TIMESTAMP DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_moderation_user ON moderation_decisions(user_id);
CREATE INDEX IF NOT EXISTS idx_moderation_trace ON moderation_decisions(trace_id);
CREATE INDEX IF NOT EXISTS idx_moderation_reason ON moderation_decisions(reason_code);

-- Moderation appeals
CREATE TABLE IF NOT EXISTS moderation_appeals (
    id BIGSERIAL PRIMARY KEY,
    decision_id BIGINT NOT NULL REFERENCES moderation_decisions(id),
    user_id BIGINT NOT NULL,
    appeal_reason TEXT NOT NULL,
    status VARCHAR(20) DEFAULT 'pending',
    admin_user_id BIGINT,
    admin_notes TEXT,
    submitted_at TIMESTAMP DEFAULT NOW(),
    resolved_at TIMESTAMP
);
CREATE INDEX IF NOT EXISTS idx_appeals_user ON moderation_appeals(user_id);
CREATE INDEX IF NOT EXISTS idx_appeals_status ON moderation_appeals(status);

-- Freshness scoring
ALTER TABLE users ADD COLUMN IF NOT EXISTS freshness_score DOUBLE PRECISION DEFAULT 1.0;
ALTER TABLE users ADD COLUMN IF NOT EXISTS last_photo_updated_at TIMESTAMP;

-- Profile edit rate limiting log
CREATE TABLE IF NOT EXISTS profile_edit_log (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    edit_type VARCHAR(30) NOT NULL,
    edited_at TIMESTAMP DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_edit_log_user_time ON profile_edit_log(user_id, edited_at);

-- Media renditions storage
CREATE TABLE IF NOT EXISTS media_renditions (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    original_key VARCHAR(500) NOT NULL,
    renditions JSONB NOT NULL DEFAULT '{}',
    created_at TIMESTAMP DEFAULT NOW()
);
CREATE INDEX IF NOT EXISTS idx_renditions_user ON media_renditions(user_id);
CREATE INDEX IF NOT EXISTS idx_renditions_key ON media_renditions(original_key);

-- Photo renditions on user table (JSONB for quick access)
ALTER TABLE users ADD COLUMN IF NOT EXISTS photo_renditions JSONB;

-- Auto-clean old profile edit logs (older than 7 days)
-- Run via pg_cron or manual cleanup job
