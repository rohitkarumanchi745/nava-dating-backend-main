-- =============================================================================
-- Migration 018: Reel seek signals + location-based feed ranking
-- Adds: seek_forward_count, seek_backward_count, pause_count to reel_views
--       and reel_engagement_events for ML training
-- =============================================================================

-- Add seek/pause tracking to reel_views
ALTER TABLE reel_views
    ADD COLUMN IF NOT EXISTS seek_forward_count  INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS seek_backward_count INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS pause_count         INT NOT NULL DEFAULT 0;

-- Add seek/pause tracking to reel_engagement_events (ML training log)
ALTER TABLE reel_engagement_events
    ADD COLUMN IF NOT EXISTS seek_forward_count  INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS seek_backward_count INT NOT NULL DEFAULT 0,
    ADD COLUMN IF NOT EXISTS pause_count         INT NOT NULL DEFAULT 0;

-- Add city to reels so feed can do location-based boosting without joining users
ALTER TABLE reels
    ADD COLUMN IF NOT EXISTS creator_city TEXT;

-- Backfill creator_city from users table
UPDATE reels r
SET creator_city = u.city
FROM users u
WHERE u.id = r.user_id AND u.city IS NOT NULL;

-- Index for location-filtered reel queries
CREATE INDEX IF NOT EXISTS idx_reels_creator_city ON reels(creator_city)
    WHERE creator_city IS NOT NULL;
