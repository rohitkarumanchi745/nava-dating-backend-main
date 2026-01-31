-- =============================================================================
-- NAVA Backend - Ambassador Authentication Schema
-- =============================================================================
-- Adds password authentication for ambassador dashboard login

-- Add password_hash to users table for ambassador login
ALTER TABLE users ADD COLUMN IF NOT EXISTS password_hash VARCHAR(255);
ALTER TABLE users ADD COLUMN IF NOT EXISTS last_active_at TIMESTAMP;

-- Create index for email lookups (ambassador login)
CREATE INDEX IF NOT EXISTS idx_users_email ON users(email);

-- Update ambassador table to ensure we have all needed fields
ALTER TABLE ambassadors ADD COLUMN IF NOT EXISTS email VARCHAR(255);
ALTER TABLE ambassadors ADD COLUMN IF NOT EXISTS phone VARCHAR(20);

-- Create ambassador_sessions table for tracking login sessions
CREATE TABLE IF NOT EXISTS ambassador_sessions (
    id SERIAL PRIMARY KEY,
    ambassador_id INT REFERENCES ambassadors(id) ON DELETE CASCADE,
    user_id INT REFERENCES users(id) ON DELETE CASCADE,
    session_token VARCHAR(255) UNIQUE NOT NULL,
    refresh_token VARCHAR(255) UNIQUE,
    ip_address VARCHAR(45),
    user_agent TEXT,
    is_active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMP DEFAULT NOW(),
    expires_at TIMESTAMP NOT NULL,
    last_accessed_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_ambassador_sessions_user ON ambassador_sessions(user_id);
CREATE INDEX IF NOT EXISTS idx_ambassador_sessions_token ON ambassador_sessions(session_token);
CREATE INDEX IF NOT EXISTS idx_ambassador_sessions_active ON ambassador_sessions(is_active, expires_at);

-- Create subscriptions table if it doesn't exist (for subscriber tracking)
CREATE TABLE IF NOT EXISTS subscriptions (
    id SERIAL PRIMARY KEY,
    user_id INT REFERENCES users(id) ON DELETE CASCADE,
    tier VARCHAR(50) NOT NULL DEFAULT 'free',  -- 'free', 'gold', 'platinum', 'ultra'
    status VARCHAR(50) NOT NULL DEFAULT 'active',  -- 'active', 'cancelled', 'expired'
    started_at TIMESTAMP DEFAULT NOW(),
    expires_at TIMESTAMP,
    cancelled_at TIMESTAMP,
    gateway VARCHAR(50),  -- 'razorpay', 'stripe', 'revenuecat'
    gateway_subscription_id VARCHAR(255),
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_subscriptions_user ON subscriptions(user_id);
CREATE INDEX IF NOT EXISTS idx_subscriptions_status ON subscriptions(status);

-- =============================================================================
-- AMBASSADOR HOURLY ANALYTICS VIEW
-- =============================================================================

-- Hourly signup breakdown for ambassador dashboard
CREATE OR REPLACE VIEW v_ambassador_hourly_signups AS
SELECT
    DATE(ru.created_at) as signup_date,
    EXTRACT(HOUR FROM ru.created_at)::int as signup_hour,
    a.id as ambassador_id,
    a.user_id as ambassador_user_id,
    u.name as ambassador_name,
    COUNT(*) as total_signups,
    COUNT(*) FILTER (WHERE ru.status = 'verified') as verified_signups,
    COUNT(*) FILTER (WHERE ru.status = 'pending') as pending_signups
FROM ambassadors a
JOIN users u ON a.user_id = u.id
LEFT JOIN referral_codes rc ON rc.user_id = a.user_id AND rc.code_type = 'ambassador'
LEFT JOIN referral_uses ru ON ru.referral_code_id = rc.id
WHERE a.status = 'active' AND ru.created_at IS NOT NULL
GROUP BY DATE(ru.created_at), EXTRACT(HOUR FROM ru.created_at), a.id, a.user_id, u.name
ORDER BY signup_date DESC, signup_hour;

-- =============================================================================
-- SUBSCRIBER TRACKING VIEW
-- =============================================================================

-- Subscribers view for ambassador dashboard
CREATE OR REPLACE VIEW v_ambassador_subscribers AS
SELECT
    ru.id as referral_id,
    rc.user_id as ambassador_user_id,
    a.id as ambassador_id,
    u.id as subscriber_id,
    u.name as subscriber_name,
    u.email as subscriber_email,
    u.phone_number as subscriber_phone,
    ru.status as referral_status,
    ru.created_at as signed_up_at,
    ru.verified_at,
    s.tier as subscription_tier,
    s.status as subscription_status,
    u.last_active_at,
    CASE
        WHEN u.last_active_at > NOW() - INTERVAL '15 minutes' THEN TRUE
        ELSE FALSE
    END as is_online,
    CASE
        WHEN u.last_active_at > NOW() - INTERVAL '24 hours' THEN TRUE
        ELSE FALSE
    END as is_active_today
FROM referral_uses ru
JOIN referral_codes rc ON ru.referral_code_id = rc.id
JOIN ambassadors a ON a.user_id = rc.user_id
JOIN users u ON ru.referee_user_id = u.id
LEFT JOIN subscriptions s ON s.user_id = u.id AND s.status = 'active'
ORDER BY ru.created_at DESC;

-- Active subscribers (online in last 15 minutes)
CREATE OR REPLACE VIEW v_ambassador_active_subscribers AS
SELECT
    rc.user_id as ambassador_user_id,
    a.id as ambassador_id,
    u.id as subscriber_id,
    u.name as subscriber_name,
    u.email as subscriber_email,
    ru.status as referral_status,
    ru.created_at as signed_up_at,
    s.tier as subscription_tier,
    u.last_active_at,
    NOW() - u.last_active_at as time_since_active
FROM referral_uses ru
JOIN referral_codes rc ON ru.referral_code_id = rc.id
JOIN ambassadors a ON a.user_id = rc.user_id
JOIN users u ON ru.referee_user_id = u.id
LEFT JOIN subscriptions s ON s.user_id = u.id AND s.status = 'active'
WHERE u.last_active_at > NOW() - INTERVAL '15 minutes'
ORDER BY u.last_active_at DESC;

-- =============================================================================
-- SAMPLE DATA FOR TESTING (comment out in production)
-- =============================================================================

-- Create a test ambassador if none exists
-- INSERT INTO users (phone_number, email, name, password_hash, is_active)
-- VALUES ('+919999999999', 'ambassador@nava.app', 'Test Ambassador',
--         '$2b$12$LQv3c1yqBWVHxkd0LHAkCOYz6TtxMQJqhN8/X4BoL3e.8wZ7HlqLW', TRUE)
-- ON CONFLICT (phone_number) DO NOTHING;

-- INSERT INTO ambassadors (user_id, tier, university, city, status)
-- SELECT id, 'campus', 'Test University', 'Hyderabad', 'active'
-- FROM users WHERE email = 'ambassador@nava.app'
-- ON CONFLICT (user_id) DO NOTHING;

-- INSERT INTO referral_codes (user_id, code, code_type)
-- SELECT id, 'TEST2024', 'ambassador'
-- FROM users WHERE email = 'ambassador@nava.app'
-- ON CONFLICT (code) DO NOTHING;
