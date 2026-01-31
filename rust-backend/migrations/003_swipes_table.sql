-- Migration: Create swipes table for core swiping functionality and Neo4j sync
-- This table is required for the dating app core functionality

-- ============================================================================
-- Swipes Table (Core Dating Functionality)
-- ============================================================================
CREATE TABLE IF NOT EXISTS swipes (
    id BIGSERIAL PRIMARY KEY,
    from_user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    to_user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    action VARCHAR(20) NOT NULL,  -- 'like', 'pass', 'superlike', 'block'
    source VARCHAR(30) DEFAULT 'discover',  -- 'discover', 'reel', 'profile', 'playground'
    created_at TIMESTAMP DEFAULT NOW(),
    CONSTRAINT unique_swipe UNIQUE (from_user_id, to_user_id)
);

-- Indexes for efficient querying
CREATE INDEX IF NOT EXISTS idx_swipes_from_user ON swipes(from_user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_swipes_to_user ON swipes(to_user_id, action);
CREATE INDEX IF NOT EXISTS idx_swipes_action ON swipes(action, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_swipes_mutual ON swipes(from_user_id, to_user_id, action);

-- ============================================================================
-- User Profiles View (Compatibility layer for sync)
-- Maps users table to expected user_profiles interface
-- ============================================================================
CREATE OR REPLACE VIEW user_profiles AS
SELECT
    id,
    phone_number,
    email,
    name,
    dob as date_of_birth,
    gender,
    bio,
    location_text,
    interests,
    languages,
    looking_for,
    profession_category,
    profession_title,
    height_cm,
    profile_photo_url,
    profile_photos,
    voice_intro_url,
    is_active,
    is_verified,
    is_profile_complete,
    university,
    student_tier,
    last_active,
    attractiveness_score,
    created_at,
    updated_at
FROM users;
