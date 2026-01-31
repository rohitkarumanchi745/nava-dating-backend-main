# NAVA Platform - Technical Architecture Document

## Executive Summary

NAVA is a modern dating application designed for the Telugu community, featuring AI-powered matching, video-based discovery (Reels), voice introductions, and privacy-first federated learning. The platform combines a high-performance Rust backend with a React Native mobile application.

---

## Table of Contents

1. [System Overview](#1-system-overview)
2. [Technology Stack](#2-technology-stack)
3. [Backend Architecture](#3-backend-architecture)
4. [Frontend Architecture](#4-frontend-architecture)
5. [Database Design](#5-database-design)
6. [Machine Learning System](#6-machine-learning-system)
7. [Real-time Features](#7-real-time-features)
8. [Security & Privacy](#8-security--privacy)
9. [Infrastructure & DevOps](#9-infrastructure--devops)
10. [API Design](#10-api-design)
11. [Monetization System](#11-monetization-system)
12. [Deployment Architecture](#12-deployment-architecture)

---

## 1. System Overview

### 1.1 High-Level Architecture

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              MOBILE CLIENTS                                  │
│                    (iOS / Android - React Native + Expo)                    │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                              CDN (CloudFront)                                │
│                    Photos, Videos, Voice Intros, Static Assets              │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                           LOAD BALANCER (AWS ALB)                           │
│                         SSL Termination, Health Checks                       │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                    ┌─────────────────┼─────────────────┐
                    ▼                 ▼                 ▼
            ┌───────────┐     ┌───────────┐     ┌───────────┐
            │  Backend  │     │  Backend  │     │  Backend  │
            │ Instance 1│     │ Instance 2│     │ Instance N│
            │  (Rust)   │     │  (Rust)   │     │  (Rust)   │
            └───────────┘     └───────────┘     └───────────┘
                    │                 │                 │
                    └─────────────────┼─────────────────┘
                                      │
        ┌─────────────────────────────┼─────────────────────────────┐
        ▼                             ▼                             ▼
┌───────────────┐           ┌───────────────┐           ┌───────────────┐
│  PostgreSQL   │           │     Redis     │           │      S3       │
│   (Primary)   │           │   (Cluster)   │           │   (Storage)   │
│               │           │               │           │               │
│ - Users       │           │ - Sessions    │           │ - Photos      │
│ - Matches     │           │ - Rate Limit  │           │ - Videos      │
│ - Messages    │           │ - Cache       │           │ - Voice       │
│ - ML Data     │           │ - Pub/Sub     │           │ - Reels       │
└───────────────┘           └───────────────┘           └───────────────┘
        │
        ▼
┌───────────────┐
│  PostgreSQL   │
│   (Replica)   │
│  Read-only    │
└───────────────┘
```

### 1.2 Core Features

| Feature | Description |
|---------|-------------|
| **Smart Matching** | AI-powered compatibility scoring using embeddings and contextual bandits |
| **Reel Discovery** | TikTok-style video feed for organic discovery |
| **Voice Intros** | Audio profiles for personality-first matching |
| **Real-time Chat** | WebSocket-based messaging with typing indicators |
| **Video Calls** | WebRTC-based 1:1 video calling |
| **Student Discounts** | University verification with tiered pricing |
| **Premium Features** | Subscription tiers via RevenueCat (Apple/Google IAP) |

---

## 2. Technology Stack

### 2.1 Backend

| Component | Technology | Purpose |
|-----------|------------|---------|
| **Language** | Rust | High performance, memory safety, async I/O |
| **Web Framework** | Axum 0.8 | Async HTTP server with Tower middleware |
| **Database** | PostgreSQL + SQLx | Type-safe queries, connection pooling |
| **Cache** | Redis | Sessions, rate limiting, pub/sub |
| **API** | REST + GraphQL | REST for simple ops, GraphQL for complex queries |
| **ML Runtime** | ONNX (tract) | On-server inference for vision models |

### 2.2 Frontend (Mobile)

| Component | Technology | Purpose |
|-----------|------------|---------|
| **Framework** | React Native 0.81 | Cross-platform mobile |
| **Build System** | Expo SDK 54 | Managed workflow, OTA updates |
| **Navigation** | Expo Router | File-based routing |
| **State** | React Context + Hooks | Lightweight state management |
| **Animations** | Reanimated 4 | 60fps native animations |
| **IAP** | RevenueCat | Unified Apple/Google subscriptions |

### 2.3 Infrastructure

| Component | Technology | Purpose |
|-----------|------------|---------|
| **Cloud** | AWS | Primary infrastructure |
| **CDN** | CloudFront | Media delivery, edge caching |
| **Storage** | S3 | Photos, videos, voice files |
| **Secrets** | AWS Secrets Manager | API keys, credentials |
| **Monitoring** | Prometheus + Grafana | Metrics and dashboards |

---

## 3. Backend Architecture

### 3.1 Module Structure

```
rust-backend/
├── src/
│   ├── main.rs              # Entry point, router setup
│   ├── config.rs            # Environment configuration
│   ├── state.rs             # Shared application state
│   ├── error.rs             # Error types and handling
│   ├── auth.rs              # JWT authentication
│   ├── handlers.rs          # REST API handlers (~3000 lines)
│   ├── graphql.rs           # GraphQL schema and resolvers
│   ├── models.rs            # Database models
│   ├── websocket.rs         # Real-time chat/calls
│   ├── vision.rs            # ONNX model inference
│   ├── redis_service.rs     # Caching, rate limiting, sessions
│   └── storage.rs           # S3/local file storage
├── migrations/
│   └── 001_initial_schema.sql  # Database schema
└── Cargo.toml               # Dependencies
```

### 3.2 Request Flow

```
┌──────────┐     ┌──────────┐     ┌──────────┐     ┌──────────┐
│  Client  │────▶│  Axum    │────▶│  Auth    │────▶│ Handler  │
│ Request  │     │  Router  │     │Middleware│     │ Function │
└──────────┘     └──────────┘     └──────────┘     └──────────┘
                                                         │
     ┌───────────────────────────────────────────────────┘
     │
     ▼
┌──────────┐     ┌──────────┐     ┌──────────┐
│  Rate    │────▶│  Redis   │────▶│ Response │
│  Limit   │     │  Cache   │     │          │
└──────────┘     └──────────┘     └──────────┘
     │                                  │
     ▼                                  │
┌──────────┐                            │
│PostgreSQL│◀───────────────────────────┘
│  Query   │
└──────────┘
```

### 3.3 Middleware Stack

```rust
// Applied in order (bottom to top in code)
app.layer(rate_limit_middleware)      // Redis-based rate limiting
   .layer(metrics_middleware)         // Request counting
   .layer(CompressionLayer)           // Gzip responses
   .layer(SetRequestIdLayer)          // UUID per request
   .layer(TimeoutLayer)               // 30s request timeout
   .layer(TraceLayer)                 // Structured logging
   .layer(CorsLayer)                  // Cross-origin config
```

### 3.4 Configuration Management

All configuration via environment variables with sensible defaults:

```rust
pub struct Config {
    // Server
    pub bind_addr: String,              // 0.0.0.0:8080
    pub environment: String,            // development/production

    // Database
    pub database_url: String,
    pub db_max_connections: u32,        // 100
    pub db_min_connections: u32,        // 10

    // Redis
    pub redis_url: String,

    // Security
    pub secret_key: String,
    pub access_token_expire_minutes: i64,  // 10080 (7 days)

    // Rate Limiting
    pub rate_limit_requests_per_minute: u32,  // 60
    pub rate_limit_burst: u32,                // 10

    // Storage
    pub upload_dir: String,
    pub max_photo_bytes: usize,         // 10MB
    pub max_video_bytes: usize,         // 50MB

    // ML Models
    pub vision_enabled: bool,
    pub vision_model_dir: String,

    // Pricing (cents)
    pub pass_price_monthly: i64,        // 1999 ($19.99)

    // ... more config
}
```

---

## 4. Frontend Architecture

### 4.1 Project Structure

```
telugu-dating-app-v1/
├── app/                          # Expo Router pages
│   ├── (tabs)/                   # Tab navigation
│   │   ├── index.tsx             # Home/Discover
│   │   ├── matches.tsx           # Matches list
│   │   ├── messages.tsx          # Chat list
│   │   └── profile.tsx           # User profile
│   ├── (auth)/                   # Auth flow
│   │   ├── login.tsx
│   │   └── verify.tsx
│   └── _layout.tsx               # Root layout
├── microfrontends/               # Feature modules
│   ├── onboarding/
│   │   ├── LoginScreen.tsx
│   │   ├── ProfileSetupScreen.tsx
│   │   └── PremiumScreen.tsx
│   └── discover/
│       ├── DiscoverScreen.tsx
│       └── ReelFeed.tsx
├── components/                   # Shared components
├── hooks/                        # Custom hooks
│   ├── useAuth.ts
│   ├── useSubscription.ts
│   └── usePremium.ts
├── constants/
│   └── api.ts                    # Axios instance
└── assets/
```

### 4.2 State Management

```typescript
// Auth Context - Global user state
const AuthContext = createContext<AuthState>({
  user: null,
  token: null,
  isLoading: true,
  login: async () => {},
  logout: async () => {},
  refreshProfile: async () => {},
});

// Usage in components
const { user, refreshProfile } = useAuth();
```

### 4.3 API Client

```typescript
// constants/api.ts
export const api = axios.create({
  baseURL: Constants.expoConfig?.extra?.apiUrl || 'http://127.0.0.1:8080',
  timeout: 30000,
});

// Request interceptor adds auth token
api.interceptors.request.use(async (config) => {
  const token = await SecureStore.getItemAsync('accessToken');
  if (token) {
    config.headers.Authorization = `Bearer ${token}`;
  }
  return config;
});
```

### 4.4 Navigation Flow

```
App Launch
    │
    ▼
┌─────────────────┐
│  Splash Screen  │
│  (Check Token)  │
└─────────────────┘
    │
    ├── No Token ──────────────▶ Auth Flow
    │                               │
    │                               ▼
    │                         ┌───────────┐
    │                         │   Login   │
    │                         │  (Phone)  │
    │                         └───────────┘
    │                               │
    │                               ▼
    │                         ┌───────────┐
    │                         │  Verify   │
    │                         │   OTP     │
    │                         └───────────┘
    │                               │
    │                               ▼
    │                         ┌───────────┐
    │                         │  Profile  │
    │                         │   Setup   │
    │                         └───────────┘
    │                               │
    └── Has Token ─────────────────┼────────▶ Main App
                                   │              │
                                   │              ▼
                                   │        ┌───────────┐
                                   │        │   Tabs    │
                                   └───────▶│  Layout   │
                                            └───────────┘
```

---

## 5. Database Design

### 5.1 Entity Relationship Diagram

```
┌─────────────┐       ┌─────────────┐       ┌─────────────┐
│    users    │       │   matches   │       │  messages   │
├─────────────┤       ├─────────────┤       ├─────────────┤
│ id (PK)     │◀──┬──▶│ user1_id    │◀─────▶│ match_id    │
│ phone       │   │   │ user2_id    │       │ sender_id   │
│ name        │   │   │ is_mutual   │       │ content     │
│ bio         │   │   │ ai_score    │       │ created_at  │
│ photos      │   │   │ status      │       └─────────────┘
│ embedding   │   │   └─────────────┘
└─────────────┘   │
      │           │   ┌─────────────┐       ┌─────────────┐
      │           │   │    reels    │       │ reel_views  │
      │           │   ├─────────────┤       ├─────────────┤
      │           └──▶│ user_id     │◀─────▶│ reel_id     │
      │               │ video_url   │       │ viewer_id   │
      │               │ engagement  │       │ watch_%     │
      │               └─────────────┘       └─────────────┘
      │
      │           ┌─────────────────┐
      └──────────▶│ user_preferences│
                  ├─────────────────┤
                  │ user_id         │
                  │ min_age         │
                  │ max_age         │
                  │ max_distance    │
                  │ preferred_gender│
                  └─────────────────┘
```

### 5.2 Core Tables

#### Users Table
```sql
CREATE TABLE users (
    id BIGSERIAL PRIMARY KEY,
    phone_number VARCHAR(20) UNIQUE NOT NULL,
    email VARCHAR(255) UNIQUE,
    name VARCHAR(255),
    dob DATE,
    gender VARCHAR(20),
    bio TEXT,
    interests JSONB,                    -- ["travel", "music", "cooking"]
    languages JSONB,                    -- ["Telugu", "English", "Hindi"]
    profile_photos JSONB,               -- Array of photo URLs
    voice_intro_url TEXT,
    is_verified BOOLEAN DEFAULT FALSE,
    is_student_verified BOOLEAN DEFAULT FALSE,
    attractiveness_score DOUBLE PRECISION,
    ai_embedding JSONB,                 -- 128-dim vector
    created_at TIMESTAMP DEFAULT NOW(),
    last_active TIMESTAMP DEFAULT NOW()
);
```

#### Matches Table
```sql
CREATE TABLE matches (
    id VARCHAR(36) PRIMARY KEY,
    user1_id BIGINT REFERENCES users(id),
    user2_id BIGINT REFERENCES users(id),
    user1_liked BOOLEAN,
    user2_liked BOOLEAN,
    is_mutual_match BOOLEAN DEFAULT FALSE,
    ai_compatibility_score DOUBLE PRECISION,  -- 0-1
    visual_compatibility_score DOUBLE PRECISION,
    match_reason VARCHAR(50),            -- "similar_interests", "mutual_friends"
    status VARCHAR(20) DEFAULT 'active', -- active, blocked, unmatched
    created_at TIMESTAMP DEFAULT NOW()
);
```

#### Reels Table
```sql
CREATE TABLE reels (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL REFERENCES users(id),
    video_url TEXT NOT NULL,
    thumbnail_url TEXT,
    duration_sec INTEGER,
    caption TEXT,
    tags JSONB,                          -- ["funny", "travel"]
    category VARCHAR(50),                -- lifestyle, humor, talent
    view_count INTEGER DEFAULT 0,
    like_count INTEGER DEFAULT 0,
    engagement_score DOUBLE PRECISION,
    content_embedding JSONB,             -- ML embedding
    created_at TIMESTAMP DEFAULT NOW()
);
```

### 5.3 ML-Specific Tables

```sql
-- User interaction events for training
CREATE TABLE interaction_events (
    id BIGSERIAL PRIMARY KEY,
    user_id BIGINT NOT NULL,
    target_user_id BIGINT NOT NULL,
    event_type VARCHAR(30) NOT NULL,     -- like, pass, message, view
    reward DOUBLE PRECISION,              -- ML reward signal
    created_at TIMESTAMP DEFAULT NOW()
);

-- Contextual bandit arm statistics
CREATE TABLE bandit_arm_stats (
    id BIGSERIAL PRIMARY KEY,
    arm_id VARCHAR(100) NOT NULL,
    a_matrix JSONB,                       -- LinUCB A matrix
    b_vector JSONB,                       -- LinUCB b vector
    num_pulls INTEGER DEFAULT 0,
    total_reward DOUBLE PRECISION DEFAULT 0
);

-- LLM-generated content labels
CREATE TABLE reel_llm_labels (
    id BIGSERIAL PRIMARY KEY,
    reel_id BIGINT REFERENCES reels(id),
    detected_mood VARCHAR(30),            -- happy, romantic, energetic
    personality_traits JSONB,             -- {funny: 0.8, adventurous: 0.6}
    dating_appeal_score DOUBLE PRECISION,
    content_embedding JSONB,
    llm_model VARCHAR(50),
    labeled_at TIMESTAMP DEFAULT NOW()
);
```

---

## 6. Machine Learning System

### 6.1 ML Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           ML PIPELINE OVERVIEW                               │
└─────────────────────────────────────────────────────────────────────────────┘

    ┌──────────────┐      ┌──────────────┐      ┌──────────────┐
    │   On-Device  │      │   Backend    │      │   LLM        │
    │   Inference  │      │   Vision     │      │   Labeling   │
    └──────────────┘      └──────────────┘      └──────────────┘
           │                     │                     │
           ▼                     ▼                     ▼
    ┌──────────────┐      ┌──────────────┐      ┌──────────────┐
    │ User Prefs   │      │ NSFW Filter  │      │ Content      │
    │ Embeddings   │      │ Face Verify  │      │ Analysis     │
    │ Response     │      │ Liveness     │      │ Personality  │
    │ Prediction   │      │ Quality      │      │ Dating Appeal│
    └──────────────┘      └──────────────┘      └──────────────┘
           │                     │                     │
           └─────────────────────┼─────────────────────┘
                                 │
                                 ▼
                    ┌──────────────────────┐
                    │  Recommendation      │
                    │  Engine              │
                    │  (Contextual Bandit) │
                    └──────────────────────┘
                                 │
                                 ▼
                    ┌──────────────────────┐
                    │  Discovery Feed      │
                    │  Personalized        │
                    │  Rankings            │
                    └──────────────────────┘
```

### 6.2 Vision Models (ONNX)

The backend runs 5 ONNX models for image/video analysis:

| Model | Purpose | Output |
|-------|---------|--------|
| **NSFW Detector** | Content moderation | Safe/Unsafe probability |
| **FER+ (Emotion)** | Facial expression | Happy, Sad, Neutral, etc. |
| **NIMA (Aesthetics)** | Photo quality | 1-10 quality score |
| **ArcFace** | Face embedding | 512-dim vector for verification |
| **MiniFASNet** | Liveness detection | Real/Spoof probability |

```rust
// Vision analysis pipeline
pub struct VisionAnalyzer {
    nsfw_model: SimplePlan<TypedFact>,
    fer_model: SimplePlan<TypedFact>,
    nima_model: SimplePlan<TypedFact>,
    arcface_model: SimplePlan<TypedFact>,
    liveness_model: SimplePlan<TypedFact>,
}

impl VisionAnalyzer {
    pub fn analyze_photo(&self, image: &DynamicImage) -> PhotoAnalysis {
        // 1. Check NSFW
        let nsfw_score = self.run_nsfw(image);
        if nsfw_score > 0.7 {
            return PhotoAnalysis::rejected("NSFW content detected");
        }

        // 2. Detect face and extract embedding
        let face_embedding = self.run_arcface(image);

        // 3. Analyze quality
        let quality_score = self.run_nima(image);

        // 4. Detect emotion
        let emotion = self.run_fer(image);

        PhotoAnalysis {
            nsfw_score,
            quality_score,
            emotion,
            face_embedding,
        }
    }
}
```

### 6.3 Recommendation Engine (Contextual Bandits)

We use **LinUCB** (Linear Upper Confidence Bound) for personalized discovery:

```
For each user U viewing discovery feed:
    1. Get user feature vector x_u (embedding + preferences)
    2. For each candidate profile P:
        a. Get profile features x_p
        b. Combine: x = concat(x_u, x_p, x_u * x_p)
        c. Compute UCB score: θᵀx + α√(xᵀA⁻¹x)
    3. Rank by UCB score (exploration + exploitation)
    4. Show top-K profiles
    5. Observe user action (like/pass/message)
    6. Update arm statistics
```

**Database Schema for Bandits:**
```sql
CREATE TABLE bandit_arm_stats (
    arm_id VARCHAR(100),      -- "age_25_30" or "interest_travel"
    a_matrix JSONB,           -- Inverse covariance matrix
    b_vector JSONB,           -- Cumulative reward vector
    num_pulls INTEGER,
    total_reward DOUBLE PRECISION
);
```

### 6.4 LLM Auto-Labeling System

Asynchronous content labeling using LLaMA 3:

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│ New Reel    │────▶│ Labeling    │────▶│ LLM Worker  │
│ Uploaded    │     │ Queue       │     │ (Batch)     │
└─────────────┘     └─────────────┘     └─────────────┘
                                               │
                                               ▼
                                        ┌─────────────┐
                                        │ Generated   │
                                        │ Labels:     │
                                        │ - Mood      │
                                        │ - Topics    │
                                        │ - Quality   │
                                        │ - Appeal    │
                                        └─────────────┘
```

**Labels Generated:**
- `detected_mood`: happy, romantic, energetic, calm
- `detected_topics`: ["cooking", "travel", "pets"]
- `personality_traits`: {funny: 0.8, adventurous: 0.6}
- `dating_appeal_score`: 0.0 - 1.0
- `conversation_starters`: ["Nice cooking!", "Where is that?"]

### 6.5 Federated Learning (Privacy-First)

User preference models trained on-device:

```
┌─────────────────────────────────────────────────────────────────┐
│                    FEDERATED LEARNING FLOW                       │
└─────────────────────────────────────────────────────────────────┘

Round N:
┌──────────┐   ┌──────────┐   ┌──────────┐   ┌──────────┐
│ Device 1 │   │ Device 2 │   │ Device 3 │   │ Device N │
│          │   │          │   │          │   │          │
│ Local    │   │ Local    │   │ Local    │   │ Local    │
│ Training │   │ Training │   │ Training │   │ Training │
└────┬─────┘   └────┬─────┘   └────┬─────┘   └────┬─────┘
     │              │              │              │
     │   ┌──────────┴──────────┐   │              │
     │   │  Weight Deltas      │   │              │
     │   │  (Encrypted + DP)   │   │              │
     └───┴──────────┬──────────┴───┴──────────────┘
                    │
                    ▼
          ┌─────────────────┐
          │  Server         │
          │  FedAvg         │
          │  Aggregation    │
          └─────────────────┘
                    │
                    ▼
          ┌─────────────────┐
          │  Updated Global │
          │  Model v(N+1)   │
          └─────────────────┘
```

**Privacy Guarantees:**
- Raw swipe data never leaves device
- Differential Privacy (ε=1.0, δ=10⁻⁵)
- Secure aggregation with gradient clipping

**Database Tables:**
```sql
-- Client registration
CREATE TABLE fl_clients (
    user_id BIGINT,
    device_id VARCHAR(64),
    compute_tier VARCHAR(20),    -- low, medium, high
    reliability_score DOUBLE PRECISION
);

-- Training rounds
CREATE TABLE fl_rounds (
    round_number INTEGER,
    model_type VARCHAR(50),      -- recommendation, response_prediction
    global_weights JSONB,
    differential_privacy BOOLEAN,
    noise_multiplier DOUBLE PRECISION
);
```

### 6.6 Response Prediction Model

Predicts likelihood of getting a response when messaging:

**Features:**
- Sender profile embedding
- Receiver profile embedding
- Reel content features (if messaging on reel)
- Message characteristics (length, has_question, sentiment)
- Historical response rate

**Training Data:**
```sql
CREATE TABLE response_training_data (
    sender_id BIGINT,
    receiver_id BIGINT,
    got_response BOOLEAN,        -- Label
    response_time_sec INTEGER,
    conversation_continued BOOLEAN,
    led_to_match BOOLEAN,
    reward DOUBLE PRECISION      -- Computed reward
);
```

---

## 7. Real-time Features

### 7.1 WebSocket Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     WEBSOCKET CONNECTIONS                        │
└─────────────────────────────────────────────────────────────────┘

    Client A                 Server                  Client B
        │                      │                        │
        │──── Connect ────────▶│                        │
        │     (match_id)       │                        │
        │                      │◀──── Connect ──────────│
        │                      │      (match_id)        │
        │                      │                        │
        │──── Message ────────▶│                        │
        │                      │──── Broadcast ────────▶│
        │                      │                        │
        │──── Typing ─────────▶│                        │
        │                      │──── Typing Event ─────▶│
        │                      │                        │
        │◀──── Message ────────│◀──── Message ──────────│
        │                      │                        │
```

### 7.2 Chat Room Management

```rust
pub struct ChatRooms {
    // match_id -> broadcast channel
    rooms: HashMap<String, broadcast::Sender<ChatMessage>>,
}

impl ChatRooms {
    pub fn get_or_create(&mut self, match_id: &str) -> broadcast::Sender<ChatMessage> {
        self.rooms
            .entry(match_id.to_string())
            .or_insert_with(|| broadcast::channel(100).0)
            .clone()
    }
}
```

### 7.3 Video Call Signaling

```
┌─────────────────────────────────────────────────────────────────┐
│                   WEBRTC SIGNALING FLOW                          │
└─────────────────────────────────────────────────────────────────┘

    Caller                    Server                   Callee
       │                        │                        │
       │── Create Call ────────▶│                        │
       │   (callee_id)          │                        │
       │                        │──── Push Notification ─▶│
       │                        │                        │
       │                        │◀───── Accept Call ─────│
       │                        │                        │
       │◀───── Call Accepted ───│                        │
       │                        │                        │
       │── SDP Offer ──────────▶│                        │
       │                        │──── SDP Offer ────────▶│
       │                        │                        │
       │                        │◀───── SDP Answer ─────│
       │◀───── SDP Answer ─────│                        │
       │                        │                        │
       │── ICE Candidate ──────▶│◀───── ICE Candidate ──│
       │◀───── ICE Candidate ──│──── ICE Candidate ────▶│
       │                        │                        │
       │◀════════ P2P Media Stream ════════════════════▶│
```

---

## 8. Security & Privacy

### 8.1 Authentication Flow

```
┌─────────────────────────────────────────────────────────────────┐
│                    AUTHENTICATION FLOW                           │
└─────────────────────────────────────────────────────────────────┘

1. Phone Verification:

   Client                      Server                    SMS Provider
      │                          │                            │
      │── POST /auth/send-otp ──▶│                            │
      │   {phone: "+1..."}       │── Send OTP ───────────────▶│
      │                          │                            │
      │◀── {message: "sent"} ────│                            │
      │                          │                            │
      │── POST /auth/verify ────▶│                            │
      │   {phone, otp}           │                            │
      │                          │ Verify OTP (Redis)         │
      │                          │ Create/Get User            │
      │                          │ Generate JWT               │
      │◀── {token, user} ────────│                            │

2. Subsequent Requests:

   Client                      Server
      │                          │
      │── GET /profile/me ──────▶│
      │   Authorization: Bearer  │
      │                          │ Decode JWT
      │                          │ Verify signature
      │                          │ Check expiry
      │                          │ Extract user_id
      │◀── {profile data} ───────│
```

### 8.2 JWT Structure

```json
{
  "header": {
    "alg": "HS256",
    "typ": "JWT"
  },
  "payload": {
    "sub": "12345",        // user_id
    "exp": 1735689600,     // Expiry (7 days)
    "iat": 1735084800      // Issued at
  }
}
```

### 8.3 Rate Limiting

Redis-based sliding window rate limiting:

```rust
async fn check_rate_limit(
    redis: &RedisService,
    identifier: &str,      // "user:123" or "ip:1.2.3.4"
    max_requests: u32,     // 60
    window_secs: u64,      // 60
) -> (bool, u32, u64) {
    // Use sorted set with timestamps
    // ZREMRANGEBYSCORE to remove old entries
    // ZADD current timestamp
    // ZCARD to count
    // Return (allowed, remaining, reset_time)
}
```

**Rate Limit Headers:**
```
X-RateLimit-Limit: 60
X-RateLimit-Remaining: 45
X-RateLimit-Reset: 1735084860
```

### 8.4 Data Privacy

| Data Type | Storage | Encryption | Retention |
|-----------|---------|------------|-----------|
| Phone numbers | PostgreSQL | Hashed | Account lifetime |
| Messages | PostgreSQL | At-rest | 1 year |
| Photos | S3 | At-rest (SSE-S3) | Until deleted |
| Location | PostgreSQL | Fuzzy (±500m) | 30 days |
| ML Embeddings | PostgreSQL | At-rest | Account lifetime |

---

## 9. Infrastructure & DevOps

### 9.1 AWS Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                        AWS ARCHITECTURE                          │
└─────────────────────────────────────────────────────────────────┘

                         ┌─────────────────┐
                         │   CloudFront    │
                         │   Distribution  │
                         └────────┬────────┘
                                  │
         ┌────────────────────────┼────────────────────────┐
         │                        │                        │
         ▼                        ▼                        ▼
┌─────────────────┐    ┌─────────────────┐    ┌─────────────────┐
│   S3 Bucket     │    │   API Gateway   │    │   S3 Bucket     │
│   (Static)      │    │   (REST/WS)     │    │   (Media)       │
│                 │    └────────┬────────┘    │                 │
│  - App assets   │             │             │  - Photos       │
│  - Web builds   │             │             │  - Videos       │
└─────────────────┘             │             │  - Voice        │
                                │             └─────────────────┘
                                ▼
                    ┌─────────────────────┐
                    │   ALB              │
                    │   (Load Balancer)   │
                    └──────────┬──────────┘
                               │
         ┌─────────────────────┼─────────────────────┐
         │                     │                     │
         ▼                     ▼                     ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│   ECS Task 1    │ │   ECS Task 2    │ │   ECS Task N    │
│   (Rust API)    │ │   (Rust API)    │ │   (Rust API)    │
└────────┬────────┘ └────────┬────────┘ └────────┬────────┘
         │                   │                   │
         └───────────────────┼───────────────────┘
                             │
         ┌───────────────────┼───────────────────┐
         │                   │                   │
         ▼                   ▼                   ▼
┌─────────────────┐ ┌─────────────────┐ ┌─────────────────┐
│   RDS           │ │   ElastiCache   │ │   Secrets       │
│   PostgreSQL    │ │   Redis         │ │   Manager       │
│   (Multi-AZ)    │ │   (Cluster)     │ │                 │
└─────────────────┘ └─────────────────┘ └─────────────────┘
```

### 9.2 Environment Configuration

**Development (.env):**
```env
ENVIRONMENT=development
DATABASE_URL=postgresql://nava:nava@localhost:5432/nava
REDIS_URL=redis://127.0.0.1:6379
STORAGE_BACKEND=local
```

**Production (.env.production):**
```env
ENVIRONMENT=production
DATABASE_URL=postgresql://nava:***@rds.amazonaws.com:5432/nava_prod?sslmode=require
REDIS_URL=rediss://:***@elasticache.amazonaws.com:6379
STORAGE_BACKEND=s3
S3_BUCKET=nava-media-prod
CDN_DOMAIN=d123abc.cloudfront.net
```

### 9.3 Deployment Pipeline

```
┌─────────────────────────────────────────────────────────────────┐
│                     CI/CD PIPELINE                               │
└─────────────────────────────────────────────────────────────────┘

  Git Push          Build & Test         Deploy              Monitor
     │                   │                  │                   │
     ▼                   ▼                  ▼                   ▼
┌─────────┐        ┌─────────┐        ┌─────────┐        ┌─────────┐
│ GitHub  │───────▶│ Actions │───────▶│ ECS     │───────▶│ CloudW  │
│ Push    │        │         │        │ Deploy  │        │ atch    │
│         │        │ cargo   │        │         │        │         │
│ main    │        │ test    │        │ Blue/   │        │ Alarms  │
│ branch  │        │ clippy  │        │ Green   │        │ Logs    │
└─────────┘        └─────────┘        └─────────┘        └─────────┘

Mobile App:
┌─────────┐        ┌─────────┐        ┌─────────┐
│ Git     │───────▶│ EAS     │───────▶│ App     │
│ Push    │        │ Build   │        │ Store   │
│         │        │         │        │ Review  │
│ release │        │ iOS     │        │         │
│ branch  │        │ Android │        │ Publish │
└─────────┘        └─────────┘        └─────────┘
```

---

## 10. API Design

### 10.1 REST Endpoints

| Method | Endpoint | Description |
|--------|----------|-------------|
| **Auth** | | |
| POST | `/auth/send-otp` | Send OTP to phone |
| POST | `/auth/verify` | Verify OTP, get token |
| **Profile** | | |
| GET | `/profile/me` | Get current user profile |
| PUT | `/profile/update` | Update profile fields |
| PUT | `/profile/preferences` | Update match preferences |
| POST | `/profile/photos/upload` | Upload profile photo |
| **Discovery** | | |
| GET | `/discover` | Get discovery feed |
| POST | `/discover/like/{user_id}` | Like a profile |
| POST | `/discover/pass/{user_id}` | Pass on a profile |
| **Matches** | | |
| GET | `/matches` | List all matches |
| GET | `/matches/{match_id}` | Get match details |
| **Messages** | | |
| GET | `/messages/{match_id}` | Get conversation |
| POST | `/messages/{match_id}` | Send message |
| **Reels** | | |
| POST | `/reels/create` | Upload new reel |
| GET | `/reels/feed` | Get reel feed |
| POST | `/reels/{reel_id}/like` | Like a reel |
| POST | `/reels/{reel_id}/message` | DM on reel |
| **Subscriptions** | | |
| POST | `/subscriptions/sync` | Sync IAP from app |
| POST | `/webhooks/revenuecat` | RevenueCat webhook |

### 10.2 GraphQL Schema

```graphql
type Query {
  me: User
  discover(limit: Int = 20): [ProfileCard!]!
  matches: [Match!]!
  messages(matchId: ID!): [Message!]!
  reels(category: String): [Reel!]!
}

type Mutation {
  updateProfile(input: ProfileInput!): User!
  like(userId: ID!): LikeResult!
  sendMessage(matchId: ID!, content: String!): Message!
  uploadReel(input: ReelInput!): Reel!
}

type Subscription {
  messageReceived(matchId: ID!): Message!
  newMatch: Match!
}

type User {
  id: ID!
  name: String!
  age: Int!
  bio: String
  photos: [String!]!
  interests: [String!]!
  isPremium: Boolean!
  compatibilityScore(targetUserId: ID!): Float
}

type ProfileCard {
  user: User!
  distance: Float
  commonInterests: [String!]!
  aiScore: Float
}
```

### 10.3 WebSocket Events

**Chat WebSocket (`/ws/chat/{match_id}`):**

```typescript
// Client -> Server
{ type: "message", content: "Hello!" }
{ type: "typing", isTyping: true }
{ type: "read", messageId: "123" }

// Server -> Client
{ type: "message", senderId: 456, content: "Hello!", messageId: "789" }
{ type: "typing", senderId: 456, isTyping: true }
{ type: "read", messageId: "123", readAt: "2024-01-15T..." }
```

**Call WebSocket (`/ws/call/{call_id}`):**

```typescript
// Signaling
{ type: "offer", sdp: "..." }
{ type: "answer", sdp: "..." }
{ type: "ice", candidate: "..." }
{ type: "end" }
```

---

## 11. Monetization System

### 11.1 Subscription Tiers

| Tier | Price | Duration | Features |
|------|-------|----------|----------|
| **Boost** | $2.99 | 1 hour | Priority visibility, unlimited swipes |
| **Daily** | $4.99 | 1 day | All Boost features |
| **Weekly** | $9.99 | 1 week | All Daily + See who likes you |
| **Monthly** | $19.99 | 1 month | All Weekly + Unlimited rewinds |
| **Ultra** | $49.99 | 3 months | All Monthly + Profile highlights |

### 11.2 Student Discounts

| University Tier | Discount |
|-----------------|----------|
| Ivy League | 30% |
| Top 50 | 20% |
| State Schools | 15% |
| Other Accredited | 10% |
| Graduate Students | 15% |
| Alumni (<2 years) | 5% |

### 11.3 RevenueCat Integration

```
┌─────────────────────────────────────────────────────────────────┐
│                   IN-APP PURCHASE FLOW                           │
└─────────────────────────────────────────────────────────────────┘

    Mobile App              RevenueCat             NAVA Backend
        │                       │                       │
        │── Purchase ──────────▶│                       │
        │   (Apple/Google)      │                       │
        │                       │                       │
        │◀── Receipt ───────────│                       │
        │                       │                       │
        │── Sync ──────────────────────────────────────▶│
        │   {product_id,        │                       │
        │    purchase_date,     │                       │ Update DB
        │    expiration}        │                       │
        │                       │                       │
        │                       │── Webhook ───────────▶│
        │                       │   (renewal/cancel)    │
        │                       │                       │
```

**Frontend Hook:**
```typescript
const {
  isPremium,
  purchasePackage,
  restorePurchases,
  getPackageForPlan,
} = useSubscription();

// Purchase flow
const handlePurchase = async (planId: string) => {
  const pkg = getPackageForPlan(planId);
  if (pkg) {
    await purchasePackage(pkg);
  }
};
```

**Backend Webhook:**
```rust
pub async fn revenuecat_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RevenueCatWebhook>,
) -> Result<Json<Value>, AppError> {
    // Verify webhook signature
    // Handle event types:
    // - INITIAL_PURCHASE
    // - RENEWAL
    // - CANCELLATION
    // - EXPIRATION
    // Update user_subscriptions table
}
```

---

## 12. Deployment Architecture

### 12.1 Docker Configuration

```dockerfile
# Rust Backend
FROM rust:1.75-alpine AS builder
WORKDIR /app
COPY . .
RUN cargo build --release

FROM alpine:3.19
COPY --from=builder /app/target/release/telugu-dating-backend /usr/local/bin/
COPY --from=builder /app/models /var/nava/models
EXPOSE 8080
CMD ["telugu-dating-backend"]
```

### 12.2 Mobile App Build

```json
// eas.json
{
  "build": {
    "development": {
      "developmentClient": true,
      "distribution": "internal"
    },
    "preview": {
      "distribution": "internal"
    },
    "production": {
      "autoIncrement": true
    }
  },
  "submit": {
    "production": {
      "ios": {
        "appleId": "...",
        "ascAppId": "..."
      },
      "android": {
        "serviceAccountKeyPath": "./google-services.json"
      }
    }
  }
}
```

### 12.3 Scaling Strategy

| Component | Scaling Approach | Trigger |
|-----------|------------------|---------|
| API Servers | Horizontal (ECS) | CPU > 70% |
| PostgreSQL | Vertical + Read Replicas | Connections > 80% |
| Redis | Cluster mode | Memory > 70% |
| S3 | Automatic | N/A |
| CloudFront | Automatic | N/A |

---

## Appendix A: File Inventory

### Backend Files

| File | Lines | Purpose |
|------|-------|---------|
| `main.rs` | ~600 | Server setup, routing, middleware |
| `handlers.rs` | ~3000 | All REST API handlers |
| `graphql.rs` | ~800 | GraphQL schema and resolvers |
| `models.rs` | ~500 | Database models |
| `auth.rs` | ~200 | JWT encoding/decoding |
| `vision.rs` | ~400 | ONNX model inference |
| `redis_service.rs` | ~550 | Caching, rate limiting |
| `storage.rs` | ~540 | S3/local file storage |
| `websocket.rs` | ~300 | Real-time chat/calls |
| `config.rs` | ~400 | Environment configuration |
| `state.rs` | ~200 | Shared app state |
| `error.rs` | ~100 | Error types |
| **migrations/** | ~1050 | Database schema |

### Frontend Files

| Directory | Files | Purpose |
|-----------|-------|---------|
| `app/` | 15+ | Expo Router pages |
| `microfrontends/` | 10+ | Feature modules |
| `hooks/` | 5+ | Custom React hooks |
| `components/` | 20+ | Shared UI components |
| `constants/` | 3 | API, theme, config |

---

## Appendix B: Environment Variables Reference

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `DATABASE_URL` | Yes | - | PostgreSQL connection string |
| `REDIS_URL` | No | `redis://127.0.0.1:6379` | Redis connection |
| `SECRET_KEY` | Yes | - | JWT signing key (32+ chars in prod) |
| `ENVIRONMENT` | No | `development` | `development`/`production` |
| `BIND_ADDR` | No | `0.0.0.0:8080` | Server listen address |
| `STORAGE_BACKEND` | No | `local` | `local` or `s3` |
| `S3_BUCKET` | If S3 | - | S3 bucket name |
| `CDN_DOMAIN` | No | - | CloudFront domain |
| `VISION_ENABLED` | No | `true` | Enable ML models |
| `RATE_LIMIT_RPM` | No | `60` | Requests per minute |
| `REVENUECAT_WEBHOOK_SECRET` | No | - | IAP webhook verification |

---

## Appendix C: Glossary

| Term | Definition |
|------|------------|
| **Contextual Bandit** | ML algorithm balancing exploration/exploitation for recommendations |
| **Embedding** | Dense vector representation of a user/content for similarity |
| **Federated Learning** | Training ML models on-device without centralizing data |
| **LinUCB** | Linear Upper Confidence Bound - bandit algorithm |
| **ONNX** | Open Neural Network Exchange - portable ML model format |
| **RevenueCat** | Third-party service managing Apple/Google subscriptions |
| **WebRTC** | Web Real-Time Communication - peer-to-peer video/audio |

---

*Document Version: 1.0*
*Last Updated: January 2026*
*Author: NAVA Engineering Team*
