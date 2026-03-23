-- =============================================================================
-- NAVA Backend - Payments & Ads Schema
-- =============================================================================
-- Multi-gateway payment support (Razorpay India, Stripe Global)
-- Ad monetization for free users

-- =============================================================================
-- PAYMENT TABLES
-- =============================================================================

-- Payment gateway configuration per region
CREATE TABLE IF NOT EXISTS payment_gateways (
    id SERIAL PRIMARY KEY,
    region_code VARCHAR(10) NOT NULL,        -- 'IN', 'US', 'EU', 'GLOBAL'
    gateway_name VARCHAR(50) NOT NULL,        -- 'razorpay', 'stripe', 'paypal'
    is_active BOOLEAN DEFAULT TRUE,
    priority INT DEFAULT 1,                   -- Lower = higher priority
    config JSONB DEFAULT '{}',                -- Gateway-specific config
    created_at TIMESTAMP DEFAULT NOW(),
    UNIQUE(region_code, gateway_name)
);

-- Insert default gateway configuration
INSERT INTO payment_gateways (region_code, gateway_name, priority) VALUES
    ('IN', 'razorpay', 1),
    ('IN', 'stripe', 2),
    ('US', 'stripe', 1),
    ('EU', 'stripe', 1),
    ('GB', 'stripe', 1),
    ('GLOBAL', 'stripe', 1)
ON CONFLICT (region_code, gateway_name) DO NOTHING;

-- Products/Plans catalog
CREATE TABLE IF NOT EXISTS products (
    id SERIAL PRIMARY KEY,
    product_id VARCHAR(100) UNIQUE NOT NULL,  -- 'premium_monthly', 'boost_5pack'
    product_type VARCHAR(50) NOT NULL,        -- 'subscription', 'consumable', 'boost'
    name VARCHAR(255) NOT NULL,
    description TEXT,
    is_active BOOLEAN DEFAULT TRUE,
    features JSONB DEFAULT '[]',              -- List of features included
    sort_order INT DEFAULT 0,                 -- For UI ordering
    duration_days INT,                        -- Subscription duration
    consumable_quantity INT,                  -- Quantity for consumable products
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

-- Regional pricing (supports multiple currencies)
CREATE TABLE IF NOT EXISTS product_prices (
    id SERIAL PRIMARY KEY,
    product_id INT REFERENCES products(id),
    region_code VARCHAR(10) NOT NULL,         -- 'IN', 'US', 'EU'
    currency VARCHAR(3) NOT NULL,             -- 'INR', 'USD', 'EUR'
    amount_cents INT NOT NULL,                -- Price in smallest unit
    stripe_price_id VARCHAR(100),             -- Stripe Price ID
    razorpay_plan_id VARCHAR(100),            -- Razorpay Plan ID
    discount_percent INT DEFAULT 0,           -- Student discount etc.
    is_active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMP DEFAULT NOW(),
    UNIQUE(product_id, region_code, currency)
);

-- Insert default products
INSERT INTO products (product_id, product_type, name, description, features) VALUES
    ('free', 'subscription', 'Free', 'Basic access', '["10_likes_daily", "basic_filters"]'),
    ('gold_monthly', 'subscription', 'Gold Monthly', 'Unlimited likes & more', '["unlimited_likes", "see_who_likes", "5_super_likes_daily"]'),
    ('platinum_monthly', 'subscription', 'Platinum Monthly', 'Premium features', '["unlimited_likes", "see_who_likes", "unlimited_super_likes", "priority_matching", "read_receipts"]'),
    ('ultra_monthly', 'subscription', 'Ultra Monthly', 'All features', '["all_platinum", "verified_badge", "concierge_support", "profile_boost_weekly"]'),
    ('boost_1', 'consumable', 'Single Boost', '30 min profile boost', '["30_min_boost"]'),
    ('boost_5', 'consumable', '5 Boosts Pack', '5 profile boosts', '["5_boosts"]'),
    ('super_like_5', 'consumable', '5 Super Likes', 'Stand out pack', '["5_super_likes"]'),
    ('spotlight_1hr', 'consumable', '1 Hour Spotlight', 'Featured in discover', '["1_hour_spotlight"]')
ON CONFLICT (product_id) DO NOTHING;

-- Insert pricing for India (INR)
INSERT INTO product_prices (product_id, region_code, currency, amount_cents) VALUES
    ((SELECT id FROM products WHERE product_id = 'gold_monthly'), 'IN', 'INR', 49900),        -- ₹499
    ((SELECT id FROM products WHERE product_id = 'platinum_monthly'), 'IN', 'INR', 199900),  -- ₹1,999
    ((SELECT id FROM products WHERE product_id = 'ultra_monthly'), 'IN', 'INR', 499900),     -- ₹4,999
    ((SELECT id FROM products WHERE product_id = 'boost_1'), 'IN', 'INR', 19900),            -- ₹199
    ((SELECT id FROM products WHERE product_id = 'boost_5'), 'IN', 'INR', 79900),            -- ₹799
    ((SELECT id FROM products WHERE product_id = 'super_like_5'), 'IN', 'INR', 29900),       -- ₹299
    ((SELECT id FROM products WHERE product_id = 'spotlight_1hr'), 'IN', 'INR', 49900)       -- ₹499
ON CONFLICT (product_id, region_code, currency) DO NOTHING;

-- Insert pricing for US (USD)
INSERT INTO product_prices (product_id, region_code, currency, amount_cents) VALUES
    ((SELECT id FROM products WHERE product_id = 'gold_monthly'), 'US', 'USD', 999),         -- $9.99
    ((SELECT id FROM products WHERE product_id = 'platinum_monthly'), 'US', 'USD', 2499),    -- $24.99
    ((SELECT id FROM products WHERE product_id = 'ultra_monthly'), 'US', 'USD', 4999),       -- $49.99
    ((SELECT id FROM products WHERE product_id = 'boost_1'), 'US', 'USD', 299),              -- $2.99
    ((SELECT id FROM products WHERE product_id = 'boost_5'), 'US', 'USD', 999),              -- $9.99
    ((SELECT id FROM products WHERE product_id = 'super_like_5'), 'US', 'USD', 499),         -- $4.99
    ((SELECT id FROM products WHERE product_id = 'spotlight_1hr'), 'US', 'USD', 699)         -- $6.99
ON CONFLICT (product_id, region_code, currency) DO NOTHING;

-- Insert pricing for EU (EUR)
INSERT INTO product_prices (product_id, region_code, currency, amount_cents) VALUES
    ((SELECT id FROM products WHERE product_id = 'gold_monthly'), 'EU', 'EUR', 899),         -- €8.99
    ((SELECT id FROM products WHERE product_id = 'platinum_monthly'), 'EU', 'EUR', 2299),    -- €22.99
    ((SELECT id FROM products WHERE product_id = 'ultra_monthly'), 'EU', 'EUR', 4499),       -- €44.99
    ((SELECT id FROM products WHERE product_id = 'boost_1'), 'EU', 'EUR', 279),              -- €2.79
    ((SELECT id FROM products WHERE product_id = 'boost_5'), 'EU', 'EUR', 899),              -- €8.99
    ((SELECT id FROM products WHERE product_id = 'super_like_5'), 'EU', 'EUR', 449),         -- €4.49
    ((SELECT id FROM products WHERE product_id = 'spotlight_1hr'), 'EU', 'EUR', 649)         -- €6.49
ON CONFLICT (product_id, region_code, currency) DO NOTHING;

-- Payment orders (initiated payments)
CREATE TABLE IF NOT EXISTS payment_orders (
    id SERIAL PRIMARY KEY,
    order_id VARCHAR(100) UNIQUE NOT NULL,    -- Our internal order ID
    user_id INT REFERENCES users(id),
    product_id INT REFERENCES products(id),

    -- Amount details
    amount_cents INT NOT NULL,
    currency VARCHAR(3) NOT NULL,
    discount_cents INT DEFAULT 0,
    final_amount_cents INT NOT NULL,

    -- Gateway details
    gateway VARCHAR(50) NOT NULL,             -- 'razorpay', 'stripe'
    gateway_order_id VARCHAR(255),            -- Razorpay order_id / Stripe payment_intent
    gateway_status VARCHAR(50),

    -- Status tracking
    status VARCHAR(50) DEFAULT 'created',     -- created, pending, completed, failed, refunded

    -- Metadata
    region_code VARCHAR(10),
    platform VARCHAR(20),                     -- 'web', 'ios', 'android'
    ip_address VARCHAR(45),
    user_agent TEXT,
    metadata JSONB DEFAULT '{}',

    -- Timestamps
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW(),
    completed_at TIMESTAMP,
    expires_at TIMESTAMP
);

CREATE INDEX idx_payment_orders_user ON payment_orders(user_id);
CREATE INDEX idx_payment_orders_status ON payment_orders(status);
CREATE INDEX idx_payment_orders_gateway ON payment_orders(gateway, gateway_order_id);
CREATE INDEX idx_payment_orders_created ON payment_orders(created_at);

-- Payment transactions (completed payments)
CREATE TABLE IF NOT EXISTS payment_transactions (
    id SERIAL PRIMARY KEY,
    transaction_id VARCHAR(100) UNIQUE NOT NULL,
    order_id INT REFERENCES payment_orders(id),
    user_id INT REFERENCES users(id),

    -- Amount
    amount_cents INT NOT NULL,
    currency VARCHAR(3) NOT NULL,
    gateway_fee_cents INT DEFAULT 0,
    net_amount_cents INT NOT NULL,

    -- Gateway details
    gateway VARCHAR(50) NOT NULL,
    gateway_payment_id VARCHAR(255),          -- Razorpay payment_id / Stripe payment_intent_id
    gateway_transaction_id VARCHAR(255),      -- Razorpay payment_id / Stripe charge_id
    gateway_response JSONB,

    -- Payment method
    payment_method VARCHAR(50),               -- 'card', 'upi', 'netbanking', 'wallet'
    payment_method_details JSONB,             -- Last 4 digits, bank name, etc.

    -- Status
    status VARCHAR(50) DEFAULT 'success',     -- success, failed, refunded, disputed
    refund_amount_cents INT DEFAULT 0,

    -- Timestamps
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_payment_transactions_user ON payment_transactions(user_id);
CREATE INDEX idx_payment_transactions_gateway ON payment_transactions(gateway, gateway_transaction_id);
CREATE INDEX idx_payment_transactions_date ON payment_transactions(created_at);

-- Subscriptions (active user subscriptions)
CREATE TABLE IF NOT EXISTS user_subscriptions (
    id SERIAL PRIMARY KEY,
    subscription_id VARCHAR(100) UNIQUE NOT NULL,  -- Our internal subscription ID
    user_id INT REFERENCES users(id),
    product_id INT REFERENCES products(id),

    -- Subscription details
    status VARCHAR(50) DEFAULT 'active',      -- active, cancelled, expired, paused

    -- Billing
    gateway VARCHAR(50),
    gateway_subscription_id VARCHAR(255),     -- Razorpay subscription_id / Stripe subscription_id
    current_period_start TIMESTAMP,
    current_period_end TIMESTAMP,
    cancel_at_period_end BOOLEAN DEFAULT FALSE,

    -- History
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW(),
    cancelled_at TIMESTAMP
);

CREATE INDEX idx_user_subscriptions_user ON user_subscriptions(user_id);
CREATE INDEX idx_user_subscriptions_status ON user_subscriptions(status);
CREATE INDEX idx_user_subscriptions_expiry ON user_subscriptions(current_period_end);

-- User consumables balance (boosts, super likes, etc.)
CREATE TABLE IF NOT EXISTS user_consumables (
    id SERIAL PRIMARY KEY,
    user_id INT REFERENCES users(id),
    consumable_type VARCHAR(50) NOT NULL,     -- 'boost', 'super_like', 'spotlight'
    balance INT DEFAULT 0,
    total_purchased INT DEFAULT 0,
    total_used INT DEFAULT 0,
    updated_at TIMESTAMP DEFAULT NOW(),
    UNIQUE(user_id, consumable_type)
);

CREATE INDEX idx_user_consumables_user ON user_consumables(user_id);

-- User daily bonuses (from watching rewarded ads)
CREATE TABLE IF NOT EXISTS user_daily_bonuses (
    id SERIAL PRIMARY KEY,
    user_id INT REFERENCES users(id),
    bonus_date DATE NOT NULL,
    extra_likes INT DEFAULT 0,
    extra_super_likes INT DEFAULT 0,
    boost_minutes INT DEFAULT 0,
    created_at TIMESTAMP DEFAULT NOW(),
    UNIQUE(user_id, bonus_date)
);

CREATE INDEX idx_user_daily_bonuses_user ON user_daily_bonuses(user_id);
CREATE INDEX idx_user_daily_bonuses_date ON user_daily_bonuses(bonus_date);

-- =============================================================================
-- REFERRAL & AMBASSADOR TABLES
-- =============================================================================

CREATE TABLE IF NOT EXISTS referral_codes (
    id SERIAL PRIMARY KEY,
    user_id INT REFERENCES users(id),
    code VARCHAR(20) UNIQUE NOT NULL,
    code_type VARCHAR(50) DEFAULT 'user',     -- 'user', 'ambassador', 'influencer'
    reward_type VARCHAR(50) DEFAULT 'premium_days',
    reward_value INT DEFAULT 7,               -- 7 days premium for referrer
    referee_reward_value INT DEFAULT 7,       -- 7 days premium for referee
    max_uses INT,                             -- NULL = unlimited
    current_uses INT DEFAULT 0,
    is_active BOOLEAN DEFAULT TRUE,
    expires_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_referral_codes_code ON referral_codes(code);
CREATE INDEX idx_referral_codes_user ON referral_codes(user_id);

CREATE TABLE IF NOT EXISTS referral_uses (
    id SERIAL PRIMARY KEY,
    referral_code_id INT REFERENCES referral_codes(id),
    referrer_user_id INT REFERENCES users(id),
    referee_user_id INT REFERENCES users(id) UNIQUE,  -- Each user can only be referred once
    status VARCHAR(50) DEFAULT 'pending',     -- pending, verified, rewarded
    referrer_rewarded BOOLEAN DEFAULT FALSE,
    referee_rewarded BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT NOW(),
    verified_at TIMESTAMP
);

CREATE INDEX idx_referral_uses_referrer ON referral_uses(referrer_user_id);
CREATE INDEX idx_referral_uses_referee ON referral_uses(referee_user_id);

-- Ambassador program
CREATE TABLE IF NOT EXISTS ambassadors (
    id SERIAL PRIMARY KEY,
    user_id INT REFERENCES users(id) UNIQUE,
    tier VARCHAR(50) DEFAULT 'campus',        -- 'campus', 'regional', 'city'
    university VARCHAR(255),
    city VARCHAR(100),
    region VARCHAR(100),

    -- Compensation
    monthly_stipend_cents INT DEFAULT 1000000, -- ₹10,000 default
    bonus_per_signup_cents INT DEFAULT 5000,   -- ₹50 per signup above target
    target_signups_monthly INT DEFAULT 50,

    -- Performance
    total_signups INT DEFAULT 0,
    total_verified_signups INT DEFAULT 0,
    total_earnings_cents BIGINT DEFAULT 0,

    -- Status
    status VARCHAR(50) DEFAULT 'active',      -- active, paused, terminated
    joined_at TIMESTAMP DEFAULT NOW(),
    last_payout_at TIMESTAMP
);

CREATE INDEX idx_ambassadors_user ON ambassadors(user_id);
CREATE INDEX idx_ambassadors_status ON ambassadors(status);

-- =============================================================================
-- ADS TABLES
-- =============================================================================

-- Ad placements configuration
CREATE TABLE IF NOT EXISTS ad_placements (
    id SERIAL PRIMARY KEY,
    placement_id VARCHAR(100) UNIQUE NOT NULL,
    name VARCHAR(255) NOT NULL,
    placement_type VARCHAR(50) NOT NULL,      -- 'banner', 'interstitial', 'native', 'rewarded'
    location VARCHAR(100) NOT NULL,           -- 'discover_feed', 'chat_list', 'profile_view'

    -- Ad network IDs
    admob_unit_id VARCHAR(100),
    facebook_placement_id VARCHAR(100),
    unity_placement_id VARCHAR(100),

    -- Targeting
    show_to_free_users BOOLEAN DEFAULT TRUE,
    show_to_premium_users BOOLEAN DEFAULT FALSE,
    frequency_cap_per_hour INT DEFAULT 10,

    is_active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMP DEFAULT NOW()
);

-- Insert default ad placements
INSERT INTO ad_placements (placement_id, name, placement_type, location) VALUES
    ('discover_native', 'Discover Feed Native', 'native', 'discover_feed'),
    ('chat_banner', 'Chat List Banner', 'banner', 'chat_list'),
    ('profile_interstitial', 'Profile View Interstitial', 'interstitial', 'profile_view'),
    ('boost_rewarded', 'Free Boost Rewarded Ad', 'rewarded', 'boost_screen'),
    ('superlike_rewarded', 'Free Super Like Rewarded', 'rewarded', 'superlike_screen'),
    ('match_interstitial', 'Match Screen Interstitial', 'interstitial', 'match_screen')
ON CONFLICT (placement_id) DO NOTHING;

-- Regional ad units for language-targeted ads
CREATE TABLE IF NOT EXISTS ad_placement_regional_units (
    id SERIAL PRIMARY KEY,
    placement_id VARCHAR(100) REFERENCES ad_placements(placement_id) ON DELETE CASCADE,
    language_code VARCHAR(10) NOT NULL,           -- 'te', 'hi', 'ta', 'kn', 'ml', etc.
    language_name VARCHAR(50) NOT NULL,           -- 'Telugu', 'Hindi', 'Tamil', etc.

    -- Language-specific ad network IDs
    admob_unit_id VARCHAR(100),
    facebook_placement_id VARCHAR(100),
    unity_placement_id VARCHAR(100),

    -- Regional eCPM adjustments
    ecpm_multiplier DECIMAL(3,2) DEFAULT 1.00,    -- 1.0 = base, 1.5 = 50% higher
    priority INT DEFAULT 1,                        -- Lower = higher priority

    is_active BOOLEAN DEFAULT TRUE,
    created_at TIMESTAMP DEFAULT NOW(),

    UNIQUE(placement_id, language_code)
);

CREATE INDEX idx_regional_units_placement ON ad_placement_regional_units(placement_id);
CREATE INDEX idx_regional_units_language ON ad_placement_regional_units(language_code);

-- Insert default regional ad units for Telugu (primary target)
INSERT INTO ad_placement_regional_units (placement_id, language_code, language_name, priority) VALUES
    ('discover_native', 'te', 'Telugu', 1),
    ('discover_native', 'hi', 'Hindi', 2),
    ('discover_native', 'ta', 'Tamil', 3),
    ('discover_native', 'kn', 'Kannada', 4),
    ('discover_native', 'ml', 'Malayalam', 5),
    ('chat_banner', 'te', 'Telugu', 1),
    ('chat_banner', 'hi', 'Hindi', 2),
    ('boost_rewarded', 'te', 'Telugu', 1),
    ('boost_rewarded', 'hi', 'Hindi', 2),
    ('superlike_rewarded', 'te', 'Telugu', 1),
    ('superlike_rewarded', 'hi', 'Hindi', 2)
ON CONFLICT (placement_id, language_code) DO NOTHING;

-- Indian states mapping table for ad targeting
CREATE TABLE IF NOT EXISTS indian_states (
    state_code VARCHAR(5) PRIMARY KEY,
    state_name VARCHAR(100) NOT NULL,
    language_code VARCHAR(10) NOT NULL,           -- Primary language
    secondary_languages VARCHAR(50)[],            -- Additional languages
    is_metro_state BOOLEAN DEFAULT FALSE,
    ecpm_tier INT DEFAULT 2                       -- 1 = high eCPM metros, 2 = standard, 3 = lower
);

INSERT INTO indian_states (state_code, state_name, language_code, secondary_languages, is_metro_state, ecpm_tier) VALUES
    ('AP', 'Andhra Pradesh', 'te', ARRAY['en', 'hi'], FALSE, 2),
    ('TS', 'Telangana', 'te', ARRAY['en', 'hi', 'ur'], TRUE, 1),  -- Hyderabad metro
    ('TN', 'Tamil Nadu', 'ta', ARRAY['en'], TRUE, 1),              -- Chennai metro
    ('KA', 'Karnataka', 'kn', ARRAY['en', 'hi'], TRUE, 1),         -- Bangalore metro
    ('KL', 'Kerala', 'ml', ARRAY['en', 'ta'], FALSE, 2),
    ('MH', 'Maharashtra', 'mr', ARRAY['en', 'hi'], TRUE, 1),       -- Mumbai metro
    ('GJ', 'Gujarat', 'gu', ARRAY['en', 'hi'], TRUE, 1),           -- Ahmedabad metro
    ('WB', 'West Bengal', 'bn', ARRAY['en', 'hi'], TRUE, 1),       -- Kolkata metro
    ('UP', 'Uttar Pradesh', 'hi', ARRAY['en', 'ur'], TRUE, 2),
    ('MP', 'Madhya Pradesh', 'hi', ARRAY['en'], FALSE, 2),
    ('RJ', 'Rajasthan', 'hi', ARRAY['en'], FALSE, 2),
    ('BR', 'Bihar', 'hi', ARRAY['en', 'bn'], FALSE, 3),
    ('PB', 'Punjab', 'pa', ARRAY['en', 'hi'], FALSE, 2),
    ('HR', 'Haryana', 'hi', ARRAY['en', 'pa'], TRUE, 1),           -- NCR
    ('DL', 'Delhi', 'hi', ARRAY['en', 'pa', 'ur'], TRUE, 1),       -- NCR
    ('JH', 'Jharkhand', 'hi', ARRAY['en', 'bn'], FALSE, 3),
    ('CG', 'Chhattisgarh', 'hi', ARRAY['en'], FALSE, 3),
    ('UK', 'Uttarakhand', 'hi', ARRAY['en'], FALSE, 2),
    ('HP', 'Himachal Pradesh', 'hi', ARRAY['en'], FALSE, 2),
    ('GA', 'Goa', 'mr', ARRAY['en', 'kn'], FALSE, 1),              -- High tourism eCPM
    ('OR', 'Odisha', 'or', ARRAY['en', 'hi'], FALSE, 3),
    ('AS', 'Assam', 'as', ARRAY['en', 'bn', 'hi'], FALSE, 3),
    ('JK', 'Jammu & Kashmir', 'ur', ARRAY['hi', 'en'], FALSE, 2)
ON CONFLICT (state_code) DO NOTHING;

-- Metro cities for eCPM targeting
CREATE TABLE IF NOT EXISTS indian_metro_cities (
    id SERIAL PRIMARY KEY,
    city_name VARCHAR(100) NOT NULL,
    alternate_names VARCHAR(100)[],               -- Bengaluru/Bangalore etc.
    state_code VARCHAR(5) REFERENCES indian_states(state_code),
    ecpm_tier INT DEFAULT 1,                      -- 1 = tier 1 metros
    population_millions DECIMAL(5,2),
    is_it_hub BOOLEAN DEFAULT FALSE,              -- IT hubs have higher spending
    created_at TIMESTAMP DEFAULT NOW(),
    UNIQUE(city_name, state_code)
);

INSERT INTO indian_metro_cities (city_name, alternate_names, state_code, ecpm_tier, population_millions, is_it_hub) VALUES
    ('Mumbai', ARRAY['Bombay'], 'MH', 1, 20.7, TRUE),
    ('Delhi', ARRAY['New Delhi', 'NCR'], 'DL', 1, 16.8, TRUE),
    ('Bangalore', ARRAY['Bengaluru'], 'KA', 1, 12.3, TRUE),
    ('Hyderabad', ARRAY['Secunderabad'], 'TS', 1, 10.0, TRUE),
    ('Chennai', ARRAY['Madras'], 'TN', 1, 10.9, TRUE),
    ('Kolkata', ARRAY['Calcutta'], 'WB', 1, 14.8, FALSE),
    ('Pune', ARRAY['Poona'], 'MH', 1, 6.6, TRUE),
    ('Ahmedabad', NULL, 'GJ', 1, 8.0, FALSE),
    ('Jaipur', NULL, 'RJ', 2, 3.9, FALSE),
    ('Surat', NULL, 'GJ', 2, 6.5, FALSE),
    ('Lucknow', NULL, 'UP', 2, 3.4, FALSE),
    ('Kanpur', NULL, 'UP', 2, 3.0, FALSE),
    ('Nagpur', NULL, 'MH', 2, 2.9, FALSE),
    ('Indore', NULL, 'MP', 2, 2.5, TRUE),
    ('Thane', NULL, 'MH', 1, 2.4, TRUE),
    ('Bhopal', NULL, 'MP', 2, 2.4, FALSE),
    ('Visakhapatnam', ARRAY['Vizag'], 'AP', 2, 2.1, TRUE),
    ('Vijayawada', ARRAY['Bezawada'], 'AP', 2, 1.5, FALSE),
    ('Coimbatore', NULL, 'TN', 2, 2.1, TRUE),
    ('Madurai', NULL, 'TN', 2, 1.5, FALSE),
    ('Kochi', ARRAY['Cochin'], 'KL', 2, 2.1, TRUE),
    ('Thiruvananthapuram', ARRAY['Trivandrum'], 'KL', 2, 1.0, FALSE),
    ('Gurgaon', ARRAY['Gurugram'], 'HR', 1, 1.5, TRUE),
    ('Noida', ARRAY['Greater Noida'], 'UP', 1, 1.2, TRUE)
ON CONFLICT (city_name, state_code) DO NOTHING;

-- Ad impressions tracking
CREATE TABLE IF NOT EXISTS ad_impressions (
    id BIGSERIAL PRIMARY KEY,
    user_id INT REFERENCES users(id),
    placement_id VARCHAR(100) NOT NULL,
    ad_network VARCHAR(50),                   -- 'admob', 'facebook', 'unity'
    ad_type VARCHAR(50),

    -- Revenue
    revenue_usd_micro INT DEFAULT 0,          -- Revenue in micro USD (1/1,000,000)
    ecpm_usd_micro INT DEFAULT 0,

    -- Interaction
    clicked BOOLEAN DEFAULT FALSE,
    completed BOOLEAN DEFAULT FALSE,          -- For rewarded ads

    -- Context
    platform VARCHAR(20),
    country_code VARCHAR(2),

    -- Regional targeting data
    state_code VARCHAR(5),                    -- Indian state (e.g., "TS", "AP")
    city VARCHAR(100),                        -- City for metro targeting
    language_code VARCHAR(10),                -- Language ad was served in (e.g., "te", "hi")

    created_at TIMESTAMP DEFAULT NOW()
);

-- Partition by month for performance
CREATE INDEX idx_ad_impressions_user ON ad_impressions(user_id);
CREATE INDEX idx_ad_impressions_date ON ad_impressions(created_at);
CREATE INDEX idx_ad_impressions_placement ON ad_impressions(placement_id);
CREATE INDEX idx_ad_impressions_language ON ad_impressions(language_code);
CREATE INDEX idx_ad_impressions_state ON ad_impressions(state_code);
CREATE INDEX idx_ad_impressions_city ON ad_impressions(city);

-- Rewarded ad completions (user earned rewards)
CREATE TABLE IF NOT EXISTS rewarded_ad_completions (
    id SERIAL PRIMARY KEY,
    user_id INT REFERENCES users(id),
    placement_id VARCHAR(100) NOT NULL,
    reward_type VARCHAR(50) NOT NULL,         -- 'boost', 'super_like', 'premium_hours'
    reward_value INT NOT NULL,                -- Amount of reward
    ad_impression_id BIGINT REFERENCES ad_impressions(id),
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_rewarded_completions_user ON rewarded_ad_completions(user_id);
CREATE INDEX idx_rewarded_completions_date ON rewarded_ad_completions(created_at);

-- Daily ad revenue aggregation (materialized for dashboards)
CREATE TABLE IF NOT EXISTS daily_ad_revenue (
    id SERIAL PRIMARY KEY,
    date DATE NOT NULL,
    placement_id VARCHAR(100),
    ad_network VARCHAR(50) NOT NULL,
    country_code VARCHAR(2),

    impressions INT DEFAULT 0,
    clicks INT DEFAULT 0,
    completions INT DEFAULT 0,
    revenue_usd_cents INT DEFAULT 0,
    revenue_micros BIGINT DEFAULT 0,         -- More precise revenue tracking

    UNIQUE(date, ad_network)                  -- Simplified unique constraint for handler compatibility
);

CREATE INDEX idx_daily_ad_revenue_date ON daily_ad_revenue(date);
CREATE INDEX idx_daily_ad_revenue_network ON daily_ad_revenue(ad_network);

-- =============================================================================
-- REVENUE ANALYTICS VIEWS
-- =============================================================================

-- Daily revenue summary
CREATE OR REPLACE VIEW v_daily_revenue AS
SELECT
    DATE(pt.created_at) as date,
    pt.gateway,
    pt.currency,
    COUNT(*) as transactions,
    COUNT(DISTINCT pt.user_id) as unique_payers,
    SUM(pt.amount_cents) as gross_revenue_cents,
    SUM(pt.gateway_fee_cents) as fees_cents,
    SUM(pt.net_amount_cents) as net_revenue_cents
FROM payment_transactions pt
WHERE pt.status = 'success'
GROUP BY DATE(pt.created_at), pt.gateway, pt.currency;

-- Monthly recurring revenue
CREATE OR REPLACE VIEW v_mrr AS
SELECT
    DATE_TRUNC('month', us.current_period_start) as month,
    p.product_id,
    COUNT(DISTINCT us.user_id) as active_subscribers,
    SUM(pp.amount_cents) as mrr_cents,
    pp.currency
FROM user_subscriptions us
JOIN products p ON us.product_id = p.id
JOIN product_prices pp ON p.id = pp.product_id
WHERE us.status = 'active'
  AND us.current_period_end > NOW()
GROUP BY DATE_TRUNC('month', us.current_period_start), p.product_id, pp.currency;

-- Ambassador performance
CREATE OR REPLACE VIEW v_ambassador_performance AS
SELECT
    a.id as ambassador_id,
    u.name as ambassador_name,
    a.university,
    a.city,
    a.tier,
    a.total_signups,
    a.total_verified_signups,
    a.target_signups_monthly,
    ROUND(a.total_verified_signups::numeric / NULLIF(a.target_signups_monthly, 0) * 100, 1) as target_achievement_pct,
    a.total_earnings_cents / 100.0 as total_earnings_inr,
    a.status
FROM ambassadors a
JOIN users u ON a.user_id = u.id;

-- =============================================================================
-- REGIONAL AD ANALYTICS VIEWS
-- =============================================================================

-- Ad revenue by language (for native language ad targeting)
CREATE OR REPLACE VIEW v_ad_revenue_by_language AS
SELECT
    DATE(ai.created_at) as date,
    ai.language_code,
    ai.language_code as language_name,
    COUNT(*) as impressions,
    SUM(CASE WHEN ai.clicked THEN 1 ELSE 0 END) as clicks,
    SUM(CASE WHEN ai.completed THEN 1 ELSE 0 END) as completions,
    COALESCE(SUM(ai.revenue_usd_micro), 0) / 1000000.0 as revenue_usd,
    CASE WHEN COUNT(*) > 0
        THEN COALESCE(SUM(ai.revenue_usd_micro), 0) / 1000000.0 / COUNT(*) * 1000
        ELSE 0 END as ecpm_usd
FROM ad_impressions ai
LEFT JOIN indian_states s ON ai.state_code = s.state_code
GROUP BY DATE(ai.created_at), ai.language_code;

-- Ad revenue by Indian state
CREATE OR REPLACE VIEW v_ad_revenue_by_state AS
SELECT
    DATE(ai.created_at) as date,
    ai.state_code,
    s.state_name,
    s.language_code as primary_language,
    s.ecpm_tier,
    COUNT(*) as impressions,
    SUM(CASE WHEN ai.clicked THEN 1 ELSE 0 END) as clicks,
    SUM(CASE WHEN ai.completed THEN 1 ELSE 0 END) as completions,
    COALESCE(SUM(ai.revenue_usd_micro), 0) / 1000000.0 as revenue_usd,
    CASE WHEN COUNT(*) > 0
        THEN COALESCE(SUM(ai.revenue_usd_micro), 0) / 1000000.0 / COUNT(*) * 1000
        ELSE 0 END as ecpm_usd
FROM ad_impressions ai
LEFT JOIN indian_states s ON ai.state_code = s.state_code
WHERE ai.country_code = 'IN'
GROUP BY DATE(ai.created_at), ai.state_code, s.state_name, s.language_code, s.ecpm_tier;

-- Ad revenue by city (metro vs non-metro)
CREATE OR REPLACE VIEW v_ad_revenue_by_city AS
SELECT
    DATE(ai.created_at) as date,
    ai.city,
    ai.state_code,
    CASE WHEN m.city_name IS NOT NULL THEN TRUE ELSE FALSE END as is_metro,
    m.ecpm_tier,
    m.is_it_hub,
    COUNT(*) as impressions,
    SUM(CASE WHEN ai.clicked THEN 1 ELSE 0 END) as clicks,
    COALESCE(SUM(ai.revenue_usd_micro), 0) / 1000000.0 as revenue_usd,
    CASE WHEN COUNT(*) > 0
        THEN COALESCE(SUM(ai.revenue_usd_micro), 0) / 1000000.0 / COUNT(*) * 1000
        ELSE 0 END as ecpm_usd
FROM ad_impressions ai
LEFT JOIN indian_metro_cities m ON LOWER(ai.city) = LOWER(m.city_name)
WHERE ai.country_code = 'IN' AND ai.city IS NOT NULL
GROUP BY DATE(ai.created_at), ai.city, ai.state_code, m.city_name, m.ecpm_tier, m.is_it_hub;

-- Telugu users ad performance (primary target audience)
CREATE OR REPLACE VIEW v_telugu_ad_performance AS
SELECT
    DATE(ai.created_at) as date,
    ai.placement_id,
    ai.ad_network,
    COUNT(*) as impressions,
    SUM(CASE WHEN ai.clicked THEN 1 ELSE 0 END) as clicks,
    SUM(CASE WHEN ai.completed THEN 1 ELSE 0 END) as completions,
    COALESCE(SUM(ai.revenue_usd_micro), 0) / 1000000.0 as revenue_usd,
    ROUND(SUM(CASE WHEN ai.clicked THEN 1 ELSE 0 END)::numeric / NULLIF(COUNT(*), 0) * 100, 2) as ctr_percent,
    CASE WHEN COUNT(*) > 0
        THEN COALESCE(SUM(ai.revenue_usd_micro), 0) / 1000000.0 / COUNT(*) * 1000
        ELSE 0 END as ecpm_usd
FROM ad_impressions ai
WHERE ai.language_code = 'te' OR ai.state_code IN ('AP', 'TS')
GROUP BY DATE(ai.created_at), ai.placement_id, ai.ad_network;

-- =============================================================================
-- AMBASSADOR ANALYTICS VIEWS
-- =============================================================================

-- Daily ambassador signups (for admin dashboard)
CREATE OR REPLACE VIEW v_ambassador_daily_signups AS
SELECT
    DATE(ru.created_at) as signup_date,
    a.id as ambassador_id,
    u.name as ambassador_name,
    a.tier,
    a.university,
    a.city,
    COUNT(*) as total_signups,
    COUNT(*) FILTER (WHERE ru.status = 'verified') as verified_signups,
    COUNT(*) FILTER (WHERE ru.status = 'pending') as pending_signups,
    ROUND(COUNT(*) FILTER (WHERE ru.status = 'verified')::numeric / NULLIF(COUNT(*), 0) * 100, 1) as conversion_rate
FROM ambassadors a
JOIN users u ON a.user_id = u.id
LEFT JOIN referral_codes rc ON rc.user_id = a.user_id AND rc.code_type = 'ambassador'
LEFT JOIN referral_uses ru ON ru.referral_code_id = rc.id
WHERE a.status = 'active'
GROUP BY DATE(ru.created_at), a.id, u.name, a.tier, a.university, a.city
ORDER BY signup_date DESC, total_signups DESC;

-- Weekly ambassador performance
CREATE OR REPLACE VIEW v_ambassador_weekly_performance AS
SELECT
    DATE_TRUNC('week', ru.created_at) as week_start,
    a.id as ambassador_id,
    u.name as ambassador_name,
    a.tier,
    a.university,
    a.city,
    COUNT(*) as total_signups,
    COUNT(*) FILTER (WHERE ru.status = 'verified') as verified_signups,
    a.target_signups_monthly / 4 as weekly_target,
    ROUND(COUNT(*) FILTER (WHERE ru.status = 'verified')::numeric / NULLIF(a.target_signups_monthly / 4, 0) * 100, 1) as target_pct
FROM ambassadors a
JOIN users u ON a.user_id = u.id
LEFT JOIN referral_codes rc ON rc.user_id = a.user_id AND rc.code_type = 'ambassador'
LEFT JOIN referral_uses ru ON ru.referral_code_id = rc.id
WHERE a.status = 'active'
GROUP BY DATE_TRUNC('week', ru.created_at), a.id, u.name, a.tier, a.university, a.city, a.target_signups_monthly
ORDER BY week_start DESC, verified_signups DESC;

-- Monthly ambassador performance with earnings
CREATE OR REPLACE VIEW v_ambassador_monthly_earnings AS
SELECT
    DATE_TRUNC('month', ru.created_at) as month,
    a.id as ambassador_id,
    u.name as ambassador_name,
    a.tier,
    a.university,
    a.city,
    COUNT(*) as total_signups,
    COUNT(*) FILTER (WHERE ru.status = 'verified') as verified_signups,
    a.target_signups_monthly as monthly_target,
    ROUND(COUNT(*) FILTER (WHERE ru.status = 'verified')::numeric / NULLIF(a.target_signups_monthly, 0) * 100, 1) as target_pct,
    a.monthly_stipend_cents / 100.0 as base_stipend_inr,
    GREATEST(0, COUNT(*) FILTER (WHERE ru.status = 'verified') - a.target_signups_monthly) * a.bonus_per_signup_cents / 100.0 as bonus_inr,
    a.monthly_stipend_cents / 100.0 +
        GREATEST(0, COUNT(*) FILTER (WHERE ru.status = 'verified') - a.target_signups_monthly) * a.bonus_per_signup_cents / 100.0 as total_earnings_inr
FROM ambassadors a
JOIN users u ON a.user_id = u.id
LEFT JOIN referral_codes rc ON rc.user_id = a.user_id AND rc.code_type = 'ambassador'
LEFT JOIN referral_uses ru ON ru.referral_code_id = rc.id
WHERE a.status = 'active'
GROUP BY DATE_TRUNC('month', ru.created_at), a.id, u.name, a.tier, a.university, a.city,
         a.target_signups_monthly, a.monthly_stipend_cents, a.bonus_per_signup_cents
ORDER BY month DESC, verified_signups DESC;

-- Ambassador leaderboard (all time)
CREATE OR REPLACE VIEW v_ambassador_leaderboard AS
SELECT
    ROW_NUMBER() OVER (ORDER BY a.total_verified_signups DESC) as rank,
    a.id as ambassador_id,
    u.name as ambassador_name,
    a.tier,
    a.university,
    a.city,
    a.total_signups,
    a.total_verified_signups,
    a.total_earnings_cents / 100.0 as total_earnings_inr,
    a.joined_at
FROM ambassadors a
JOIN users u ON a.user_id = u.id
WHERE a.status = 'active'
ORDER BY a.total_verified_signups DESC;

-- Ambassador signups by region (for geographic analytics)
CREATE OR REPLACE VIEW v_ambassador_signups_by_region AS
SELECT
    a.city,
    a.region,
    COUNT(DISTINCT a.id) as ambassador_count,
    SUM(a.total_signups) as total_signups,
    SUM(a.total_verified_signups) as total_verified,
    ROUND(SUM(a.total_verified_signups)::numeric / NULLIF(SUM(a.total_signups), 0) * 100, 1) as avg_conversion_rate,
    SUM(a.total_earnings_cents) / 100.0 as total_earnings_inr
FROM ambassadors a
WHERE a.status = 'active'
GROUP BY a.city, a.region
ORDER BY total_verified DESC;

-- Top performing universities
CREATE OR REPLACE VIEW v_top_universities AS
SELECT
    a.university,
    COUNT(DISTINCT a.id) as ambassador_count,
    SUM(a.total_signups) as total_signups,
    SUM(a.total_verified_signups) as total_verified,
    ROUND(AVG(a.total_verified_signups), 1) as avg_signups_per_ambassador,
    ROUND(SUM(a.total_verified_signups)::numeric / NULLIF(SUM(a.total_signups), 0) * 100, 1) as conversion_rate
FROM ambassadors a
WHERE a.status = 'active' AND a.university IS NOT NULL
GROUP BY a.university
HAVING SUM(a.total_signups) > 0
ORDER BY total_verified DESC;
