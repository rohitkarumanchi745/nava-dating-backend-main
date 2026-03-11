-- =============================================================================
-- Migration 013: Class year filter + New in city feature
-- =============================================================================

-- 1. Add class_year alias column (graduation_year already exists, add class_year as generated)
ALTER TABLE student_verifications
    ADD COLUMN IF NOT EXISTS class_year integer
        GENERATED ALWAYS AS (graduation_year) STORED;

-- Index for class year filtering in alumni/student search
CREATE INDEX IF NOT EXISTS idx_sv_class_year
    ON student_verifications (class_year, status)
    WHERE class_year IS NOT NULL;

-- Index for alumni filtering
CREATE INDEX IF NOT EXISTS idx_sv_alumni
    ON student_verifications (is_alumni, university_id, status)
    WHERE is_alumni = true;

-- 2. Add new-in-city fields to users
ALTER TABLE users
    ADD COLUMN IF NOT EXISTS city_arrival_date  date,
    ADD COLUMN IF NOT EXISTS current_city       varchar(100),
    ADD COLUMN IF NOT EXISTS is_new_in_city     boolean NOT NULL DEFAULT false;

-- Auto-compute is_new_in_city: true if city_arrival_date is within last 60 days
-- This is kept as a plain column and updated by the app on login/profile update.
-- A partial index makes the discover filter fast.
CREATE INDEX IF NOT EXISTS idx_users_new_in_city
    ON users (is_new_in_city, current_city)
    WHERE is_new_in_city = true;
