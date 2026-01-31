# NAVA Platform - Complete Technical Specification

## Document Overview

This document provides an exhaustive technical specification of the NAVA dating platform, covering every aspect of the system from database schema to API endpoints, matching algorithms, ML systems, and real-time features. This is based on the actual implementation code.

---

# Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [System Architecture](#2-system-architecture)
3. [Database Schema - Complete Reference](#3-database-schema---complete-reference)
4. [Discovery & Matching System - Deep Dive](#4-discovery--matching-system---deep-dive)
5. [Reels System - Complete Implementation](#5-reels-system---complete-implementation)
6. [Machine Learning Pipeline](#6-machine-learning-pipeline)
7. [Real-Time Features](#7-real-time-features)
8. [Authentication & Security](#8-authentication--security)
9. [Storage & CDN](#9-storage--cdn)
10. [Monetization System](#10-monetization-system)
11. [API Reference](#11-api-reference)
12. [Data Flow Diagrams](#12-data-flow-diagrams)

---

# 1. Executive Summary

## 1.1 What is NAVA?

NAVA is a dating application designed for the Telugu community with two primary ways to find matches:

1. **Swipe-Based Discovery** - Traditional like/pass on profile cards
2. **Reel-Based Discovery** - TikTok-style video feed where users can message creators

## 1.2 Key Differentiators

| Feature | How It Works |
|---------|--------------|
| **Dual Discovery** | Users can match via swipes OR via reel messages |
| **Voice Intros** | 30-second audio profiles for personality-first matching |
| **ML-Powered Ranking** | Contextual bandits learn what each user likes |
| **Privacy-First ML** | Federated learning keeps swipe data on-device |
| **Student Discounts** | Tiered pricing based on university verification |

## 1.3 Technology Stack

```
┌─────────────────────────────────────────────────────────────┐
│                        FRONTEND                              │
│  React Native 0.81 + Expo SDK 54 + Expo Router              │
│  RevenueCat (IAP) + Reanimated 4 (Animations)               │
└─────────────────────────────────────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│                        BACKEND                               │
│  Rust + Axum 0.8 + SQLx (PostgreSQL) + Redis                │
│  ONNX Runtime (tract) for Vision AI                         │
└─────────────────────────────────────────────────────────────┘
                              │
          ┌───────────────────┼───────────────────┐
          ▼                   ▼                   ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│   PostgreSQL    │ │     Redis       │ │    S3 + CDN     │
│   (Primary DB)  │ │ (Cache/Session) │ │ (Media Storage) │
└─────────────────┘ └─────────────────┘ └─────────────────┘
```

---

# 2. System Architecture

## 2.1 High-Level Architecture

```
                           ┌─────────────────────┐
                           │    Mobile Client    │
                           │  (iOS / Android)    │
                           └──────────┬──────────┘
                                      │
                                      ▼
                           ┌─────────────────────┐
                           │     CloudFront      │
                           │   (CDN for Media)   │
                           └──────────┬──────────┘
                                      │
                    ┌─────────────────┼─────────────────┐
                    │                 │                 │
                    ▼                 ▼                 ▼
             ┌───────────┐     ┌───────────┐     ┌───────────┐
             │  Photos   │     │   API     │     │  Videos   │
             │   (S3)    │     │ Gateway   │     │   (S3)    │
             └───────────┘     └─────┬─────┘     └───────────┘
                                     │
                                     ▼
                           ┌─────────────────────┐
                           │   Load Balancer     │
                           │      (AWS ALB)      │
                           └──────────┬──────────┘
                                      │
              ┌───────────────────────┼───────────────────────┐
              ▼                       ▼                       ▼
       ┌────────────┐          ┌────────────┐          ┌────────────┐
       │  Rust API  │          │  Rust API  │          │  Rust API  │
       │ Instance 1 │          │ Instance 2 │          │ Instance N │
       └─────┬──────┘          └─────┬──────┘          └─────┬──────┘
             │                       │                       │
             └───────────────────────┼───────────────────────┘
                                     │
         ┌───────────────────────────┼───────────────────────────┐
         ▼                           ▼                           ▼
┌─────────────────┐         ┌─────────────────┐         ┌─────────────────┐
│   PostgreSQL    │         │     Redis       │         │   LLM Service   │
│                 │         │   (Optional)    │         │   (Labeling)    │
│ • Users         │         │ • Sessions      │         │                 │
│ • Matches       │         │ • Rate Limits   │         │ • Content       │
│ • Messages      │         │ • Cache         │         │   Analysis      │
│ • Reels         │         │ • Online Status │         │ • Personality   │
│ • ML Data       │         └─────────────────┘         └─────────────────┘
└─────────────────┘
```

## 2.2 Backend Module Structure

```
rust-backend/src/
├── main.rs              # Server entry point, route definitions
├── config.rs            # Environment configuration (50+ variables)
├── state.rs             # Shared application state (DB, Redis, Vision)
├── auth.rs              # JWT token creation/verification
├── handlers.rs          # REST API handlers (~3500 lines)
├── graphql.rs           # GraphQL schema & resolvers (~1500 lines)
├── models.rs            # Database model structs
├── websocket.rs         # Real-time chat & call signaling
├── vision.rs            # ONNX model inference (NSFW, face, quality)
├── redis_service.rs     # Caching, rate limiting, sessions
├── storage.rs           # S3/local file storage with CDN
└── error.rs             # Error types
```

## 2.3 Request Flow

```
┌──────────────┐
│   Client     │
│   Request    │
└──────┬───────┘
       │
       ▼
┌──────────────┐     ┌──────────────┐
│    CORS      │────▶│   Tracing    │
│   Layer      │     │   (Logging)  │
└──────────────┘     └──────┬───────┘
                            │
                            ▼
                     ┌──────────────┐
                     │   Timeout    │
                     │  (30 secs)   │
                     └──────┬───────┘
                            │
                            ▼
                     ┌──────────────┐
                     │  Request ID  │
                     │    (UUID)    │
                     └──────┬───────┘
                            │
                            ▼
                     ┌──────────────┐
                     │ Compression  │
                     │   (Gzip)     │
                     └──────┬───────┘
                            │
                            ▼
                     ┌──────────────┐     No Token
                     │   Rate       │────────────────┐
                     │   Limit      │                │
                     └──────┬───────┘                │
                            │                        │
                            ▼                        ▼
                     ┌──────────────┐         ┌──────────────┐
                     │    Auth      │         │   Public     │
                     │  Middleware  │         │   Routes     │
                     └──────┬───────┘         └──────────────┘
                            │
                            ▼
                     ┌──────────────┐
                     │   Handler    │
                     │  Function    │
                     └──────┬───────┘
                            │
              ┌─────────────┼─────────────┐
              ▼             ▼             ▼
       ┌───────────┐ ┌───────────┐ ┌───────────┐
       │PostgreSQL │ │   Redis   │ │    S3     │
       │  Query    │ │   Cache   │ │  Upload   │
       └───────────┘ └───────────┘ └───────────┘
```

---

# 3. Database Schema - Complete Reference

## 3.1 Core Tables

### 3.1.1 Users Table

The central table storing all user information.

```sql
CREATE TABLE users (
    -- Identity
    id BIGSERIAL PRIMARY KEY,
    phone_number VARCHAR(20) UNIQUE NOT NULL,
    email VARCHAR(255) UNIQUE,

    -- Profile Information
    name VARCHAR(255),
    dob DATE,                              -- Date of birth (age calculated from this)
    gender VARCHAR(20),                    -- male, female, non-binary, other
    bio TEXT,                              -- User's self-description
    location_text VARCHAR(255),            -- Display location

    -- Interests & Preferences
    interests JSONB,                       -- ["travel", "cooking", "music"]
    languages JSONB,                       -- ["Telugu", "English", "Hindi"]
    looking_for VARCHAR(50),               -- relationship, casual, friendship

    -- Professional Info
    profession_category VARCHAR(100),      -- tech, healthcare, education, etc.
    profession_title VARCHAR(100),         -- Software Engineer, Doctor, etc.
    height_cm INTEGER,

    -- Photos
    profile_photo_url TEXT,                -- Primary photo (legacy)
    profile_photos JSONB,                  -- Array of all photo URLs
    profile_photo_1 TEXT,                  -- Individual photo slots
    profile_photo_2 TEXT,
    profile_photo_3 TEXT,

    -- Voice Introduction
    voice_intro_url TEXT,                  -- 30-second audio file
    voice_intro_duration INTEGER,          -- Duration in seconds

    -- Status Flags
    is_active BOOLEAN DEFAULT TRUE,        -- Account active
    is_verified BOOLEAN DEFAULT FALSE,     -- Identity verified
    is_profile_complete BOOLEAN DEFAULT FALSE,
    is_student_verified BOOLEAN DEFAULT FALSE,
    verified_at TIMESTAMP,
    last_active TIMESTAMP DEFAULT NOW(),

    -- ML Features
    attractiveness_score DOUBLE PRECISION, -- Computed from photo analysis
    ai_embedding JSONB,                    -- 128-dim user embedding vector

    -- Student Info
    university VARCHAR(255),
    student_tier VARCHAR(50),              -- ivy, top50, state, other

    -- Timestamps
    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

-- Indexes for query optimization
CREATE INDEX idx_users_phone ON users(phone_number);
CREATE INDEX idx_users_active ON users(is_active, is_profile_complete);
CREATE INDEX idx_users_gender_dob ON users(gender, dob);
CREATE INDEX idx_users_last_active ON users(last_active);
```

**Key Fields Explained:**

| Field | Purpose | Used By |
|-------|---------|---------|
| `attractiveness_score` | Photo quality rating (0-1) | Discovery ranking |
| `ai_embedding` | 128-dim vector from profile | Similarity matching |
| `voice_intro_url` | Audio introduction file | Profile display |
| `is_profile_complete` | Gates access to discovery | Discovery filter |

### 3.1.2 User Preferences Table

Stores what each user is looking for in a match.

```sql
CREATE TABLE user_preferences (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT UNIQUE NOT NULL REFERENCES users(id),

    -- Age Range
    min_age INTEGER DEFAULT 18,
    max_age INTEGER DEFAULT 50,

    -- Gender Preference
    preferred_genders JSONB,               -- ["male", "female"]

    -- Intent
    intent VARCHAR(50),                    -- serious, casual, friendship

    -- Language Preference
    languages JSONB,                       -- Preferred languages

    -- Location
    max_distance INTEGER DEFAULT 50,       -- In kilometers
    distance_miles INTEGER,                -- Legacy field
    preferred_locations JSONB,             -- Specific cities

    -- Filters
    only_verified BOOLEAN DEFAULT FALSE,   -- Only show verified users
    only_students BOOLEAN DEFAULT FALSE,   -- Only show students

    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);
```

### 3.1.3 User Locations Table

Stores user's current location for distance-based matching.

```sql
CREATE TABLE user_locations (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT UNIQUE NOT NULL REFERENCES users(id),

    -- Coordinates
    latitude DOUBLE PRECISION,
    longitude DOUBLE PRECISION,
    accuracy DOUBLE PRECISION,             -- GPS accuracy in meters

    -- Location Names
    city VARCHAR(100),
    state VARCHAR(100),
    country VARCHAR(100) DEFAULT 'US',
    neighborhood VARCHAR(100),

    -- Privacy Settings
    is_fuzzy BOOLEAN DEFAULT FALSE,        -- Randomize exact location
    show_exact_distance BOOLEAN DEFAULT FALSE,

    last_updated TIMESTAMP DEFAULT NOW(),
    update_source VARCHAR(20)              -- gps, manual, ip
);

-- Spatial indexes for distance queries
CREATE INDEX idx_locations_coords ON user_locations(latitude, longitude);
CREATE INDEX idx_locations_city ON user_locations(city, state);
```

### 3.1.4 Matches Table

**This is the core matching table** - stores all like/pass interactions and mutual matches.

```sql
CREATE TABLE matches (
    id VARCHAR(36) PRIMARY KEY,            -- UUID string

    -- User Pair (user1_id < user2_id always)
    user1_id BIGINT NOT NULL REFERENCES users(id),
    user2_id BIGINT NOT NULL REFERENCES users(id),

    -- Like Status
    user1_liked BOOLEAN,                   -- NULL = not seen, TRUE = liked, FALSE = passed
    user2_liked BOOLEAN,                   -- NULL = not seen, TRUE = liked, FALSE = passed
    is_mutual_match BOOLEAN DEFAULT FALSE, -- TRUE when both liked

    -- AI Scores
    ai_compatibility_score DOUBLE PRECISION,     -- ML-predicted compatibility
    visual_compatibility_score DOUBLE PRECISION, -- Photo similarity score
    match_reason VARCHAR(50),                    -- "similar_interests", "mutual_friends"

    -- Conversation Status
    messages_count INTEGER DEFAULT 0,
    voice_messages_count INTEGER DEFAULT 0,
    last_message_at TIMESTAMP,
    can_send_text BOOLEAN DEFAULT FALSE,   -- Unlocked after voice exchange

    -- Status
    status VARCHAR(20) DEFAULT 'active',   -- active, blocked, unmatched
    blocked_by_user_id BIGINT,

    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

-- Unique constraint ensures one record per user pair
CREATE UNIQUE INDEX idx_matches_users ON matches(user1_id, user2_id);
CREATE INDEX idx_matches_mutual ON matches(is_mutual_match, created_at);
```

**How Matching Works (from actual code):**

```
MATCHING ALGORITHM:
==================

1. User A likes User B:
   - Determine order: (user1_id, user2_id) = (min(A,B), max(A,B))
   - Check if match record exists

2. If NO existing record:
   - Create new match: {user1_liked: A_is_user1, user2_liked: NULL}
   - is_mutual = FALSE

3. If existing record:
   - Update the liker's field (user1_liked or user2_liked)
   - Check if OTHER person already liked
   - If both liked: is_mutual_match = TRUE → "It's a match!"

4. User can also PASS:
   - Sets user*_liked = FALSE
   - Prevents showing in discovery again
```

**Example:**
```
User 5 likes User 12:
  → user1_id = 5, user2_id = 12 (5 < 12)
  → user1_liked = TRUE (because user 5 is user1)
  → user2_liked = NULL (user 12 hasn't seen yet)

Later, User 12 likes User 5:
  → Same record found
  → user2_liked = TRUE
  → is_mutual_match = TRUE ✓ MATCH!
```

### 3.1.5 Messages Table

Stores chat messages between matched users.

```sql
CREATE TABLE messages (
    id BIGSERIAL PRIMARY KEY,
    match_id VARCHAR(36) NOT NULL REFERENCES matches(id),
    sender_id BIGINT NOT NULL REFERENCES users(id),
    receiver_id BIGINT NOT NULL REFERENCES users(id),

    content TEXT NOT NULL,
    message_type VARCHAR(20) DEFAULT 'text', -- text, image, voice, video

    is_read BOOLEAN DEFAULT FALSE,
    read_at TIMESTAMP,
    is_deleted BOOLEAN DEFAULT FALSE,
    is_flagged BOOLEAN DEFAULT FALSE,
    moderation_status VARCHAR(20),

    created_at TIMESTAMP DEFAULT NOW()
);
```

## 3.2 Reels Tables

### 3.2.1 Reels Table

Main table for TikTok-style video content.

```sql
CREATE TABLE reels (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),

    -- Video Content
    video_url TEXT NOT NULL,
    thumbnail_url TEXT,
    duration_sec INTEGER,
    caption TEXT,
    audio_track VARCHAR(255),              -- Music/sound used

    -- Categorization
    tags JSONB,                            -- ["funny", "travel", "cooking"]
    category VARCHAR(50),                  -- lifestyle, humor, talent, travel, fitness
    location_tag VARCHAR(100),

    -- Status
    is_active BOOLEAN DEFAULT TRUE,

    -- Engagement Metrics
    view_count INTEGER DEFAULT 0,
    like_count INTEGER DEFAULT 0,
    comment_count INTEGER DEFAULT 0,       -- Not used (private DMs only)
    message_count INTEGER DEFAULT 0,       -- DMs received on this reel
    share_count INTEGER DEFAULT 0,
    avg_watch_percent DOUBLE PRECISION DEFAULT 0,
    engagement_score DOUBLE PRECISION DEFAULT 0, -- Composite score

    -- ML Embeddings
    content_embedding JSONB,               -- Visual content embedding
    audio_embedding JSONB,                 -- Audio/music embedding

    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

-- Indexes for feed ordering
CREATE INDEX idx_reels_engagement ON reels(engagement_score DESC);
CREATE INDEX idx_reels_category ON reels(category, engagement_score DESC);
CREATE INDEX idx_reels_user ON reels(user_id, created_at DESC);
```

### 3.2.2 Reel Views Table

Tracks detailed viewing behavior for ML training.

```sql
CREATE TABLE reel_views (
    id BIGSERIAL PRIMARY KEY,
    reel_id BIGINT NOT NULL REFERENCES reels(id),
    viewer_id BIGINT NOT NULL REFERENCES users(id),

    -- Watch Metrics
    watch_duration_sec INTEGER,            -- How long they watched
    watch_percent DOUBLE PRECISION,        -- 0-100%, completion rate
    rewatched BOOLEAN DEFAULT FALSE,       -- Did they replay?
    rewatch_count INTEGER DEFAULT 0,       -- How many times

    -- Context
    source VARCHAR(30),                    -- feed, profile, search, shared
    session_id VARCHAR(36),                -- Links views in same session

    created_at TIMESTAMP DEFAULT NOW()
);

-- Unique per session (allows multiple views)
CREATE UNIQUE INDEX idx_reel_views_unique ON reel_views(reel_id, viewer_id, session_id);
```

### 3.2.3 Reel Messages Table

Private DMs sent on reels - **this is a key dating feature**.

```sql
CREATE TABLE reel_messages (
    id BIGSERIAL PRIMARY KEY,
    reel_id BIGINT NOT NULL REFERENCES reels(id),
    sender_id BIGINT NOT NULL REFERENCES users(id),
    receiver_id BIGINT NOT NULL REFERENCES users(id),  -- Always reel owner

    -- Message Content
    content TEXT NOT NULL,
    message_type VARCHAR(20) DEFAULT 'text',  -- text, voice, reaction
    reaction_emoji VARCHAR(10),                -- 🔥, ❤️, etc.

    -- Status
    is_read BOOLEAN DEFAULT FALSE,
    read_at TIMESTAMP,

    -- Outcome Tracking (for ML)
    replied BOOLEAN DEFAULT FALSE,             -- Did owner reply?
    reply_delay_sec INTEGER,                   -- How fast?
    conversation_continued BOOLEAN DEFAULT FALSE, -- >3 messages
    led_to_match BOOLEAN DEFAULT FALSE,        -- Did they match?
    match_id VARCHAR(36),

    created_at TIMESTAMP DEFAULT NOW()
);
```

### 3.2.4 Reel Conversations Table

Tracks back-and-forth messaging on a reel.

```sql
CREATE TABLE reel_conversations (
    id BIGSERIAL PRIMARY KEY,
    reel_id BIGINT NOT NULL REFERENCES reels(id),
    user_a BIGINT NOT NULL REFERENCES users(id),  -- user_a < user_b
    user_b BIGINT NOT NULL REFERENCES users(id),

    -- Message Counts
    a_message_count INTEGER DEFAULT 0,
    b_message_count INTEGER DEFAULT 0,
    total_messages INTEGER DEFAULT 0,

    -- Conversation Flow
    a_initiated BOOLEAN,                   -- Who started
    last_message_by BIGINT,
    last_message_at TIMESTAMP,
    avg_reply_time_sec INTEGER,

    -- ML Scores
    sentiment_trend DOUBLE PRECISION,      -- Getting more positive?
    flirt_score DOUBLE PRECISION,          -- ML-detected flirtiness
    compatibility_score DOUBLE PRECISION,  -- Computed from conversation

    -- Match Eligibility
    eligible_for_match BOOLEAN DEFAULT FALSE,  -- Both engaged enough
    match_suggested BOOLEAN DEFAULT FALSE,
    match_accepted_a BOOLEAN,
    match_accepted_b BOOLEAN,
    match_id VARCHAR(36),

    created_at TIMESTAMP DEFAULT NOW(),
    updated_at TIMESTAMP DEFAULT NOW()
);

CREATE UNIQUE INDEX idx_reel_conv_unique ON reel_conversations(reel_id, user_a, user_b);
```

## 3.3 ML Training Tables

### 3.3.1 Interaction Events Table

Logs all user interactions for ML training.

```sql
CREATE TABLE interaction_events (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    target_user_id BIGINT NOT NULL REFERENCES users(id),

    event_type VARCHAR(30) NOT NULL,       -- impression, like, pass, message, view
    event_metadata JSONB,                  -- Additional context

    -- Discovery Context
    slate_id VARCHAR(36),                  -- Groups events from same feed load
    rank INTEGER,                          -- Position in feed (0 = top)
    surface VARCHAR(30),                   -- discover, reel, profile

    -- ML Reward Signal
    reward DOUBLE PRECISION,               -- -1 to +3 based on action
    delay_ms INTEGER,                      -- Time to action

    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX idx_interaction_events_slate ON interaction_events(slate_id);
CREATE INDEX idx_interaction_events_type ON interaction_events(event_type);
```

**Reward Values (from code):**

| Action | Reward | Meaning |
|--------|--------|---------|
| Impression | 0 | Profile shown |
| Pass | -0.1 | Negative signal |
| View (<25%) | -0.1 | Low interest |
| View (25-50%) | 0.2 | Some interest |
| View (50-90%) | 0.5 | Good interest |
| View (>90%) | 1.0 | High interest |
| Like | 1.0 | Positive signal |
| Reel Like | 2.0 | Strong signal |
| Message | 3.0+ | Highest signal |

### 3.3.2 Bandit Arm Stats Table

Stores contextual bandit model state for recommendations.

```sql
CREATE TABLE bandit_arm_stats (
    id BIGSERIAL PRIMARY KEY,
    arm_id VARCHAR(100) NOT NULL,          -- "age_25_30", "interest_travel"
    arm_type VARCHAR(20) DEFAULT 'global', -- global, user, context
    user_id BIGINT,                        -- For personalized arms

    -- LinUCB Statistics
    a_matrix JSONB,                        -- Inverse covariance matrix
    b_vector JSONB,                        -- Cumulative reward vector
    theta_vector JSONB,                    -- Model weights

    -- Stats
    num_pulls INTEGER DEFAULT 0,           -- Times this arm was selected
    total_reward DOUBLE PRECISION DEFAULT 0,

    updated_at TIMESTAMP DEFAULT NOW()
);
```

### 3.3.3 User Content Preferences Table

Learned preferences from reel interactions.

```sql
CREATE TABLE user_content_preferences (
    user_id BIGINT PRIMARY KEY REFERENCES users(id),

    -- Category Preferences (learned weights)
    preferred_categories JSONB,            -- {humor: 0.8, travel: 0.6}
    preferred_tags JSONB,                  -- {cooking: 0.9, pets: 0.7}
    preferred_audio_types JSONB,           -- {trending_music: 0.8}

    -- Viewing Behavior
    avg_watch_duration_sec DOUBLE PRECISION,
    completion_rate DOUBLE PRECISION,      -- How often they finish reels
    like_rate DOUBLE PRECISION,            -- Likes / Views
    comment_rate DOUBLE PRECISION,
    message_rate DOUBLE PRECISION,         -- DMs from reels
    response_rate DOUBLE PRECISION,        -- Responses to their DMs

    -- Preferences
    preferred_reel_length VARCHAR(20),     -- short, medium, long
    active_hours JSONB,                    -- {14: 0.8, 20: 0.9} (hour → engagement)

    -- Learned Embedding
    embedding JSONB,                       -- Preference vector

    updated_at TIMESTAMP DEFAULT NOW()
);
```

### 3.3.4 Response Training Data Table

Training data for predicting message responses.

```sql
CREATE TABLE response_training_data (
    id BIGSERIAL PRIMARY KEY,
    sender_id BIGINT NOT NULL REFERENCES users(id),
    receiver_id BIGINT NOT NULL REFERENCES users(id),

    -- Context
    interaction_source VARCHAR(20) NOT NULL, -- swipe, reel_message
    reel_id BIGINT REFERENCES reels(id),

    -- Features (for training)
    sender_features JSONB,
    receiver_features JSONB,
    reel_features JSONB,
    message_features JSONB,                -- {length, has_question, effort}

    -- Labels (ground truth)
    got_response BOOLEAN NOT NULL,
    response_time_sec INTEGER,
    response_quality VARCHAR(20),          -- none, short, engaged
    conversation_continued BOOLEAN,
    led_to_match BOOLEAN,

    -- Computed Reward
    reward DOUBLE PRECISION,

    created_at TIMESTAMP DEFAULT NOW()
);
```

## 3.4 LLM Labeling Tables

### 3.4.1 Reel LLM Labels Table

Auto-generated content analysis for reels.

```sql
CREATE TABLE reel_llm_labels (
    id BIGSERIAL PRIMARY KEY,
    reel_id BIGINT NOT NULL REFERENCES reels(id),

    -- Content Analysis
    content_summary TEXT,                  -- LLM-generated summary
    detected_topics JSONB,                 -- ["cooking", "humor"]
    detected_mood VARCHAR(30),             -- happy, romantic, energetic
    detected_intent VARCHAR(30),           -- entertainment, flirty
    detected_setting VARCHAR(50),          -- outdoor, beach, city
    detected_activity VARCHAR(50),         -- cooking, dancing

    -- Quality Scores (0-1)
    production_quality DOUBLE PRECISION,
    creativity_score DOUBLE PRECISION,
    engagement_potential DOUBLE PRECISION,
    authenticity_score DOUBLE PRECISION,

    -- Dating Relevance
    dating_appeal_score DOUBLE PRECISION,
    personality_traits JSONB,              -- {funny: 0.8, adventurous: 0.6}
    conversation_starters JSONB,           -- ["Nice cooking!", "Where is that?"]

    -- Safety
    nsfw_score DOUBLE PRECISION,
    spam_score DOUBLE PRECISION,
    catfish_risk DOUBLE PRECISION,

    -- Embeddings
    content_embedding JSONB,

    -- Processing Metadata
    llm_model VARCHAR(50),                 -- "llama3"
    confidence DOUBLE PRECISION,
    labeled_at TIMESTAMP DEFAULT NOW()
);
```

## 3.5 Federated Learning Tables

### 3.5.1 FL Clients Table

Device registration for federated learning.

```sql
CREATE TABLE fl_clients (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    device_id VARCHAR(64) NOT NULL,

    -- Device Info
    device_type VARCHAR(30),               -- ios, android
    device_model VARCHAR(100),
    os_version VARCHAR(30),
    app_version VARCHAR(20),

    -- Capability
    compute_tier VARCHAR(20),              -- low, medium, high
    battery_threshold INTEGER,             -- Min battery to train
    wifi_only BOOLEAN DEFAULT TRUE,

    -- Status
    is_eligible BOOLEAN DEFAULT TRUE,
    last_trained_at TIMESTAMP,
    training_samples INTEGER DEFAULT 0,

    -- Reliability Score (for client selection)
    reliability_score DOUBLE PRECISION DEFAULT 0.5,
    successful_rounds INTEGER DEFAULT 0,
    failed_rounds INTEGER DEFAULT 0,

    created_at TIMESTAMP DEFAULT NOW()
);
```

### 3.5.2 FL Rounds Table

Training round history.

```sql
CREATE TABLE fl_rounds (
    id BIGSERIAL PRIMARY KEY,
    round_number INTEGER NOT NULL,
    model_type VARCHAR(50),                -- recommendation, response

    -- Aggregated Weights
    global_weights JSONB,

    -- Privacy Settings
    differential_privacy BOOLEAN DEFAULT TRUE,
    noise_multiplier DOUBLE PRECISION,
    clip_norm DOUBLE PRECISION,

    -- Stats
    participants INTEGER,
    avg_loss DOUBLE PRECISION,

    started_at TIMESTAMP,
    completed_at TIMESTAMP
);
```

---

# 4. Discovery & Matching System - Deep Dive

## 4.1 Discovery Flow Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                     DISCOVERY FLOW                                   │
└─────────────────────────────────────────────────────────────────────┘

User Opens Discover Tab
         │
         ▼
┌─────────────────────┐
│  GET /discover      │
│  or GraphQL         │
│  query { discover } │
└─────────┬───────────┘
          │
          ▼
┌─────────────────────┐
│  1. Load User       │
│     Profile &       │
│     Preferences     │
└─────────┬───────────┘
          │
          ▼
┌─────────────────────┐
│  2. Build Query     │
│  • Age range        │
│  • Gender prefs     │
│  • Distance filter  │
│  • Exclude seen     │
└─────────┬───────────┘
          │
          ▼
┌─────────────────────┐
│  3. Fetch Candidates│
│  ORDER BY:          │
│  • Voice intro      │
│  • Attractiveness   │
│  • Last active      │
└─────────┬───────────┘
          │
          ▼
┌─────────────────────┐
│  4. Calculate       │
│  Compatibility      │
│  Scores             │
└─────────┬───────────┘
          │
          ▼
┌─────────────────────┐
│  5. Filter by       │
│  Distance           │
└─────────┬───────────┘
          │
          ▼
┌─────────────────────┐
│  6. Log Impressions │
│  for ML Training    │
└─────────┬───────────┘
          │
          ▼
┌─────────────────────┐
│  7. Return Profiles │
│  with slate_id      │
└─────────────────────┘
```

## 4.2 Discovery Query (Actual SQL)

From [handlers.rs:821-850](rust-backend/src/handlers.rs#L821-L850):

```sql
SELECT u.id, u.name, u.dob, u.gender, u.bio,
       u.profile_photo_url, u.profile_photos,
       u.profile_photo_1, u.profile_photo_2, u.profile_photo_3,
       u.is_verified, u.attractiveness_score,
       u.looking_for, u.profession_title, u.height_cm,
       l.city, l.latitude, l.longitude
FROM users u
LEFT JOIN user_locations l ON l.user_id = u.id
WHERE u.id != $1                           -- Not self
  AND u.is_active = TRUE                   -- Active accounts only
  AND u.is_profile_complete = TRUE         -- Complete profiles only
  AND ($2 = FALSE OR u.is_verified = TRUE) -- Verified filter
  AND u.dob IS NOT NULL                    -- Has birthdate
  AND EXTRACT(YEAR FROM AGE(u.dob)) BETWEEN $3 AND $4  -- Age range
  AND NOT EXISTS (
      -- Exclude already interacted
      SELECT 1 FROM matches m
      WHERE (m.user1_id = $1 AND m.user2_id = u.id AND m.user1_liked IS NOT NULL)
         OR (m.user2_id = $1 AND m.user1_id = u.id AND m.user2_liked IS NOT NULL)
  )
ORDER BY
    u.attractiveness_score DESC NULLS LAST,  -- Quality first
    RANDOM()                                  -- Then randomize
LIMIT $5
```

**Key Points:**
- **Excludes seen users**: Anyone the current user has liked OR passed
- **Orders by attractiveness**: Higher-quality profiles shown first
- **Adds randomness**: Prevents always showing same top profiles

## 4.3 Compatibility Score Calculation

From [graphql.rs:1486-1542](rust-backend/src/graphql.rs#L1486-L1542):

```rust
fn calculate_compatibility(
    my_interests: &[String],
    my_languages: &[String],
    my_looking_for: &Option<String>,
    their_interests: &[String],
    their_languages: &[String],
    their_looking_for: &Option<String>,
    has_voice_intro: bool,
    is_verified: bool,
) -> f64 {
    let mut score = 50.0; // Base score

    // Interest overlap (up to +25 points)
    // Uses set intersection to find common interests
    if !my_interests.is_empty() && !their_interests.is_empty() {
        let my_set: HashSet<_> = my_interests.iter()
            .map(|s| s.to_lowercase()).collect();
        let their_set: HashSet<_> = their_interests.iter()
            .map(|s| s.to_lowercase()).collect();
        let overlap = my_set.intersection(&their_set).count();
        let max_possible = my_set.len().min(their_set.len()).max(1);
        let interest_score = (overlap as f64 / max_possible as f64) * 25.0;
        score += interest_score;
    }

    // Language match (up to +15 points)
    if !my_languages.is_empty() && !their_languages.is_empty() {
        let my_langs: HashSet<_> = my_languages.iter()
            .map(|s| s.to_lowercase()).collect();
        let their_langs: HashSet<_> = their_languages.iter()
            .map(|s| s.to_lowercase()).collect();
        if my_langs.intersection(&their_langs).count() > 0 {
            score += 15.0;
        }
    }

    // Looking for match (+10 points)
    if let (Some(mine), Some(theirs)) = (my_looking_for, their_looking_for) {
        if mine.to_lowercase() == theirs.to_lowercase() {
            score += 10.0;
        }
    }

    // Voice intro bonus (+5 points)
    if has_voice_intro {
        score += 5.0;
    }

    // Verified bonus (+5 points)
    if is_verified {
        score += 5.0;
    }

    // Add randomness (±5%)
    let variance = (rand::random::<f64>() - 0.5) * 10.0;
    score += variance;

    score.max(0.0).min(100.0).round()
}
```

**Score Breakdown:**

| Factor | Max Points | How It Works |
|--------|------------|--------------|
| Base | 50 | Everyone starts here |
| Interest Overlap | +25 | (common / min_total) * 25 |
| Language Match | +15 | Any shared language |
| Looking For Match | +10 | Same relationship intent |
| Voice Intro | +5 | Has audio introduction |
| Verified | +5 | Identity verified |
| Random | ±5 | Prevents identical scores |
| **Maximum** | **110** | (clamped to 100) |

## 4.4 Like/Pass Flow

### 4.4.1 Like User (REST API)

From [handlers.rs:912-1003](rust-backend/src/handlers.rs#L912-L1003):

```
POST /discover/like
Body: { "target_user_id": 123 }

┌─────────────────────────────────────────────────────────────────────┐
│                        LIKE FLOW                                     │
└─────────────────────────────────────────────────────────────────────┘

1. Validate Request
   │
   ├─► Cannot like yourself
   │
   ├─► Target must exist and be active
   │
   ▼
2. Determine User Order
   │
   │   user1_id = min(current_user, target)
   │   user2_id = max(current_user, target)
   │   is_user1 = (current_user < target)
   │
   ▼
3. Check for Existing Match Record
   │
   │   SELECT * FROM matches
   │   WHERE user1_id = $1 AND user2_id = $2
   │
   ├─► If EXISTS:
   │   │
   │   ├─► Update current user's like status
   │   │   UPDATE matches SET user1_liked = TRUE... OR user2_liked = TRUE...
   │   │
   │   ├─► Check if OTHER user already liked
   │   │
   │   └─► If both liked: is_mutual_match = TRUE
   │
   └─► If NOT EXISTS:
       │
       └─► Create new match record
           INSERT INTO matches (id, user1_id, user2_id, user1_liked, user2_liked, ...)
           VALUES (uuid, ..., TRUE, NULL, ...)  -- Only liker's field set

4. Log Interaction Event (for ML)
   │
   │   INSERT INTO interaction_events
   │   (user_id, target_user_id, event_type='like', surface='discover')
   │
   ▼
5. Return Result
   {
     "message": "It's a match!" or "Like sent",
     "match_id": "uuid-...",
     "is_mutual": true/false
   }
```

### 4.4.2 Pass User (REST API)

From [handlers.rs:1006-1068](rust-backend/src/handlers.rs#L1006-L1068):

```
POST /discover/pass
Body: { "target_user_id": 123 }

1. Log pass event (negative ML signal)
   │
   │   INSERT INTO interaction_events (event_type='pass', reward=-0.1)
   │
   ▼
2. Determine user order (same as like)
   │
   ▼
3. Update/Create match record with liked = FALSE
   │
   │   This ensures user won't see this profile again
   │
   ▼
4. Return { "message": "Passed" }
```

## 4.5 GraphQL Mutations for Matching

From [graphql.rs:1206-1300](rust-backend/src/graphql.rs#L1206-L1300):

```graphql
mutation LikeUser($targetUserId: Int!) {
  likeUser(targetUserId: $targetUserId) {
    success
    isMutual
    matchId
    message
  }
}

mutation PassUser($targetUserId: Int!) {
  passUser(targetUserId: $targetUserId)
}
```

**Response Types:**
```graphql
type LikeResult {
  success: Boolean!
  isMutual: Boolean!     # TRUE if it's a match
  matchId: String        # Only present if mutual
  message: String!       # "It's a match!" or "Like sent"
}
```

## 4.6 Distance Calculation

From [handlers.rs](rust-backend/src/handlers.rs) - Uses Haversine formula:

```rust
fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    const R: f64 = 6371.0; // Earth's radius in km

    let dlat = (lat2 - lat1).to_radians();
    let dlon = (lon2 - lon1).to_radians();
    let lat1 = lat1.to_radians();
    let lat2 = lat2.to_radians();

    let a = (dlat / 2.0).sin().powi(2)
          + lat1.cos() * lat2.cos() * (dlon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();

    R * c
}
```

---

# 5. Reels System - Complete Implementation

## 5.1 Reels Overview

The reels system provides an alternative discovery method where users:
1. Upload short videos (like TikTok)
2. Browse a personalized feed of other users' reels
3. Send **private DMs** to reel creators (no public comments)
4. Conversations can lead to matches

## 5.2 Reel Feed Algorithm

From [handlers.rs:3643-3696](rust-backend/src/handlers.rs#L3643-L3696):

```sql
SELECT r.id, r.user_id, r.video_url, r.thumbnail_url,
       r.duration_sec, r.caption, r.tags, r.category,
       r.engagement_score, r.view_count, r.like_count, r.created_at,
       u.name as creator_name,
       u.profile_photo_1 as creator_photo,
       u.is_verified as creator_verified
FROM reels r
JOIN users u ON u.id = r.user_id
WHERE r.is_active = TRUE
  AND r.user_id != $1                     -- Not own reels
  AND NOT EXISTS (
      -- Exclude blocked users
      SELECT 1 FROM matches m
      WHERE ((m.user1_id = $1 AND m.user2_id = r.user_id)
          OR (m.user2_id = $1 AND m.user1_id = r.user_id))
      AND m.status = 'blocked'
  )
ORDER BY
    r.engagement_score DESC NULLS LAST,   -- High engagement first
    r.created_at DESC                     -- Then recent
LIMIT $2
```

## 5.3 Tracking Reel Views

From [handlers.rs:3711-3758](rust-backend/src/handlers.rs#L3711-L3758):

```
POST /reels/track-view
Body: {
  "reel_id": 123,
  "watch_duration_sec": 15,
  "watch_percent": 85.5,
  "rewatched": false,
  "source": "feed",
  "session_id": "abc-123",
  "scroll_velocity": 0.5,
  "position_in_feed": 3
}
```

**What Gets Tracked:**

| Field | Purpose | ML Use |
|-------|---------|--------|
| `watch_percent` | How much was watched | Interest signal |
| `watch_duration_sec` | Absolute time | Engagement depth |
| `rewatched` | Did they replay | Strong interest |
| `scroll_velocity` | How fast scrolling | Low = more interested |
| `position_in_feed` | Where in feed | Position bias |
| `source` | How they found reel | Attribution |

**Interest Score Calculation:**

```rust
fn calc_interest_score(
    watch_percent: f64,
    duration: i32,
    rewatched: bool,
    scroll_velocity: Option<f64>
) -> f64 {
    let mut score = 0.0;

    // Watch completion (0-0.5)
    score += (watch_percent / 100.0) * 0.5;

    // Duration bonus (0-0.2)
    score += (duration.min(30) as f64 / 30.0) * 0.2;

    // Rewatch bonus (+0.2)
    if rewatched {
        score += 0.2;
    }

    // Slow scroll bonus (0-0.1)
    if let Some(vel) = scroll_velocity {
        if vel < 0.3 {
            score += 0.1;
        }
    }

    score.min(1.0)
}
```

**Reward Calculation for ML:**

```rust
let reward = if watch_percent >= 90.0 {
    1.0    // Watched almost all
} else if watch_percent >= 50.0 {
    0.5    // Watched half
} else if watch_percent >= 25.0 {
    0.2    // Watched some
} else {
    -0.1   // Quickly skipped
};
```

## 5.4 Liking Reels

From [handlers.rs:3767-3790](rust-backend/src/handlers.rs#L3767-L3790):

```
POST /reels/like
Body: { "reel_id": 123 }

Flow:
1. Verify reel exists
2. Cannot like own reel
3. Insert into reel_likes (idempotent with ON CONFLICT DO NOTHING)
4. Increment reel's like_count
5. Log ML event with reward = 2.0 (strong signal)
6. Update user's content preferences
```

## 5.5 Sending Reel Messages (Key Dating Feature)

From [handlers.rs:3818-3869](rust-backend/src/handlers.rs#L3818-L3869):

```
POST /reels/message
Body: {
  "reel_id": 123,
  "content": "Love your cooking! What dish is that?",
  "message_type": "text",
  "reaction_emoji": "🔥"
}
```

**Message Effort Score Calculation:**

```rust
fn calc_message_effort(content: &str, has_reaction: bool) -> f64 {
    let mut score = 0.0;

    // Length bonus (0-0.4)
    let word_count = content.split_whitespace().count();
    score += (word_count.min(20) as f64 / 20.0) * 0.4;

    // Question bonus (+0.2) - shows genuine interest
    if content.contains('?') {
        score += 0.2;
    }

    // Reaction bonus (+0.1)
    if has_reaction {
        score += 0.1;
    }

    // Personalization indicators (+0.3)
    // (Checked by LLM later for references to reel content)

    score.min(1.0)
}
```

**What Happens When You Message:**

1. **Message saved** to `reel_messages` table
2. **Reel stats updated**: `message_count += 1`
3. **Conversation thread created/updated** in `reel_conversations`
4. **ML event logged** with reward = `3.0 + effort_score`
5. **Content preferences updated** for sender
6. **Response training data created** to track if they get a reply

## 5.6 Reel-to-Match Conversion

When a reel conversation is going well, the system can suggest a match:

```sql
-- Check if eligible for match suggestion
UPDATE reel_conversations
SET eligible_for_match = TRUE
WHERE reel_id = $1
  AND user_a = $2
  AND user_b = $3
  AND total_messages >= 6                  -- At least 6 messages
  AND a_message_count >= 2                 -- Both participated
  AND b_message_count >= 2
  AND avg_reply_time_sec < 3600;           -- Replies within 1 hour
```

---

# 6. Machine Learning Pipeline

## 6.1 ML Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────┐
│                        ML PIPELINE                                   │
└─────────────────────────────────────────────────────────────────────┘

                    ┌─────────────────────┐
                    │   User Interactions  │
                    │   (likes, passes,    │
                    │    views, messages)  │
                    └──────────┬──────────┘
                               │
           ┌───────────────────┼───────────────────┐
           ▼                   ▼                   ▼
    ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
    │ Interaction │     │   Reel      │     │  Response   │
    │   Events    │     │ Engagement  │     │  Training   │
    │   Table     │     │   Events    │     │    Data     │
    └──────┬──────┘     └──────┬──────┘     └──────┬──────┘
           │                   │                   │
           └───────────────────┼───────────────────┘
                               │
                               ▼
                    ┌─────────────────────┐
                    │  Training Pipeline   │
                    │                      │
                    │  • Contextual Bandit │
                    │  • Response Predictor│
                    │  • Content Ranker    │
                    └──────────┬──────────┘
                               │
           ┌───────────────────┼───────────────────┐
           ▼                   ▼                   ▼
    ┌─────────────┐     ┌─────────────┐     ┌─────────────┐
    │   Bandit    │     │    User     │     │  Response   │
    │  Arm Stats  │     │  Content    │     │  Patterns   │
    │             │     │   Prefs     │     │             │
    └─────────────┘     └─────────────┘     └─────────────┘
                               │
                               ▼
                    ┌─────────────────────┐
                    │   Personalized      │
                    │   Discovery Feed    │
                    └─────────────────────┘
```

## 6.2 Vision AI Models (ONNX)

Five ONNX models run on the backend for image/video analysis:

| Model | Input | Output | Use Case |
|-------|-------|--------|----------|
| **NSFW Detector** | 224x224 image | Safe/Unsafe (0-1) | Content moderation |
| **FER+ (Emotion)** | 48x48 face | 7 emotion probabilities | Profile analysis |
| **NIMA (Aesthetics)** | 224x224 image | Quality score (1-10) | Photo ranking |
| **ArcFace** | 112x112 face | 512-dim embedding | Face verification |
| **MiniFASNet** | 112x112 face | Real/Spoof (0-1) | Liveness detection |

**Photo Analysis Pipeline:**

```rust
pub fn analyze_photo(&self, image: &DynamicImage) -> PhotoAnalysis {
    // 1. NSFW Check
    let nsfw_score = self.run_nsfw(image);
    if nsfw_score > 0.7 {
        return PhotoAnalysis::rejected("NSFW content detected");
    }

    // 2. Face Detection + Embedding
    let face_embedding = self.run_arcface(image);

    // 3. Quality Score
    let quality_score = self.run_nima(image);

    // 4. Emotion Detection
    let emotion = self.run_fer(image);

    PhotoAnalysis {
        nsfw_score,
        quality_score,    // Used for attractiveness_score
        emotion,
        face_embedding,   // Used for verification
    }
}
```

## 6.3 Contextual Bandits (LinUCB)

The recommendation engine uses LinUCB to balance exploration vs exploitation:

**How It Works:**

```
For each user viewing discover feed:

1. Get user features: x_user = [age, gender, interests_embedding, ...]

2. For each candidate profile:
   a. Get profile features: x_profile = [age, gender, ...]
   b. Combine features: x = concat(x_user, x_profile, x_user * x_profile)
   c. Compute UCB score:
      score = θᵀx + α * sqrt(xᵀA⁻¹x)
              ↑         ↑
        exploitation  exploration

3. Rank by UCB score (higher = show first)

4. User takes action (like/pass)

5. Update arm statistics:
   A = A + xxᵀ           (inverse covariance)
   b = b + r*x           (cumulative reward)
   θ = A⁻¹b              (updated weights)
```

**Database Storage:**

```sql
-- Each "arm" represents a type of profile
INSERT INTO bandit_arm_stats (arm_id, a_matrix, b_vector, theta_vector, num_pulls)
VALUES
  ('age_18_24', '[[...]]', '[...]', '[...]', 1523),
  ('age_25_30', '[[...]]', '[...]', '[...]', 2341),
  ('interest_travel', '[[...]]', '[...]', '[...]', 892);
```

## 6.4 LLM Auto-Labeling

Async system for content analysis using LLaMA 3:

```
┌─────────────────────────────────────────────────────────────────────┐
│                    LLM LABELING PIPELINE                             │
└─────────────────────────────────────────────────────────────────────┘

New Reel Uploaded
       │
       ▼
┌─────────────────┐
│ Add to Queue    │
│ (priority 5)    │
└────────┬────────┘
         │
         ▼
┌─────────────────┐     ┌─────────────────┐
│ Worker Picks Up │────▶│ LLM Analysis    │
│ (batch of 10)   │     │ (LLaMA 3)       │
└─────────────────┘     └────────┬────────┘
                                 │
                                 ▼
                    ┌─────────────────────┐
                    │ Generated Labels:   │
                    │ • detected_mood     │
                    │ • detected_topics   │
                    │ • personality_traits│
                    │ • dating_appeal     │
                    │ • conversation_     │
                    │   starters          │
                    │ • content_embedding │
                    └────────┬────────────┘
                             │
                             ▼
                    ┌─────────────────────┐
                    │ Store in           │
                    │ reel_llm_labels    │
                    └─────────────────────┘
```

**Queue Table:**

```sql
CREATE TABLE llm_labeling_queue (
    id BIGSERIAL PRIMARY KEY,
    content_type VARCHAR(20),    -- reel, message, user
    content_id BIGINT,
    priority INTEGER DEFAULT 5,  -- 1=highest, 10=lowest
    status VARCHAR(20),          -- pending, processing, completed, failed
    retry_count INTEGER DEFAULT 0,
    created_at TIMESTAMP
);
```

## 6.5 Federated Learning

Privacy-preserving on-device training:

```
┌─────────────────────────────────────────────────────────────────────┐
│                    FEDERATED LEARNING FLOW                           │
└─────────────────────────────────────────────────────────────────────┘

                         Server
                    ┌─────────────┐
                    │ Global Model│
                    │   v(N)      │
                    └──────┬──────┘
                           │
         Download          │           Download
    ┌──────────────────────┼──────────────────────┐
    │                      │                      │
    ▼                      ▼                      ▼
┌──────────┐         ┌──────────┐         ┌──────────┐
│ Device 1 │         │ Device 2 │         │ Device N │
│          │         │          │         │          │
│ Local    │         │ Local    │         │ Local    │
│ Swipes   │         │ Swipes   │         │ Swipes   │
│ (private)│         │ (private)│         │ (private)│
│          │         │          │         │          │
│ Train    │         │ Train    │         │ Train    │
│ Locally  │         │ Locally  │         │ Locally  │
└────┬─────┘         └────┬─────┘         └────┬─────┘
     │                    │                    │
     │    ┌───────────────┼───────────────┐    │
     │    │               │               │    │
     │    │   Weight Deltas (encrypted)   │    │
     │    │   + Differential Privacy      │    │
     │    │                               │    │
     └────┼───────────────┼───────────────┼────┘
          │               │               │
          └───────────────┼───────────────┘
                          │
                          ▼
                    ┌─────────────┐
                    │   Server    │
                    │   FedAvg    │
                    │ Aggregation │
                    └──────┬──────┘
                           │
                           ▼
                    ┌─────────────┐
                    │ Global Model│
                    │   v(N+1)    │
                    └─────────────┘
```

**Privacy Guarantees:**

| Mechanism | Implementation |
|-----------|----------------|
| Local Training | Raw swipes never leave device |
| Gradient Clipping | `clip_norm = 1.0` |
| Differential Privacy | `noise_multiplier = 1.0`, ε=1.0 |
| Secure Aggregation | Only aggregated updates sent |

**Configuration:**

```env
FL_ENABLED=true
FL_MIN_CLIENTS=10          # Min devices per round
FL_CLIENT_FRACTION=0.1     # % of clients per round
FL_LOCAL_EPOCHS=1          # Training epochs on device
FL_LEARNING_RATE=0.01
FL_DP_ENABLED=true         # Differential privacy
FL_NOISE_MULTIPLIER=1.0
FL_CLIP_NORM=1.0
```

---

# 7. Real-Time Features

## 7.1 WebSocket Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                    WEBSOCKET CONNECTIONS                             │
└─────────────────────────────────────────────────────────────────────┘

Client connects to:
  ws://server/ws/chat/{match_id}    (for messaging)
  ws://server/ws/call/{call_id}     (for video calls)

            ┌────────────────────────────────────────┐
            │              Server                     │
            │                                        │
            │   ┌────────────────────────────────┐   │
            │   │         Chat Rooms             │   │
            │   │  HashMap<match_id, broadcast>  │   │
            │   └───────────────┬────────────────┘   │
            │                   │                    │
            │   ┌───────────────┼────────────────┐   │
            │   │               │                │   │
            │   ▼               ▼                ▼   │
            │ Room A         Room B          Room C  │
            │ (match_1)     (match_2)       (match_3)│
            └─────┬───────────┬─────────────┬───────┘
                  │           │             │
        ┌─────────┴───┐   ┌───┴───┐   ┌────┴────┐
        │ User A      │   │ Users │   │ Users   │
        │ User B      │   │ C & D │   │ E & F   │
        └─────────────┘   └───────┘   └─────────┘
```

## 7.2 Chat Message Flow

```
User A sends message in Room for match_123:

1. Client A → WebSocket → Server
   { "type": "message", "content": "Hello!" }

2. Server:
   a. Save to messages table
   b. Get broadcast channel for match_123
   c. Broadcast to all subscribers

3. Server → WebSocket → Client B
   {
     "type": "message",
     "sender_id": 123,
     "content": "Hello!",
     "message_id": "abc-123",
     "created_at": "2024-01-15T..."
   }
```

**Chat Room Management:**

```rust
pub struct ChatRooms {
    rooms: HashMap<String, broadcast::Sender<ChatMessage>>,
}

impl ChatRooms {
    pub fn get_or_create(&mut self, match_id: &str) -> broadcast::Sender<ChatMessage> {
        self.rooms
            .entry(match_id.to_string())
            .or_insert_with(|| {
                let (tx, _) = broadcast::channel(100);
                tx
            })
            .clone()
    }
}
```

## 7.3 Video Call Signaling

```
┌─────────────────────────────────────────────────────────────────────┐
│                    VIDEO CALL FLOW                                   │
└─────────────────────────────────────────────────────────────────────┘

    Caller                  Server                    Callee
       │                      │                         │
       │─── Create Call ─────▶│                         │
       │    POST /calls       │                         │
       │                      │──── Push Notification ─▶│
       │                      │     (incoming call)     │
       │                      │                         │
       │                      │◀──── Accept Call ───────│
       │                      │      WS connect         │
       │                      │                         │
       │◀── Call Accepted ────│                         │
       │    (callee joined)   │                         │
       │                      │                         │
       │─── SDP Offer ───────▶│                         │
       │    (via WS)          │─── Forward Offer ──────▶│
       │                      │                         │
       │                      │◀── SDP Answer ─────────│
       │◀── Forward Answer ───│    (via WS)            │
       │                      │                         │
       │─── ICE Candidate ───▶│◀── ICE Candidate ──────│
       │◀── ICE Candidate ────│─── ICE Candidate ─────▶│
       │                      │                         │
       │◀═══════════ P2P Connection Established ═══════▶│
       │                      │                         │
       │◀═══════════════ Video/Audio Stream ═══════════▶│
```

**Call Signal Types:**

| Signal | Direction | Purpose |
|--------|-----------|---------|
| `offer` | Caller → Callee | Initial SDP offer |
| `answer` | Callee → Caller | SDP answer |
| `ice` | Both ways | ICE candidates |
| `join` | Either | User joined call |
| `leave` | Either | User left call |
| `end` | Either | End call |

---

# 8. Authentication & Security

## 8.1 Authentication Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                    AUTHENTICATION FLOW                               │
└─────────────────────────────────────────────────────────────────────┘

1. REQUEST OTP

   Client                         Server
      │                             │
      │─── POST /auth/send-otp ────▶│
      │    { phone: "+1234567890" } │
      │                             │
      │                             │──▶ Generate 4-digit OTP
      │                             │──▶ Store in Redis (5 min TTL)
      │                             │──▶ Send via SMS provider
      │                             │
      │◀─── { message: "sent" } ────│
      │                             │

2. VERIFY OTP

   Client                         Server
      │                             │
      │─── POST /auth/verify ──────▶│
      │    { phone, otp: "1234" }   │
      │                             │
      │                             │──▶ Check Redis for OTP
      │                             │──▶ Find/Create user
      │                             │──▶ Generate JWT
      │                             │
      │◀─── {                  ─────│
      │       access_token,         │
      │       user_id,              │
      │       is_new_user,          │
      │       is_profile_complete   │
      │     }                       │

3. AUTHENTICATED REQUESTS

   Client                         Server
      │                             │
      │─── GET /profile/me ────────▶│
      │    Authorization: Bearer    │
      │    eyJhbGciOiJIUzI1Ni...    │
      │                             │
      │                             │──▶ Decode JWT
      │                             │──▶ Verify signature
      │                             │──▶ Check expiration
      │                             │──▶ Extract user_id
      │                             │
      │◀─── { profile data } ───────│
```

## 8.2 JWT Structure

```json
{
  "header": {
    "alg": "HS256",
    "typ": "JWT"
  },
  "payload": {
    "sub": "12345",              // user_id as string
    "exp": 1735689600,           // Expiration timestamp
    "iat": 1735084800            // Issued at timestamp
  },
  "signature": "HMACSHA256(...)"
}
```

**Token Configuration:**

```env
SECRET_KEY=your-super-secret-key-change-this-in-production
ACCESS_TOKEN_EXPIRE_MINUTES=10080   # 7 days
CALL_TOKEN_EXPIRE_MINUTES=15        # 15 minutes for video calls
```

## 8.3 Rate Limiting

Redis-based sliding window rate limiting:

```
┌─────────────────────────────────────────────────────────────────────┐
│                    RATE LIMITING                                     │
└─────────────────────────────────────────────────────────────────────┘

Request arrives
       │
       ▼
┌─────────────────┐
│ Get identifier  │
│ (user_id or IP) │
└────────┬────────┘
         │
         ▼
┌─────────────────┐     ┌─────────────────┐
│ Redis ZSET      │────▶│ Check count     │
│ rl:{identifier} │     │ in window       │
└─────────────────┘     └────────┬────────┘
                                 │
                    ┌────────────┴────────────┐
                    │                         │
                    ▼                         ▼
            Count < Limit              Count >= Limit
                    │                         │
                    ▼                         ▼
            ┌───────────┐             ┌───────────┐
            │ ZADD      │             │ 429 Error │
            │ timestamp │             │ Too Many  │
            │           │             │ Requests  │
            │ Proceed   │             └───────────┘
            └───────────┘

Response Headers:
  X-RateLimit-Limit: 60
  X-RateLimit-Remaining: 45
  X-RateLimit-Reset: 1735084860
```

**Configuration:**

```env
RATE_LIMIT_RPM=60     # Requests per minute
RATE_LIMIT_BURST=10   # Burst allowance
```

---

# 9. Storage & CDN

## 9.1 Storage Architecture

```
┌─────────────────────────────────────────────────────────────────────┐
│                    STORAGE FLOW                                      │
└─────────────────────────────────────────────────────────────────────┘

Photo Upload
       │
       ▼
┌─────────────────┐
│ Vision Analysis │──▶ NSFW Check, Quality Score, Face Embedding
└────────┬────────┘
         │
         ▼
┌─────────────────┐     ┌─────────────────┐
│ STORAGE_BACKEND │     │                 │
│ = "local" ?     │     │                 │
└────────┬────────┘     └────────┬────────┘
         │                       │
    ┌────┴────┐             ┌────┴────┐
    │  Local  │             │   S3    │
    │ ./uploads             │ Bucket  │
    └────┬────┘             └────┬────┘
         │                       │
         ▼                       ▼
┌─────────────────┐     ┌─────────────────┐
│ Return URL:     │     │ Return URL:     │
│ /uploads/photo..│     │ cdn.domain/...  │
└─────────────────┘     └─────────────────┘
```

## 9.2 File Categories

From [storage.rs](rust-backend/src/storage.rs):

| Category | S3 Path | Use Case |
|----------|---------|----------|
| `ProfilePhoto` | `photos/{user_id}/{uuid}.jpg` | Profile pictures |
| `VoiceIntro` | `voice/{user_id}/{uuid}.m4a` | Audio introductions |
| `Spot` | `spots/{user_id}/{uuid}.mp4` | Short video spots |
| `Reel` | `reels/{user_id}/{uuid}.mp4` | TikTok-style reels |
| `Message` | `messages/{match_id}/{uuid}.*` | Chat attachments |
| `Verification` | `verification/{user_id}/{uuid}.jpg` | ID verification |

## 9.3 S3 + CloudFront Configuration

```env
# Storage Backend
STORAGE_BACKEND=s3
UPLOAD_DIR=/var/nava/uploads        # Local fallback

# S3 Configuration
S3_BUCKET=nava-media-prod
S3_REGION=us-east-1
S3_ACCESS_KEY=AKIA...
S3_SECRET_KEY=...

# CloudFront CDN
CDN_DOMAIN=d1234567890abc.cloudfront.net

# Signed URLs (for private content)
CDN_KEY_PAIR_ID=K1234567890ABC
CDN_PRIVATE_KEY_PATH=/etc/nava/cloudfront-private-key.pem
SIGNED_URL_EXPIRY_SECS=3600
```

## 9.4 Signed URLs

For private content (verification photos), CloudFront signed URLs are generated:

```rust
pub fn get_signed_url(&self, key: &str, expiry_secs: Option<u64>) -> Result<String, StorageError> {
    let expiry = expiry_secs.unwrap_or(self.config.signed_url_expiry_secs);
    let expires = Utc::now() + Duration::seconds(expiry as i64);

    // Create CloudFront policy
    let policy = json!({
        "Statement": [{
            "Resource": format!("https://{}/{}", self.config.cdn_domain, key),
            "Condition": {
                "DateLessThan": {
                    "AWS:EpochTime": expires.timestamp()
                }
            }
        }]
    });

    // Sign with RSA-SHA1
    let signature = sign_cloudfront_policy(&self.config.cdn_private_key, &policy)?;

    Ok(format!(
        "https://{}/{}?Policy={}&Signature={}&Key-Pair-Id={}",
        self.config.cdn_domain,
        key,
        base64_url_encode(policy),
        signature,
        self.config.cdn_key_pair_id
    ))
}
```

---

# 10. Monetization System

## 10.1 Subscription Tiers

| Tier | Duration | Price | Features |
|------|----------|-------|----------|
| **Boost** | 1 hour | $2.99 | Priority visibility, unlimited swipes |
| **Daily** | 1 day | $4.99 | All Boost features |
| **Weekly** | 1 week | $9.99 | + See who likes you |
| **Monthly** | 1 month | $19.99 | + Unlimited rewinds |
| **Ultra** | 3 months | $49.99 | + Profile highlights |

## 10.2 Student Discounts

From [config](rust-backend/src/config.rs):

| University Tier | Discount |
|-----------------|----------|
| Ivy League | 30% |
| Top 50 Schools | 20% |
| State Schools | 15% |
| Other Accredited | 10% |
| Graduate Students | 15% |
| Alumni (<2 years) | 5% |

**Verification Process:**

1. User enters `.edu` email
2. System sends verification code
3. User enters code
4. University tier determined automatically
5. Discount applied to all purchases

## 10.3 RevenueCat Integration

```
┌─────────────────────────────────────────────────────────────────────┐
│                    IN-APP PURCHASE FLOW                              │
└─────────────────────────────────────────────────────────────────────┘

    Mobile App              RevenueCat              NAVA Backend
        │                       │                       │
        │─── Purchase ─────────▶│                       │
        │    (Apple/Google)     │                       │
        │                       │                       │
        │◀── Receipt ───────────│                       │
        │                       │                       │
        │─── Sync Purchase ────────────────────────────▶│
        │    POST /subscriptions/sync                   │
        │    { product_id, purchase_date, expiry }      │
        │                       │                       │
        │                       │                       │──▶ Create subscription
        │                       │                       │    record in DB
        │                       │                       │
        │                       │─── Webhook ──────────▶│
        │                       │    (renewal/cancel)   │
        │                       │                       │
        │                       │                       │──▶ Update subscription
        │                       │                       │    status
```

**Webhook Handler:**

```rust
pub async fn revenuecat_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RevenueCatWebhook>,
) -> Result<Json<Value>, AppError> {
    // Verify webhook signature
    let secret = &state.config.revenuecat_webhook_secret;
    verify_signature(&headers, &payload, secret)?;

    match payload.event.event_type.as_str() {
        "INITIAL_PURCHASE" => {
            create_subscription(&state.db, &payload).await?;
        }
        "RENEWAL" => {
            extend_subscription(&state.db, &payload).await?;
        }
        "CANCELLATION" | "EXPIRATION" => {
            cancel_subscription(&state.db, &payload).await?;
        }
        _ => {}
    }

    Ok(Json(json!({ "status": "ok" })))
}
```

---

# 11. API Reference

## 11.1 REST Endpoints

### Authentication

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/auth/send-otp` | Send OTP to phone |
| POST | `/auth/verify` | Verify OTP, get token |

### Profile

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/profile/me` | Get current user profile |
| PUT | `/profile/update` | Update profile fields |
| POST | `/profile/photos/upload` | Upload profile photo |
| POST | `/profile/voice/upload` | Upload voice intro |
| PUT | `/profile/preferences` | Update match preferences |
| PUT | `/profile/location` | Update location |

### Discovery & Matching

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/discover` | Get discovery feed |
| POST | `/discover/like` | Like a profile |
| POST | `/discover/pass` | Pass on a profile |
| GET | `/matches` | List all matches |
| GET | `/matches/{id}` | Get match details |

### Messages

| Method | Endpoint | Description |
|--------|----------|-------------|
| GET | `/messages/{match_id}` | Get conversation |
| POST | `/messages/{match_id}` | Send message |
| PUT | `/messages/{id}/read` | Mark as read |

### Reels

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/reels/create` | Upload new reel |
| GET | `/reels/feed` | Get personalized feed |
| POST | `/reels/track-view` | Track view metrics |
| POST | `/reels/like` | Like a reel |
| DELETE | `/reels/unlike` | Unlike a reel |
| POST | `/reels/message` | Send DM on reel |
| GET | `/reels/inbox` | Get received messages |

### Subscriptions

| Method | Endpoint | Description |
|--------|----------|-------------|
| POST | `/subscriptions/sync` | Sync from RevenueCat |
| POST | `/webhooks/revenuecat` | RevenueCat webhook |
| POST | `/student/verify` | Start student verification |

## 11.2 GraphQL Schema

```graphql
type Query {
  me: User
  user(id: ID!): User
  myPreferences: UserPreferences
  discover(filters: DiscoverFilters): [DiscoverProfile!]!
  matches: [Match!]!
  conversation(matchId: ID!, limit: Int, offset: Int): [Message!]!
  studentStatus: StudentStatus
}

type Mutation {
  sendOtp(phoneNumber: String!): OtpResponse!
  verifyOtp(phoneNumber: String!, otp: String!): AuthPayload!
  updateProfile(...): Boolean!
  savePreferences(input: PreferencesInput!): UserPreferences!
  likeUser(targetUserId: Int!): LikeResult!
  passUser(targetUserId: Int!): Boolean!
  sendChatMessage(matchId: String!, content: String!): Message!
  uploadVoiceIntro(voiceUrl: String, durationSeconds: Int!): VoiceIntroResult!
  trackVoicePlay(targetUserId: Int!, playDurationSeconds: Int): Boolean!
}

type User {
  id: ID!
  name: String
  age: Int
  gender: String
  bio: String
  photos: [String!]!
  interests: [String!]!
  languages: [String!]!
  isVerified: Boolean!
  isPremium: Boolean!
}

type DiscoverProfile {
  id: ID!
  name: String
  age: Int
  gender: String
  bio: String
  photos: [String!]!
  interests: [String!]!
  compatibilityScore: Float
  distanceKm: Float
  isVerified: Boolean!
  hasVoiceIntro: Boolean!
  voiceIntroUrl: String
}

type LikeResult {
  success: Boolean!
  isMutual: Boolean!
  matchId: String
  message: String!
}
```

## 11.3 WebSocket Events

### Chat WebSocket (`/ws/chat/{match_id}`)

**Client → Server:**
```json
{ "type": "message", "content": "Hello!" }
{ "type": "typing", "isTyping": true }
{ "type": "read", "messageId": "123" }
```

**Server → Client:**
```json
{ "type": "message", "senderId": 456, "content": "Hello!", "messageId": "789" }
{ "type": "typing", "senderId": 456, "isTyping": true }
{ "type": "read", "messageId": "123", "readAt": "2024-01-15T..." }
```

### Call WebSocket (`/ws/call/{call_id}`)

```json
{ "type": "offer", "sdp": "..." }
{ "type": "answer", "sdp": "..." }
{ "type": "ice", "candidate": "..." }
{ "type": "end" }
```

---

# 12. Data Flow Diagrams

## 12.1 Complete User Journey

```
┌─────────────────────────────────────────────────────────────────────┐
│                    USER JOURNEY                                      │
└─────────────────────────────────────────────────────────────────────┘

ONBOARDING:
┌────────────┐    ┌────────────┐    ┌────────────┐    ┌────────────┐
│   Phone    │───▶│   OTP      │───▶│  Profile   │───▶│  Photos    │
│   Entry    │    │  Verify    │    │   Setup    │    │  Upload    │
└────────────┘    └────────────┘    └────────────┘    └────────────┘
                                                            │
                                           ┌────────────────┘
                                           │
                                           ▼
                                    ┌────────────┐
                                    │   Voice    │
                                    │   Intro    │
                                    │ (optional) │
                                    └─────┬──────┘
                                          │
                                          ▼
DISCOVERY:                         ┌────────────┐
┌────────────┐                     │   Main     │
│   Swipe    │◀────────────────────│   App      │
│   Feed     │                     └─────┬──────┘
└─────┬──────┘                           │
      │                                  │
      │  ┌─────────┐                     │
      ├─▶│  Like   │──┐                  │
      │  └─────────┘  │                  │
      │               │                  ▼
      │  ┌─────────┐  │           ┌────────────┐
      └─▶│  Pass   │  │           │   Reel     │
         └─────────┘  │           │   Feed     │
                      │           └─────┬──────┘
                      │                 │
                      │    ┌────────────┼────────────┐
                      │    │            │            │
                      │    ▼            ▼            ▼
                      │ ┌────────┐ ┌────────┐ ┌────────┐
                      │ │  View  │ │  Like  │ │  DM    │
                      │ └────────┘ └────────┘ └───┬────┘
                      │                           │
                      │                           │
MATCHING:             ▼                           ▼
              ┌─────────────────────────────────────────┐
              │            Match Check                   │
              │   (Both liked OR conversation engaged)  │
              └─────────────────────┬───────────────────┘
                                    │
                    ┌───────────────┴───────────────┐
                    ▼                               ▼
             Not Mutual                         MATCH! 🎉
                    │                               │
                    ▼                               ▼
              Wait for                      ┌────────────┐
              other user                    │   Chat     │
                                            │   Unlocked │
                                            └─────┬──────┘
                                                  │
                                                  ▼
                                            ┌────────────┐
                                            │   Video    │
                                            │   Call     │
                                            └────────────┘
```

## 12.2 ML Training Data Flow

```
┌─────────────────────────────────────────────────────────────────────┐
│                    ML TRAINING DATA FLOW                             │
└─────────────────────────────────────────────────────────────────────┘

User Actions
     │
     ├──▶ Like ────────────────▶ interaction_events (reward: 1.0)
     │
     ├──▶ Pass ────────────────▶ interaction_events (reward: -0.1)
     │
     ├──▶ View Reel ───────────▶ reel_engagement_events
     │                               • watch_percent
     │                               • scroll_velocity
     │                               • position_in_feed
     │
     ├──▶ Like Reel ───────────▶ reel_engagement_events (reward: 2.0)
     │
     ├──▶ Send DM ─────────────▶ response_training_data
     │                               • message_features
     │                               • (tracks: got_response?)
     │
     └──▶ Match Outcome ───────▶ Updates previous events
                                     • led_to_match = TRUE

                                          │
                                          ▼
                               ┌─────────────────────┐
                               │   Batch Training    │
                               │                     │
                               │ • Contextual Bandit │
                               │ • Response Predictor│
                               │ • Content Ranker    │
                               └──────────┬──────────┘
                                          │
                                          ▼
                               ┌─────────────────────┐
                               │   Updated Models    │
                               │                     │
                               │ • bandit_arm_stats  │
                               │ • user_content_prefs│
                               │ • user_interaction_ │
                               │   _model            │
                               └─────────────────────┘
```

---

# Appendix A: Environment Variables

## Complete Configuration Reference

```env
# ============================================================
# ENVIRONMENT
# ============================================================
ENVIRONMENT=production                    # development, staging, production

# ============================================================
# DATABASE
# ============================================================
DATABASE_URL=postgresql://user:pass@host:5432/nava
DB_MAX_CONNECTIONS=100
DB_MIN_CONNECTIONS=10
DB_ACQUIRE_TIMEOUT_SECS=30
DB_IDLE_TIMEOUT_SECS=600

# ============================================================
# REDIS
# ============================================================
REDIS_URL=rediss://:password@hostname:6379

# ============================================================
# SERVER
# ============================================================
BIND_ADDR=0.0.0.0:8080
RUST_LOG=info,telugu_dating_backend=info
SHUTDOWN_TIMEOUT_SECS=30
REQUEST_TIMEOUT_SECS=30

# ============================================================
# SECURITY
# ============================================================
SECRET_KEY=your-64-char-secret-key
ACCESS_TOKEN_EXPIRE_MINUTES=10080         # 7 days
CALL_TOKEN_EXPIRE_MINUTES=15

# ============================================================
# RATE LIMITING
# ============================================================
RATE_LIMIT_RPM=60
RATE_LIMIT_BURST=10

# ============================================================
# STORAGE & CDN
# ============================================================
STORAGE_BACKEND=s3                        # local or s3
UPLOAD_DIR=/var/nava/uploads
S3_BUCKET=nava-media-prod
S3_REGION=us-east-1
S3_ACCESS_KEY=AKIA...
S3_SECRET_KEY=...
CDN_DOMAIN=d1234567890abc.cloudfront.net
CDN_KEY_PAIR_ID=K1234567890ABC
CDN_PRIVATE_KEY_PATH=/etc/nava/cloudfront-private-key.pem
SIGNED_URL_EXPIRY_SECS=3600
MAX_PHOTO_BYTES=10485760                  # 10MB
MAX_VIDEO_BYTES=52428800                  # 50MB

# ============================================================
# VISION AI
# ============================================================
VISION_ENABLED=true
VISION_MODEL_DIR=/var/nava/models
SELFIE_MATCH_THRESHOLD=0.45
SELFIE_LIVENESS_THRESHOLD=0.5

# ============================================================
# DISCOVERY
# ============================================================
DISCOVER_LIMIT=20
DEFAULT_MAX_DISTANCE_KM=50

# ============================================================
# SPOTS/REELS
# ============================================================
MAX_SPOT_DURATION_SEC=30
FREE_SPOTS_LIMIT=2
SPOT_EXPIRY_DAYS=15

# ============================================================
# PRICING (cents)
# ============================================================
PASS_PRICE_HOURLY=299
PASS_PRICE_DAILY=499
PASS_PRICE_WEEKLY=999
PASS_PRICE_MONTHLY=1999
PASS_PRICE_ULTRA=4999

# ============================================================
# STUDENT DISCOUNTS
# ============================================================
STUDENT_DISCOUNT_IVY=0.30
STUDENT_DISCOUNT_TOP50=0.20
STUDENT_DISCOUNT_STATE=0.15
STUDENT_DISCOUNT_OTHER=0.10
STUDENT_DISCOUNT_GRADUATE=0.15
STUDENT_DISCOUNT_ALUMNI=0.05

# ============================================================
# LLM LABELING
# ============================================================
LLM_ENABLED=true
LLM_API_URL=http://llm-service:8000
LLM_MODEL_NAME=llama3
LLM_BATCH_SIZE=10
LLM_MAX_RETRIES=3

# ============================================================
# FEDERATED LEARNING
# ============================================================
FL_ENABLED=true
FL_MIN_CLIENTS=10
FL_CLIENT_FRACTION=0.1
FL_LOCAL_EPOCHS=1
FL_LEARNING_RATE=0.01
FL_DP_ENABLED=true
FL_NOISE_MULTIPLIER=1.0
FL_CLIP_NORM=1.0

# ============================================================
# EMAIL (SMTP)
# ============================================================
SMTP_HOST=smtp.gmail.com
SMTP_USERNAME=your-email@gmail.com
SMTP_PASSWORD=your-app-password
SMTP_FROM=NAVA <noreply@nava.app>

# ============================================================
# REVENUECAT
# ============================================================
REVENUECAT_WEBHOOK_SECRET=your_webhook_secret
```

---

# Appendix B: Glossary

| Term | Definition |
|------|------------|
| **Contextual Bandit** | ML algorithm that balances exploration (trying new things) vs exploitation (using what works) |
| **Embedding** | Dense vector representation (e.g., 128 numbers) that captures semantic meaning |
| **Federated Learning** | Training ML models on user devices without sending raw data to server |
| **LinUCB** | Linear Upper Confidence Bound - specific bandit algorithm used for recommendations |
| **ONNX** | Open Neural Network Exchange - portable format for ML models |
| **RevenueCat** | Third-party service that handles Apple/Google in-app purchases |
| **Slate** | A batch of profiles shown together in discovery |
| **UCB** | Upper Confidence Bound - exploration bonus in bandit algorithms |
| **WebRTC** | Web Real-Time Communication - peer-to-peer video/audio protocol |

---

*Document Version: 2.0*
*Generated: January 2026*
*Based on: Actual NAVA codebase implementation*
