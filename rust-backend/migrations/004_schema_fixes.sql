-- Migration: Schema fixes for sync compatibility
-- Adds missing columns and fixes views for Neo4j sync

-- ============================================================================
-- Add missing columns to universities table
-- ============================================================================
ALTER TABLE universities
ADD COLUMN IF NOT EXISTS email_domain VARCHAR(255),
ADD COLUMN IF NOT EXISTS university_type VARCHAR(50);

-- Update email_domain from domain column
UPDATE universities SET email_domain = domain WHERE email_domain IS NULL;

-- Update university_type from tier (normalize naming)
UPDATE universities SET university_type = tier WHERE university_type IS NULL;

-- ============================================================================
-- Drop and recreate user_profiles view with user_id alias
-- ============================================================================
DROP VIEW IF EXISTS user_profiles;

CREATE VIEW user_profiles AS
SELECT
    id,
    id as user_id,  -- Alias for sync compatibility
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
