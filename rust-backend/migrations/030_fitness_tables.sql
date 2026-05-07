-- Fitness schema. Reconstructed from the read/write paths in
-- handlers/mod.rs (sync_fitness, get_fitness_*, *_fitness_challenge*).
-- These tables existed in production but had no migration of record;
-- a fresh environment couldn't boot the fitness handlers without errors.
-- All statements are idempotent so this is safe to run on existing DBs.

-- HealthKit / sync source of truth — one row per workout.
CREATE TABLE IF NOT EXISTS user_fitness_activities (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    activity_type TEXT NOT NULL,
    calories_burned DOUBLE PRECISION,
    duration_min DOUBLE PRECISION,
    distance_km DOUBLE PRECISION,
    elevation_gain_m DOUBLE PRECISION,
    avg_heart_rate INTEGER,
    location_name TEXT,
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    started_at TIMESTAMP NOT NULL DEFAULT NOW(),
    source TEXT
);

-- Per-user activity feed is the dominant read shape.
CREATE INDEX IF NOT EXISTS idx_user_fitness_activities_user_started
    ON user_fitness_activities (user_id, started_at DESC);

-- Aggregated rollups recomputed on each sync_fitness write.
-- user_id is the PK because writes use ON CONFLICT (user_id) DO UPDATE.
CREATE TABLE IF NOT EXISTS user_fitness_profile (
    user_id BIGINT PRIMARY KEY,
    weekly_active_minutes INTEGER NOT NULL DEFAULT 0,
    weekly_calories INTEGER NOT NULL DEFAULT 0,
    weekly_workouts INTEGER NOT NULL DEFAULT 0,
    total_distance_km DOUBLE PRECISION NOT NULL DEFAULT 0,
    streak_days INTEGER NOT NULL DEFAULT 0,
    favorite_activity TEXT,
    fitness_level TEXT,
    last_workout_at TIMESTAMP,
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- Leaderboard sorts by these counters (NULLS LAST), per metric.
CREATE INDEX IF NOT EXISTS idx_user_fitness_profile_calories
    ON user_fitness_profile (weekly_calories DESC NULLS LAST);
CREATE INDEX IF NOT EXISTS idx_user_fitness_profile_active_minutes
    ON user_fitness_profile (weekly_active_minutes DESC NULLS LAST);
CREATE INDEX IF NOT EXISTS idx_user_fitness_profile_workouts
    ON user_fitness_profile (weekly_workouts DESC NULLS LAST);
CREATE INDEX IF NOT EXISTS idx_user_fitness_profile_streak
    ON user_fitness_profile (streak_days DESC NULLS LAST);

-- Two-person fitness challenges (creator + partner).
CREATE TABLE IF NOT EXISTS fitness_challenges (
    id BIGSERIAL PRIMARY KEY,
    creator_id BIGINT NOT NULL,
    partner_id BIGINT,
    challenge_type TEXT NOT NULL,
    target_value DOUBLE PRECISION NOT NULL,
    target_unit TEXT NOT NULL,
    creator_progress DOUBLE PRECISION NOT NULL DEFAULT 0,
    partner_progress DOUBLE PRECISION NOT NULL DEFAULT 0,
    ends_at TIMESTAMP,
    status TEXT NOT NULL DEFAULT 'active',
    created_at TIMESTAMP NOT NULL DEFAULT NOW()
);

-- get_fitness_challenges filters on (creator_id|partner_id, status='active').
CREATE INDEX IF NOT EXISTS idx_fitness_challenges_creator_active
    ON fitness_challenges (creator_id, status, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_fitness_challenges_partner_active
    ON fitness_challenges (partner_id, status, created_at DESC)
    WHERE partner_id IS NOT NULL;

-- Mirrors the inline CREATE TABLE in get_fitness_goals so fresh
-- environments don't depend on that handler being hit first.
CREATE TABLE IF NOT EXISTS user_fitness_goals (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    goal_type TEXT NOT NULL,
    target_value DOUBLE PRECISION NOT NULL,
    unit TEXT NOT NULL,
    period TEXT NOT NULL DEFAULT 'weekly',
    is_active BOOLEAN NOT NULL DEFAULT TRUE,
    created_at TIMESTAMP NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMP NOT NULL DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_user_fitness_goals_user
    ON user_fitness_goals (user_id, is_active);
