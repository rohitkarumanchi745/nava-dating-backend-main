-- Telugu Dating Backend - Database Schema
-- Run this migration to set up the database

-- Users table
CREATE TABLE IF NOT EXISTS users (
    id BIGSERIAL PRIMARY KEY,
    phone_number VARCHAR(20) UNIQUE NOT NULL,
    email VARCHAR(255) UNIQUE,
    name VARCHAR(255),
    dob DATE,
    gender VARCHAR(20),
    bio TEXT,
    location_text VARCHAR(255),
    interests JSONB,
    languages JSONB,
    looking_for VARCHAR(50),
    profession_category VARCHAR(100),
    profession_title VARCHAR(100),
    height_cm INTEGER,
    profile_photo_url TEXT,
    profile_photos JSONB,
    profile_photo_1 TEXT,
    profile_photo_2 TEXT,
    profile_photo_3 TEXT,
    voice_intro_url TEXT,
    voice_intro_duration INTEGER,
    is_active BOOLEAN DEFAULT TRUE,
    is_verified BOOLEAN DEFAULT FALSE,
    is_profile_complete BOOLEAN DEFAULT FALSE,
    is_student_verified BOOLEAN DEFAULT FALSE,
    verified_at TIMESTAMP,
    last_active TIMESTAMP DEFAULT NOW(),
    attractiveness_score DOUBLE PRECISION,
    ai_embedding JSONB,
    university VARCHAR(255),
    student_tier VARCHAR(50),
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_users_phone ON users(phone_number);
CREATE INDEX IF NOT EXISTS idx_users_active ON users(is_active, is_profile_complete);
CREATE INDEX IF NOT EXISTS idx_users_gender_dob ON users(gender, dob);
CREATE INDEX IF NOT EXISTS idx_users_last_active ON users(last_active);

-- User Preferences
CREATE TABLE IF NOT EXISTS user_preferences (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT UNIQUE NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    min_age INTEGER DEFAULT 18,
    max_age INTEGER DEFAULT 50,
    preferred_genders JSONB,
    intent VARCHAR(50),
    languages JSONB,
    max_distance INTEGER DEFAULT 50,
    distance_miles INTEGER,
    preferred_locations JSONB,
    only_verified BOOLEAN DEFAULT FALSE,
    only_students BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_preferences_user ON user_preferences(user_id);

-- User Locations
CREATE TABLE IF NOT EXISTS user_locations (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT UNIQUE NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    accuracy DOUBLE PRECISION,
    city VARCHAR(100),
    state VARCHAR(100),
    country VARCHAR(100) DEFAULT 'US',
    neighborhood VARCHAR(100),
    is_fuzzy BOOLEAN DEFAULT FALSE,
    show_exact_distance BOOLEAN DEFAULT FALSE,
    last_updated TIMESTAMP DEFAULT NOW(),
    update_source VARCHAR(20)
);

CREATE INDEX IF NOT EXISTS idx_locations_user ON user_locations(user_id);
CREATE INDEX IF NOT EXISTS idx_locations_city ON user_locations(city, state);
CREATE INDEX IF NOT EXISTS idx_locations_coords ON user_locations(latitude, longitude);

-- Matches
CREATE TABLE IF NOT EXISTS matches (
    id VARCHAR(36) PRIMARY KEY,
    user1_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user2_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user1_liked BOOLEAN,
    user2_liked BOOLEAN,
    is_mutual_match BOOLEAN DEFAULT FALSE,
    ai_compatibility_score DOUBLE PRECISION,
    visual_compatibility_score DOUBLE PRECISION,
    match_reason VARCHAR(50),
    messages_count INTEGER DEFAULT 0,
    voice_messages_count INTEGER DEFAULT 0,
    last_message_at TIMESTAMP,
    can_send_text BOOLEAN DEFAULT FALSE,
    status VARCHAR(20) DEFAULT 'active',
    blocked_by_user_id BIGINT,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_matches_user1 ON matches(user1_id, status);
CREATE INDEX IF NOT EXISTS idx_matches_user2 ON matches(user2_id, status);
CREATE INDEX IF NOT EXISTS idx_matches_mutual ON matches(is_mutual_match, created_at);
CREATE UNIQUE INDEX IF NOT EXISTS idx_matches_users ON matches(user1_id, user2_id);

-- Messages
CREATE TABLE IF NOT EXISTS messages (
    id BIGSERIAL PRIMARY KEY,
    match_id VARCHAR(36) NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    sender_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    receiver_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    message_type VARCHAR(20) DEFAULT 'text',
    is_read BOOLEAN DEFAULT FALSE,
    read_at TIMESTAMP,
    is_deleted BOOLEAN DEFAULT FALSE,
    is_flagged BOOLEAN DEFAULT FALSE,
    moderation_status VARCHAR(20),
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_messages_match ON messages(match_id, created_at);
CREATE INDEX IF NOT EXISTS idx_messages_receiver_read ON messages(receiver_id, is_read);

-- Voice Messages
CREATE TABLE IF NOT EXISTS voice_messages (
    id BIGSERIAL PRIMARY KEY,
    match_id VARCHAR(36) NOT NULL REFERENCES matches(id) ON DELETE CASCADE,
    sender_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    receiver_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    voice_url TEXT NOT NULL,
    duration INTEGER,
    file_size INTEGER,
    transcription TEXT,
    is_played BOOLEAN DEFAULT FALSE,
    played_at TIMESTAMP,
    play_count INTEGER DEFAULT 0,
    sentiment_score DOUBLE PRECISION,
    audio_quality_score DOUBLE PRECISION,
    is_flagged BOOLEAN DEFAULT FALSE,
    moderation_status VARCHAR(20),
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_voice_messages_match ON voice_messages(match_id, created_at);

-- User Subscriptions
CREATE TABLE IF NOT EXISTS user_subscriptions (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    subscription_type VARCHAR(20),
    pass_type VARCHAR(20),
    status VARCHAR(20) DEFAULT 'active',
    original_price DECIMAL(10,2),
    amount_paid DECIMAL(10,2),
    discount_applied DECIMAL(10,2),
    discount_type VARCHAR(20),
    start_date TIMESTAMP DEFAULT NOW(),
    end_date TIMESTAMP,
    payment_id VARCHAR(255),
    payment_method VARCHAR(50),
    stripe_subscription_id VARCHAR(255),
    is_active BOOLEAN DEFAULT TRUE,
    enhanced_radius DOUBLE PRECISION DEFAULT 0,
    can_see_city_names BOOLEAN DEFAULT FALSE,
    can_see_neighborhoods BOOLEAN DEFAULT FALSE,
    unlimited_swipes BOOLEAN DEFAULT FALSE,
    priority_visibility BOOLEAN DEFAULT FALSE,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_subscriptions_user ON user_subscriptions(user_id, status, end_date);

-- Spots (Short Videos/Audio)
CREATE TABLE IF NOT EXISTS spots (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    title VARCHAR(100),
    original_url TEXT,
    poster_url TEXT,
    mime_type VARCHAR(50),
    duration_sec INTEGER,
    renditions JSONB,
    tags JSONB,
    city VARCHAR(100),
    is_global BOOLEAN DEFAULT TRUE,
    expires_at TIMESTAMP,
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_spots_user ON spots(user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_spots_city ON spots(city, is_global);

-- Spot Messages
CREATE TABLE IF NOT EXISTS spot_messages (
    id BIGSERIAL PRIMARY KEY,
    spot_id BIGINT NOT NULL REFERENCES spots(id) ON DELETE CASCADE,
    sender_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    text TEXT NOT NULL,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_spot_messages_spot ON spot_messages(spot_id, created_at);

-- Spot Pair Status
CREATE TABLE IF NOT EXISTS spot_pair_status (
    id BIGSERIAL PRIMARY KEY,
    spot_id BIGINT NOT NULL REFERENCES spots(id) ON DELETE CASCADE,
    user_a BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_b BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    a_count INTEGER DEFAULT 0,
    b_count INTEGER DEFAULT 0,
    eligible_for_match BOOLEAN DEFAULT FALSE,
    request_status VARCHAR(20) DEFAULT 'none',
    match_id VARCHAR(36),
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_spot_pair ON spot_pair_status(spot_id, user_a, user_b);

-- Student Verifications
CREATE TABLE IF NOT EXISTS student_verifications (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT UNIQUE NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    university_name VARCHAR(255),
    university_type VARCHAR(20),
    email VARCHAR(255),
    student_id VARCHAR(100),
    status VARCHAR(20) DEFAULT 'pending',
    verification_method VARCHAR(20),
    discount_tier VARCHAR(20),
    submitted_at TIMESTAMP DEFAULT NOW(),
    verified_at TIMESTAMP,
    expires_at TIMESTAMP,
    verification_code VARCHAR(10),
    verification_token VARCHAR(255)
);

CREATE INDEX IF NOT EXISTS idx_student_verifications_user ON student_verifications(user_id);

-- Interaction Events (for ML)
CREATE TABLE IF NOT EXISTS interaction_events (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    target_user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    event_type VARCHAR(30) NOT NULL,
    event_metadata JSONB,
    slate_id VARCHAR(36),
    rank INTEGER,
    surface VARCHAR(30),
    reward DOUBLE PRECISION,
    delay_ms INTEGER,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_interaction_events_user ON interaction_events(user_id, created_at);
CREATE INDEX IF NOT EXISTS idx_interaction_events_type ON interaction_events(event_type);
CREATE INDEX IF NOT EXISTS idx_interaction_events_slate ON interaction_events(slate_id);

-- User Features (cached embeddings for ML)
CREATE TABLE IF NOT EXISTS user_features (
    user_id BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    embedding JSONB,
    recency_stats JSONB,
    updated_at TIMESTAMP DEFAULT NOW()
);

-- Bandit Arm Stats (contextual bandit state)
CREATE TABLE IF NOT EXISTS bandit_arm_stats (
    id BIGSERIAL PRIMARY KEY,
    arm_id VARCHAR(100) NOT NULL,
    arm_type VARCHAR(20) DEFAULT 'global',
    user_id BIGINT,
    a_matrix JSONB,
    b_vector JSONB,
    theta_vector JSONB,
    num_pulls INTEGER DEFAULT 0,
    total_reward DOUBLE PRECISION DEFAULT 0,
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_bandit_arm ON bandit_arm_stats(arm_id, arm_type);
CREATE INDEX IF NOT EXISTS idx_bandit_user ON bandit_arm_stats(user_id);

-- Content Moderation
CREATE TABLE IF NOT EXISTS content_moderation (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content_type VARCHAR(30) NOT NULL,
    content_id VARCHAR(100) NOT NULL,
    content_url TEXT,
    status VARCHAR(20) DEFAULT 'pending',
    auto_flagged BOOLEAN DEFAULT FALSE,
    manual_review BOOLEAN DEFAULT FALSE,
    inappropriate_score DOUBLE PRECISION,
    toxicity_score DOUBLE PRECISION,
    spam_score DOUBLE PRECISION,
    action_taken VARCHAR(30),
    moderator_notes TEXT,
    created_at TIMESTAMP DEFAULT NOW(),
    reviewed_at TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_moderation_status ON content_moderation(status, created_at);
CREATE INDEX IF NOT EXISTS idx_moderation_user ON content_moderation(user_id);

-- Spot Embeddings
CREATE TABLE IF NOT EXISTS spot_embeddings (
    spot_id BIGINT PRIMARY KEY REFERENCES spots(id) ON DELETE CASCADE,
    embedding JSONB,
    created_at TIMESTAMP DEFAULT NOW()
);

-- ============================================================================
-- REEL-BASED DATING SYSTEM
-- ============================================================================

-- Reels (enhanced spots for dating)
CREATE TABLE IF NOT EXISTS reels (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    video_url TEXT NOT NULL,
    thumbnail_url TEXT,
    duration_sec INTEGER,
    caption TEXT,
    audio_track VARCHAR(255),
    tags JSONB,                          -- ["funny", "travel", "cooking"]
    category VARCHAR(50),                -- lifestyle, humor, talent, travel, fitness
    location_tag VARCHAR(100),
    is_active BOOLEAN DEFAULT TRUE,
    view_count INTEGER DEFAULT 0,
    like_count INTEGER DEFAULT 0,
    comment_count INTEGER DEFAULT 0,
    message_count INTEGER DEFAULT 0,
    share_count INTEGER DEFAULT 0,
    avg_watch_percent DOUBLE PRECISION DEFAULT 0,
    engagement_score DOUBLE PRECISION DEFAULT 0,
    content_embedding JSONB,             -- ML embedding of video content
    audio_embedding JSONB,               -- ML embedding of audio/music
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_reels_user ON reels(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reels_active ON reels(is_active, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reels_category ON reels(category, engagement_score DESC);
CREATE INDEX IF NOT EXISTS idx_reels_engagement ON reels(engagement_score DESC);

-- Reel Views (tracks watch behavior)
CREATE TABLE IF NOT EXISTS reel_views (
    id BIGSERIAL PRIMARY KEY,
    reel_id BIGINT NOT NULL REFERENCES reels(id) ON DELETE CASCADE,
    viewer_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    watch_duration_sec INTEGER,
    watch_percent DOUBLE PRECISION,      -- 0-100, how much they watched
    rewatched BOOLEAN DEFAULT FALSE,
    rewatch_count INTEGER DEFAULT 0,
    source VARCHAR(30),                  -- feed, profile, search, shared
    session_id VARCHAR(36),
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_reel_views_reel ON reel_views(reel_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reel_views_viewer ON reel_views(viewer_id, created_at DESC);
CREATE UNIQUE INDEX IF NOT EXISTS idx_reel_views_unique ON reel_views(reel_id, viewer_id, session_id);

-- Reel Likes
CREATE TABLE IF NOT EXISTS reel_likes (
    id BIGSERIAL PRIMARY KEY,
    reel_id BIGINT NOT NULL REFERENCES reels(id) ON DELETE CASCADE,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_reel_likes_unique ON reel_likes(reel_id, user_id);
CREATE INDEX IF NOT EXISTS idx_reel_likes_user ON reel_likes(user_id, created_at DESC);

-- Reel Messages (PRIVATE DMs on reels - only sender and reel owner can see)
CREATE TABLE IF NOT EXISTS reel_messages (
    id BIGSERIAL PRIMARY KEY,
    reel_id BIGINT NOT NULL REFERENCES reels(id) ON DELETE CASCADE,
    sender_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    receiver_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    content TEXT NOT NULL,
    message_type VARCHAR(20) DEFAULT 'text',  -- text, voice, reaction
    reaction_emoji VARCHAR(10),
    is_read BOOLEAN DEFAULT FALSE,
    read_at TIMESTAMP,
    replied BOOLEAN DEFAULT FALSE,            -- Did receiver reply?
    reply_delay_sec INTEGER,                  -- How fast did they reply?
    conversation_continued BOOLEAN DEFAULT FALSE,  -- Did conversation go beyond 3 messages?
    led_to_match BOOLEAN DEFAULT FALSE,
    match_id VARCHAR(36),
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_reel_messages_reel ON reel_messages(reel_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reel_messages_sender ON reel_messages(sender_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reel_messages_receiver ON reel_messages(receiver_id, is_read);
CREATE INDEX IF NOT EXISTS idx_reel_messages_pair ON reel_messages(sender_id, receiver_id);

-- Reel Conversation Threads (tracks back-and-forth on a reel)
CREATE TABLE IF NOT EXISTS reel_conversations (
    id BIGSERIAL PRIMARY KEY,
    reel_id BIGINT NOT NULL REFERENCES reels(id) ON DELETE CASCADE,
    user_a BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    user_b BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    a_message_count INTEGER DEFAULT 0,
    b_message_count INTEGER DEFAULT 0,
    total_messages INTEGER DEFAULT 0,
    a_initiated BOOLEAN,                      -- Who started the conversation
    last_message_by BIGINT,
    last_message_at TIMESTAMP,
    avg_reply_time_sec INTEGER,
    sentiment_trend DOUBLE PRECISION,         -- Is conversation getting more positive?
    flirt_score DOUBLE PRECISION,             -- ML detected flirtiness level
    compatibility_score DOUBLE PRECISION,     -- Computed from conversation
    eligible_for_match BOOLEAN DEFAULT FALSE, -- Both engaged enough
    match_suggested BOOLEAN DEFAULT FALSE,
    match_accepted_a BOOLEAN,
    match_accepted_b BOOLEAN,
    match_id VARCHAR(36),
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_reel_conv_unique ON reel_conversations(reel_id, user_a, user_b);
CREATE INDEX IF NOT EXISTS idx_reel_conv_users ON reel_conversations(user_a, user_b);
CREATE INDEX IF NOT EXISTS idx_reel_conv_eligible ON reel_conversations(eligible_for_match, created_at DESC);

-- ============================================================================
-- ML LEARNING TABLES FOR REEL + SWIPE PATTERNS
-- ============================================================================

-- User Content Preferences (learned from reel interactions)
CREATE TABLE IF NOT EXISTS user_content_preferences (
    user_id BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,
    preferred_categories JSONB,              -- {humor: 0.8, travel: 0.6, fitness: 0.3}
    preferred_tags JSONB,                    -- {cooking: 0.9, pets: 0.7}
    preferred_audio_types JSONB,             -- {trending_music: 0.8, original: 0.5}
    avg_watch_duration_sec DOUBLE PRECISION,
    completion_rate DOUBLE PRECISION,        -- How often they watch full reels
    like_rate DOUBLE PRECISION,              -- Likes / Views
    comment_rate DOUBLE PRECISION,
    message_rate DOUBLE PRECISION,           -- How often they DM from reels
    response_rate DOUBLE PRECISION,          -- How often they get responses
    preferred_reel_length VARCHAR(20),       -- short (<15s), medium (15-30s), long (>30s)
    active_hours JSONB,                      -- {hour: engagement_score}
    embedding JSONB,                         -- Learned preference embedding
    updated_at TIMESTAMP DEFAULT NOW()
);

-- User Response Patterns (what gets this user responses)
CREATE TABLE IF NOT EXISTS user_response_patterns (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- What type of content gets responses for this user
    successful_categories JSONB,             -- Categories where user gets responses
    successful_tags JSONB,
    successful_caption_style VARCHAR(50),    -- question, statement, emoji, minimal
    successful_reel_length VARCHAR(20),

    -- What type of opener gets responses
    successful_opener_types JSONB,           -- {compliment: 0.7, question: 0.8, humor: 0.6}
    avg_opener_length INTEGER,
    uses_emoji_in_opener BOOLEAN,
    references_reel_content BOOLEAN,         -- Do they mention the reel specifically?

    -- Response metrics
    total_messages_sent INTEGER DEFAULT 0,
    total_responses_received INTEGER DEFAULT 0,
    response_rate DOUBLE PRECISION,
    avg_response_time_sec INTEGER,
    conversations_started INTEGER DEFAULT 0,
    conversations_continued INTEGER DEFAULT 0,  -- >3 messages
    matches_from_reels INTEGER DEFAULT 0,

    -- What type of people respond to this user
    responder_age_range JSONB,               -- {min: 22, max: 28, avg: 25}
    responder_gender_dist JSONB,
    responder_interests_overlap JSONB,

    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_response_patterns_user ON user_response_patterns(user_id);

-- Reel Engagement Events (detailed ML training data)
CREATE TABLE IF NOT EXISTS reel_engagement_events (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    reel_id BIGINT NOT NULL REFERENCES reels(id) ON DELETE CASCADE,
    reel_owner_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- Event details
    event_type VARCHAR(30) NOT NULL,         -- view, like, comment, message, share, skip, report
    event_value JSONB,                       -- Additional event data

    -- Context at time of event
    watch_percent DOUBLE PRECISION,
    time_on_reel_sec INTEGER,
    scroll_velocity DOUBLE PRECISION,        -- How fast were they scrolling?
    source VARCHAR(30),                      -- feed, profile, search, notification
    position_in_feed INTEGER,
    session_duration_sec INTEGER,
    reels_viewed_in_session INTEGER,

    -- Outcome tracking
    led_to_response BOOLEAN,
    response_delay_sec INTEGER,
    led_to_conversation BOOLEAN,
    conversation_length INTEGER,
    led_to_match BOOLEAN,

    -- ML features
    user_reel_owner_similarity DOUBLE PRECISION,  -- Embedding similarity
    content_preference_match DOUBLE PRECISION,    -- How well reel matches user preferences

    -- Reward signal for RL
    reward DOUBLE PRECISION,

    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_reel_events_user ON reel_engagement_events(user_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reel_events_reel ON reel_engagement_events(reel_id, event_type);
CREATE INDEX IF NOT EXISTS idx_reel_events_owner ON reel_engagement_events(reel_owner_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reel_events_type ON reel_engagement_events(event_type, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_reel_events_outcome ON reel_engagement_events(led_to_match, led_to_conversation);

-- Combined Interaction Model (learns from BOTH swipes and reels)
CREATE TABLE IF NOT EXISTS user_interaction_model (
    user_id BIGINT PRIMARY KEY REFERENCES users(id) ON DELETE CASCADE,

    -- Swipe-based learning
    swipe_q_weights JSONB,                   -- Q-learning weights for swipes
    swipe_exploration_rate DOUBLE PRECISION DEFAULT 0.3,
    total_swipes INTEGER DEFAULT 0,
    total_matches_from_swipes INTEGER DEFAULT 0,
    swipe_success_rate DOUBLE PRECISION,

    -- Reel-based learning
    reel_q_weights JSONB,                    -- Q-learning weights for reels
    reel_exploration_rate DOUBLE PRECISION DEFAULT 0.3,
    total_reel_interactions INTEGER DEFAULT 0,
    total_matches_from_reels INTEGER DEFAULT 0,
    reel_success_rate DOUBLE PRECISION,

    -- Combined model
    combined_weights JSONB,                  -- Blended model weights
    swipe_reel_weight_ratio DOUBLE PRECISION DEFAULT 0.5,  -- How much to weight swipe vs reel signals

    -- What works for this user
    best_interaction_mode VARCHAR(20),       -- swipe, reel, or both
    optimal_first_contact VARCHAR(20),       -- swipe_like, reel_comment, reel_message

    -- Personalization vectors
    attraction_embedding JSONB,              -- Who this user is attracted to
    attracted_by_embedding JSONB,            -- Who is attracted to this user
    content_style_embedding JSONB,           -- What content style works

    training_samples INTEGER DEFAULT 0,
    last_trained_at TIMESTAMP,
    model_version INTEGER DEFAULT 1,
    updated_at TIMESTAMP DEFAULT NOW()
);

-- Response Success Training Data
CREATE TABLE IF NOT EXISTS response_training_data (
    id BIGSERIAL PRIMARY KEY,
    sender_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    receiver_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- Context
    interaction_source VARCHAR(20) NOT NULL, -- swipe, reel_comment, reel_message
    reel_id BIGINT REFERENCES reels(id) ON DELETE SET NULL,

    -- Sender features at time of interaction
    sender_features JSONB,

    -- Receiver features
    receiver_features JSONB,

    -- Content features (for reel-based)
    reel_features JSONB,
    message_features JSONB,                  -- Length, sentiment, has_question, has_emoji, etc.

    -- Outcome (label for training)
    got_response BOOLEAN NOT NULL,
    response_time_sec INTEGER,
    response_quality VARCHAR(20),            -- none, short, engaged, enthusiastic
    conversation_continued BOOLEAN,
    led_to_match BOOLEAN,

    -- Computed reward
    reward DOUBLE PRECISION,

    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_response_training_sender ON response_training_data(sender_id, created_at DESC);
CREATE INDEX IF NOT EXISTS idx_response_training_outcome ON response_training_data(got_response, led_to_match);
CREATE INDEX IF NOT EXISTS idx_response_training_source ON response_training_data(interaction_source);

-- ============================================================================
-- LLM AUTO-LABELING SYSTEM
-- ============================================================================

-- LLM Labels for Reels (auto-generated content analysis)
CREATE TABLE IF NOT EXISTS reel_llm_labels (
    id BIGSERIAL PRIMARY KEY,
    reel_id BIGINT NOT NULL REFERENCES reels(id) ON DELETE CASCADE,

    -- Content Analysis
    content_summary TEXT,                    -- LLM generated summary
    detected_topics JSONB,                   -- ["cooking", "humor", "dance"]
    detected_mood VARCHAR(30),               -- happy, sad, energetic, calm, romantic
    detected_intent VARCHAR(30),             -- entertainment, showcase, storytelling, flirty
    detected_setting VARCHAR(50),            -- indoor, outdoor, beach, city, nature
    detected_activity VARCHAR(50),           -- cooking, dancing, talking, sports

    -- Quality Scores (0-1)
    production_quality DOUBLE PRECISION,     -- Video/audio quality
    creativity_score DOUBLE PRECISION,       -- How creative/unique
    engagement_potential DOUBLE PRECISION,   -- Predicted engagement
    authenticity_score DOUBLE PRECISION,     -- Real vs staged

    -- Dating Relevance
    dating_appeal_score DOUBLE PRECISION,    -- How attractive for dating context
    personality_traits JSONB,                -- {funny: 0.8, adventurous: 0.6, caring: 0.5}
    conversation_starters JSONB,             -- ["Nice cooking!", "Where is that place?"]

    -- Safety/Moderation
    nsfw_score DOUBLE PRECISION,
    spam_score DOUBLE PRECISION,
    catfish_risk DOUBLE PRECISION,

    -- Embeddings
    content_embedding JSONB,                 -- LLM embedding vector
    semantic_tags JSONB,                     -- Semantic tag embeddings

    -- Processing metadata
    llm_model VARCHAR(50),                   -- Model used for labeling
    llm_version VARCHAR(20),
    confidence DOUBLE PRECISION,
    processing_time_ms INTEGER,
    labeled_at TIMESTAMP DEFAULT NOW(),

    -- Human review
    human_reviewed BOOLEAN DEFAULT FALSE,
    human_corrections JSONB,
    reviewer_id BIGINT REFERENCES users(id),
    reviewed_at TIMESTAMP
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_reel_labels_reel ON reel_llm_labels(reel_id);
CREATE INDEX IF NOT EXISTS idx_reel_labels_mood ON reel_llm_labels(detected_mood);
CREATE INDEX IF NOT EXISTS idx_reel_labels_intent ON reel_llm_labels(detected_intent);
CREATE INDEX IF NOT EXISTS idx_reel_labels_quality ON reel_llm_labels(dating_appeal_score DESC);

-- LLM Labels for Messages (conversation analysis)
CREATE TABLE IF NOT EXISTS message_llm_labels (
    id BIGSERIAL PRIMARY KEY,
    message_id BIGINT NOT NULL REFERENCES reel_messages(id) ON DELETE CASCADE,

    -- Message Analysis
    message_type VARCHAR(30),                -- opener, response, question, compliment, flirt
    sentiment VARCHAR(20),                   -- positive, negative, neutral, flirty
    sentiment_score DOUBLE PRECISION,        -- -1 to 1
    intent VARCHAR(30),                      -- get_attention, show_interest, ask_question, humor

    -- Quality Metrics
    effort_score DOUBLE PRECISION,           -- How much effort put in
    personalization_score DOUBLE PRECISION,  -- References reel/profile specifically
    creativity_score DOUBLE PRECISION,
    conversation_value DOUBLE PRECISION,     -- Likely to continue conversation

    -- Content Features
    has_question BOOLEAN,
    has_compliment BOOLEAN,
    has_humor BOOLEAN,
    has_emoji BOOLEAN,
    references_reel BOOLEAN,
    references_profile BOOLEAN,
    word_count INTEGER,

    -- Response Prediction
    predicted_response_prob DOUBLE PRECISION,
    predicted_response_quality VARCHAR(20),  -- none, short, engaged, enthusiastic

    -- Red Flags
    spam_score DOUBLE PRECISION,
    creepy_score DOUBLE PRECISION,
    generic_score DOUBLE PRECISION,          -- Too generic/copy-paste

    -- Embeddings
    message_embedding JSONB,

    -- Processing metadata
    llm_model VARCHAR(50),
    confidence DOUBLE PRECISION,
    labeled_at TIMESTAMP DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_message_labels_msg ON message_llm_labels(message_id);
CREATE INDEX IF NOT EXISTS idx_message_labels_type ON message_llm_labels(message_type);
CREATE INDEX IF NOT EXISTS idx_message_labels_sentiment ON message_llm_labels(sentiment);

-- LLM Labels for User Profiles (personality analysis)
CREATE TABLE IF NOT EXISTS user_llm_labels (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- Profile Analysis
    bio_quality_score DOUBLE PRECISION,
    bio_authenticity DOUBLE PRECISION,
    photo_consistency DOUBLE PRECISION,     -- Do photos look like same person
    profile_completeness DOUBLE PRECISION,

    -- Personality Analysis (from bio + reels + messages)
    personality_summary TEXT,
    big_five JSONB,                          -- {openness: 0.7, conscientiousness: 0.5, ...}
    communication_style VARCHAR(30),         -- direct, playful, formal, casual
    humor_type VARCHAR(30),                  -- witty, sarcastic, wholesome, dark

    -- Dating Profile
    dating_style VARCHAR(30),                -- serious, casual, adventurous
    relationship_goals JSONB,                -- Inferred from content
    dealbreakers JSONB,                      -- Inferred preferences

    -- Interaction Patterns
    opener_style VARCHAR(30),                -- question, compliment, humor, direct
    avg_message_quality DOUBLE PRECISION,
    response_pattern VARCHAR(30),            -- quick, thoughtful, brief
    conversation_depth VARCHAR(20),          -- surface, moderate, deep

    -- Risk Assessment
    catfish_probability DOUBLE PRECISION,
    bot_probability DOUBLE PRECISION,
    toxic_probability DOUBLE PRECISION,

    -- Embeddings
    personality_embedding JSONB,
    preference_embedding JSONB,

    -- Metadata
    samples_analyzed INTEGER,                -- How many messages/reels analyzed
    last_analyzed_at TIMESTAMP,
    llm_model VARCHAR(50),
    model_version INTEGER DEFAULT 1,
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_user_labels_user ON user_llm_labels(user_id);
CREATE INDEX IF NOT EXISTS idx_user_labels_style ON user_llm_labels(communication_style);

-- LLM Labeling Queue (async processing)
CREATE TABLE IF NOT EXISTS llm_labeling_queue (
    id BIGSERIAL PRIMARY KEY,
    content_type VARCHAR(20) NOT NULL,       -- reel, message, user, conversation
    content_id BIGINT NOT NULL,
    priority INTEGER DEFAULT 5,              -- 1=highest, 10=lowest
    status VARCHAR(20) DEFAULT 'pending',    -- pending, processing, completed, failed
    retry_count INTEGER DEFAULT 0,
    max_retries INTEGER DEFAULT 3,
    error_message TEXT,
    created_at TIMESTAMP DEFAULT NOW(),
    started_at TIMESTAMP,
    completed_at TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_llm_queue_status ON llm_labeling_queue(status, priority, created_at);
CREATE INDEX IF NOT EXISTS idx_llm_queue_content ON llm_labeling_queue(content_type, content_id);

-- ============================================================================
-- FEDERATED LEARNING INFRASTRUCTURE
-- ============================================================================

-- Device/Client Registration for Federated Learning
CREATE TABLE IF NOT EXISTS fl_clients (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    device_id VARCHAR(64) NOT NULL,
    device_type VARCHAR(30),                 -- ios, android, web
    device_model VARCHAR(100),
    os_version VARCHAR(30),
    app_version VARCHAR(20),

    -- Capabilities
    compute_tier VARCHAR(20),                -- low, medium, high
    battery_threshold INTEGER DEFAULT 50,    -- Min battery to participate
    wifi_only BOOLEAN DEFAULT TRUE,

    -- Training Stats
    total_rounds_participated INTEGER DEFAULT 0,
    last_participation TIMESTAMP,
    avg_training_time_ms INTEGER,
    reliability_score DOUBLE PRECISION DEFAULT 1.0,

    -- Status
    is_active BOOLEAN DEFAULT TRUE,
    opted_in BOOLEAN DEFAULT TRUE,

    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_fl_clients_device ON fl_clients(user_id, device_id);
CREATE INDEX IF NOT EXISTS idx_fl_clients_active ON fl_clients(is_active, opted_in);

-- Federated Learning Rounds
CREATE TABLE IF NOT EXISTS fl_rounds (
    id BIGSERIAL PRIMARY KEY,
    round_number INTEGER NOT NULL,
    model_type VARCHAR(50) NOT NULL,         -- recommendation, response_prediction, compatibility

    -- Round Configuration
    target_clients INTEGER,
    min_clients INTEGER,
    client_fraction DOUBLE PRECISION,        -- % of clients to sample
    local_epochs INTEGER DEFAULT 1,
    batch_size INTEGER DEFAULT 32,
    learning_rate DOUBLE PRECISION DEFAULT 0.01,

    -- Global Model State
    global_weights JSONB,                    -- Current global model weights
    model_version INTEGER,

    -- Aggregation Config
    aggregation_method VARCHAR(30),          -- fedavg, fedprox, scaffold
    differential_privacy BOOLEAN DEFAULT TRUE,
    noise_multiplier DOUBLE PRECISION DEFAULT 1.0,
    clip_norm DOUBLE PRECISION DEFAULT 1.0,

    -- Round Status
    status VARCHAR(20) DEFAULT 'pending',    -- pending, in_progress, aggregating, completed, failed
    started_at TIMESTAMP,
    completed_at TIMESTAMP,

    -- Metrics
    clients_participated INTEGER DEFAULT 0,
    clients_failed INTEGER DEFAULT 0,
    avg_loss DOUBLE PRECISION,
    avg_accuracy DOUBLE PRECISION,
    convergence_delta DOUBLE PRECISION,

    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_fl_rounds_model ON fl_rounds(model_type, round_number DESC);
CREATE INDEX IF NOT EXISTS idx_fl_rounds_status ON fl_rounds(status, created_at);

-- Client Updates (local model updates from devices)
CREATE TABLE IF NOT EXISTS fl_client_updates (
    id BIGSERIAL PRIMARY KEY,
    round_id BIGINT NOT NULL REFERENCES fl_rounds(id) ON DELETE CASCADE,
    client_id BIGINT NOT NULL REFERENCES fl_clients(id) ON DELETE CASCADE,

    -- Local Training Results
    local_weights JSONB,                     -- Encrypted/DP-noised local weights
    weight_delta JSONB,                      -- Difference from global model
    num_samples INTEGER,                     -- Local training samples

    -- Training Metrics
    local_loss DOUBLE PRECISION,
    local_accuracy DOUBLE PRECISION,
    training_time_ms INTEGER,

    -- Privacy
    dp_epsilon DOUBLE PRECISION,             -- Privacy budget used
    dp_delta DOUBLE PRECISION,
    noise_added BOOLEAN DEFAULT TRUE,

    -- Verification
    checksum VARCHAR(64),
    is_valid BOOLEAN,
    validation_errors JSONB,

    -- Status
    status VARCHAR(20) DEFAULT 'received',   -- received, validating, accepted, rejected
    received_at TIMESTAMP DEFAULT NOW(),
    processed_at TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_fl_updates_round ON fl_client_updates(round_id, status);
CREATE INDEX IF NOT EXISTS idx_fl_updates_client ON fl_client_updates(client_id, received_at DESC);

-- Federated Models (versioned global models)
CREATE TABLE IF NOT EXISTS fl_models (
    id BIGSERIAL PRIMARY KEY,
    model_type VARCHAR(50) NOT NULL,
    version INTEGER NOT NULL,

    -- Model Architecture
    architecture JSONB,                      -- Layer definitions
    input_shape JSONB,
    output_shape JSONB,
    total_params INTEGER,

    -- Weights
    weights JSONB,                           -- Serialized model weights
    weights_url TEXT,                        -- Or URL to weights file

    -- Training History
    total_rounds INTEGER,
    total_samples INTEGER,
    total_clients INTEGER,

    -- Performance Metrics
    validation_loss DOUBLE PRECISION,
    validation_accuracy DOUBLE PRECISION,
    test_metrics JSONB,

    -- Deployment
    is_active BOOLEAN DEFAULT FALSE,
    deployed_at TIMESTAMP,

    -- Privacy
    privacy_budget_spent DOUBLE PRECISION,   -- Cumulative epsilon

    created_at TIMESTAMP DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_fl_models_version ON fl_models(model_type, version);
CREATE INDEX IF NOT EXISTS idx_fl_models_active ON fl_models(model_type, is_active);

-- Local Training Data (on-device data references)
CREATE TABLE IF NOT EXISTS fl_local_data (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id) ON DELETE CASCADE,

    -- Data Summary (not actual data - stays on device)
    data_type VARCHAR(30),                   -- interactions, messages, preferences
    sample_count INTEGER,
    feature_stats JSONB,                     -- Min/max/mean for normalization
    label_distribution JSONB,                -- Class balance info

    -- Data Quality
    quality_score DOUBLE PRECISION,
    freshness_score DOUBLE PRECISION,        -- How recent is the data
    diversity_score DOUBLE PRECISION,

    -- Training Eligibility
    min_samples_met BOOLEAN,
    last_used_in_round BIGINT REFERENCES fl_rounds(id),

    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE UNIQUE INDEX IF NOT EXISTS idx_fl_local_data_user ON fl_local_data(user_id, data_type);

-- ============================================================================
-- LLM TRAINING DATA EXPORT
-- ============================================================================

-- Training Data Snapshots (for fine-tuning LLM)
CREATE TABLE IF NOT EXISTS llm_training_snapshots (
    id BIGSERIAL PRIMARY KEY,
    snapshot_type VARCHAR(30) NOT NULL,      -- reel_labels, message_quality, response_prediction

    -- Data Selection Criteria
    date_from TIMESTAMP,
    date_to TIMESTAMP,
    filters JSONB,                           -- Additional filters applied
    sample_count INTEGER,

    -- Export Info
    export_format VARCHAR(20),               -- jsonl, parquet, tfrecord
    export_path TEXT,
    file_size_bytes BIGINT,
    checksum VARCHAR(64),

    -- Privacy
    anonymized BOOLEAN DEFAULT TRUE,
    pii_removed BOOLEAN DEFAULT TRUE,
    dp_applied BOOLEAN DEFAULT FALSE,

    -- Status
    status VARCHAR(20) DEFAULT 'pending',    -- pending, exporting, completed, failed
    created_at TIMESTAMP DEFAULT NOW(),
    completed_at TIMESTAMP
);

CREATE INDEX IF NOT EXISTS idx_llm_snapshots_type ON llm_training_snapshots(snapshot_type, created_at DESC);

-- Labeled Conversation Pairs (for response quality model)
CREATE TABLE IF NOT EXISTS llm_conversation_labels (
    id BIGSERIAL PRIMARY KEY,

    -- Conversation Context (anonymized)
    reel_labels JSONB,                       -- Content analysis of reel
    sender_profile_embedding JSONB,          -- Anonymized sender embedding
    receiver_profile_embedding JSONB,        -- Anonymized receiver embedding

    -- Message Sequence
    opener_text TEXT,                        -- Anonymized/sanitized
    opener_labels JSONB,
    response_text TEXT,
    response_labels JSONB,

    -- Outcome Labels
    got_response BOOLEAN,
    response_quality VARCHAR(20),
    conversation_length INTEGER,
    led_to_match BOOLEAN,
    match_quality VARCHAR(20),               -- If matched: unmatched, ghosted, ongoing, success

    -- Quality Control
    auto_labeled BOOLEAN DEFAULT TRUE,
    human_verified BOOLEAN DEFAULT FALSE,
    label_confidence DOUBLE PRECISION,

    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_conv_labels_outcome ON llm_conversation_labels(got_response, led_to_match);
CREATE INDEX IF NOT EXISTS idx_conv_labels_quality ON llm_conversation_labels(response_quality);

-- Model Performance Tracking
CREATE TABLE IF NOT EXISTS model_performance_log (
    id BIGSERIAL PRIMARY KEY,
    model_type VARCHAR(50) NOT NULL,         -- llm_labeler, fl_recommender, response_predictor
    model_version VARCHAR(30),

    -- Evaluation Metrics
    metric_name VARCHAR(50),                 -- accuracy, precision, recall, f1, auc, mse
    metric_value DOUBLE PRECISION,

    -- Evaluation Context
    eval_dataset VARCHAR(50),                -- train, validation, test, production
    sample_count INTEGER,

    -- A/B Test Info
    experiment_id VARCHAR(36),
    variant VARCHAR(20),

    recorded_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_model_perf_type ON model_performance_log(model_type, recorded_at DESC);
CREATE INDEX IF NOT EXISTS idx_model_perf_experiment ON model_performance_log(experiment_id);
