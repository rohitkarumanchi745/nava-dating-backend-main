# Nava - High-Performance Dating Platform Backend

Production backend powering the Nava dating apps (SwiftUI iOS + React Native cross-platform). Built with Rust/Axum microservices, event-driven Kafka architecture, real-time WebSocket chat & calling, and ML-powered matching.

## Architecture

```
            ┌───────────────┐     ┌────────────────────┐
            │   iOS App     │     │  React Native App  │
            │   (SwiftUI)   │     │   (Expo / RN)      │
            └───────┬───────┘     └─────────┬──────────┘
                    └──────────┬────────────┘
                               │
                  ┌────────────▼────────────┐
                  │    API Gateway (Rust)    │
                  │  JWT · Rate Limiting     │
                  │  GraphQL · REST · WS     │
                  └────────────┬────────────┘
        ┌──────────┬───────────┼───────────┬──────────┐
        ▼          ▼           ▼           ▼          ▼
   ┌────────┐ ┌────────┐ ┌─────────┐ ┌────────┐ ┌────────┐
   │  Auth  │ │  User  │ │  Match  │ │  Chat  │ │Payment │
   │Service │ │Service │ │ Service │ │Service │ │Service │
   └───┬────┘ └───┬────┘ └────┬────┘ └───┬────┘ └───┬────┘
       └──────────┴────────────┴──────────┴──────────┘
                               │
                  ┌────────────▼────────────┐
                  │     Apache Kafka        │
                  │    Event Streaming      │
                  └────────────┬────────────┘
            ┌──────────────────┼──────────────────┐
            ▼                  ▼                   ▼
      ┌───────────┐    ┌───────────┐       ┌───────────┐
      │Notification│    │ Analytics │       │    ML     │
      │  Service   │    │  Service  │       │  Service  │
      └───────────┘    └───────────┘       └───────────┘

 ┌──────────┐ ┌───────┐ ┌───────┐ ┌─────┐ ┌──────────┐
 │PostgreSQL│ │ Redis │ │ Neo4j │ │ S3  │ │ClickHouse│
 │(Users,   │ │(Cache,│ │(Graph │ │(Media│ │(Analytics│
 │ Payments)│ │ OTP)  │ │ Rel.) │ │ CDN)│ │  OLAP)   │
 └──────────┘ └───────┘ └───────┘ └─────┘ └──────────┘
```

## Client Apps

| App | Stack | Repo |
|-----|-------|------|
| **iOS** | SwiftUI, StoreKit 2 | `navaswift-ui-iOS-` |
| **Cross-Platform** | React Native, Expo, RevenueCat | `nava-dating-app-v1-main` |
| **Ambassador Dashboard** | React, TypeScript, Vite | Included in this repo |

## API Contract

All endpoints require `Authorization: Bearer <jwt>` header unless noted.

### Authentication (GraphQL)

| Operation | Type | Description |
|-----------|------|-------------|
| `sendOtp(phoneNumber)` | Mutation | Send OTP to phone (no auth required) |
| `verifyOtp(phoneNumber, otp)` | Mutation | Verify OTP → returns `accessToken`, `userId`, `isNewUser`, `isProfileComplete` |
| `me` | Query | Get authenticated user profile (bootstraps app state) |

### Profile Management

| Endpoint | Method | Description |
|----------|--------|-------------|
| `updateProfile(...)` | GraphQL Mutation | Update name, dob, gender, bio, location, interests, languages, heightCm, lookingFor, profession |
| `uploadVoiceIntro(voice)` | GraphQL Mutation (multipart) | Upload voice intro (m4a, max 15s) |
| `verifySelfie(selfie)` | GraphQL Mutation (multipart) | Selfie liveness verification → `{ verified, confidence, failureReasons[] }` |
| `/voice-intro` | POST (multipart) | Upload voice intro (iOS REST fallback) |
| `/reels` | POST (multipart) | Upload video reel (mp4, max 30s) + caption |
| `/spots` | POST (multipart) | Upload video spot (React Native) |
| `/verify/selfie` | POST (multipart) | Selfie verification (iOS REST fallback) |
| `/update-bio` | POST | Quick bio update |
| `/profile/me` | POST | Get/load user profile |

### Discovery & Matching (GraphQL)

| Operation | Type | Description |
|-----------|------|-------------|
| `discover(filters: { useAi, limit })` | Query | Get swipeable profiles with AI `compatibilityScore` (0-100) |
| `likeUser(targetUserId)` | Mutation | Like a profile → `{ success, isMutual, matchId }` |
| `passUser(targetUserId)` | Mutation | Skip a profile |
| `matches` | Query | Get all matches with partner details (mutual + received likes) |

### Preferences (GraphQL)

| Operation | Type | Description |
|-----------|------|-------------|
| `myPreferences` | Query | Get saved dating preferences |
| `savePreferences(input)` | Mutation | Save preferences: `minAge`, `maxAge`, `maxDistanceKm`, `preferredGenders[]`, `onlyVerified`, `onlyStudents` |

### Real-Time Chat (WebSocket + GraphQL)

**WebSocket:** `ws(s)://api.nava.app/ws/chat?match_id=<id>&token=<jwt>`

```json
{ "type": "message", "content": "Hi there!" }
{ "type": "typing" }
{ "type": "read", "message_id": 12345 }

// Server response
{ "type": "message", "sender_id": 123, "content": "...", "message_id": 456, "timestamp": "..." }
```

| Operation | Type | Description |
|-----------|------|-------------|
| `conversation(matchId, limit, offset)` | GraphQL Query | Load message history with `isRead` status |
| `sendChatMessage(matchId, content)` | GraphQL Mutation | Persist message to DB |

### Audio/Video Calling (WebSocket Signaling)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/calls` | POST | Initiate call → returns `{ callId, token, signalingUrl }` |

**WebSocket:** `ws(s)://api.nava.app/ws/call?call_id=<id>&match_id=<id>&token=<jwt>`

```json
// Signaling messages
{ "type": "ringing" }
{ "type": "participant_joined" }
{ "type": "participant_left" }
```

Call states: `idle → connecting → ringing → active → idle`

### Reels / Video Feed (REST)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/reels/feed` | GET | Vertical video feed with user info, like/view counts |
| `/reels/like` | POST | Like a reel |
| `/reels/unlike` | POST | Unlike a reel |
| `/reels/view` | POST | Track reel view |
| `/reels/message` | POST | Send DM from a reel |

### Payments

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/api/payments/verify-apple` | POST | Verify Apple StoreKit receipt (iOS) |
| `/subscriptions/sync` | POST | Sync subscription status with RevenueCat (React Native) |
| `/location/purchase-pass` | POST | Purchase location-based pass with idempotency key |

**Subscription Tiers:**
| Tier | Features |
|------|----------|
| **Gold** | Unlimited likes, see who likes you, 5 super likes/day, 1 boost/month, advanced filters |
| **Platinum** | All Gold + priority matching, read receipts, weekly boost, undo swipe |
| **Ultra** | All Platinum + priority support, exclusive events, unlimited super likes |

**Consumables:** Boosts (1hr, 5x), Super Likes (5x), Spotlight (1hr), Daily/Weekly passes

**Product IDs (RevenueCat):** `nava_boost_1hr`, `nava_daily_pass`, `nava_weekly_sub`, `nava_monthly_sub`, `nava_ultra_3mo`

**Product IDs (StoreKit):** `com.nava.gold_monthly`, `com.nava.platinum_monthly`, `com.nava.ultra_monthly`, `com.nava.boost_1`, `com.nava.super_like_5`

### Verification (REST)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/student/verify` | POST | Send OTP to .edu email |
| `/student/verify-otp` | POST | Verify student OTP → adds verified badge |
| `/student/status` | GET | Check student verification status |
| `/verify/selfie` | POST | Selfie liveness + face verification |

### Location (REST)

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/location/update` | POST | Update GPS coordinates, city, state, country, accuracy |
| `/me/location` | GET | Get stored user location |
| `/location/purchase-pass` | POST | Purchase location-based pass (`boost_1hr`, `daily_pass`, etc.) |

## Tech Stack

| Layer | Technologies |
|-------|-------------|
| **Backend** | Rust, Axum, Tokio, SQLx, async-graphql |
| **Real-Time** | WebSocket pub/sub (chat + call signaling), typing indicators, read receipts |
| **Databases** | PostgreSQL 15, Redis 7, Neo4j 5, ClickHouse |
| **Event Streaming** | Apache Kafka (user, payment, match, chat, analytics topics) |
| **Verification** | ONNX Runtime (selfie liveness detection) |
| **File Storage** | AWS S3 + CloudFront CDN (photos, voice intros, reels) |
| **Payments** | Apple StoreKit 2 (iOS), RevenueCat (React Native), Razorpay, Stripe |
| **Infrastructure** | Docker, Kubernetes, Kustomize, Prometheus, Grafana |
| **iOS** | SwiftUI, Combine, StoreKit 2 |
| **Cross-Platform** | React Native, Expo, TypeScript |
| **Dashboard** | React, TypeScript, Vite, Tailwind CSS |

## Microservices

| Service | Port | Responsibility |
|---------|------|----------------|
| API Gateway | 8000 | Routing, JWT validation, rate limiting, GraphQL/REST/WS |
| Auth Service | 8001 | Phone OTP, JWT tokens, session management |
| User Service | 8002 | Profile CRUD, photos, voice intros, preferences |
| Match Service | 8003 | Discovery algorithm, swipes, AI compatibility scoring |
| Chat Service | 8004 | WebSocket messaging + call signaling, typing indicators, read receipts |
| Payment Service | 8005 | Apple/RevenueCat receipt validation, subscriptions, webhooks |
| Ambassador Service | 8006 | Referral program, partner tracking |
| Notification Service | — | FCM/APNs push notifications |
| Analytics Service | 8008 | Kafka consumer → ClickHouse OLAP, DAU, event counts |

## Kafka Event Topics

```
user.events        →  registration, verification, profile updates, premium activation
payment.events     →  orders, completions, subscriptions, refunds
match.events       →  swipe.like, swipe.pass, match.created, match.unmatched
chat.events        →  message.sent, message.read
analytics.events   →  generic tracking
notification.cmds  →  push, email, SMS commands
dlq.events         →  dead letter queue for failed events
```

## Data Models

### UserProfile
```
id, name, phoneNumber, dob, age, gender, bio, location,
profession, professionCategory, professionTitle,
interests[], languages[], photos[], heightCm,
lookingFor (long_term | short_term | casual | friendship | figuring_out),
voiceIntroUrl, isProfileComplete, isVerified, isStudentVerified
```

### DiscoverProfile
```
id, name, age, bio, location, photos[], interests[], languages[],
compatibilityScore (0-100), professionTitle, isVerified,
voiceIntroUrl, hasVoiceIntro, hasReels
```

### ChatMessage
```
id, matchId, senderId, receiverId, content, createdAt, isRead
status: sending → sent → delivered → read
```

### Match
```
id, partner { id, name, age, location, bio, photos[] }, isMutual, status, matchedAt
```

### Reel / Spot
```
id, userId, userName, userAge, userPhoto, videoUrl, title, description,
likeCount, viewCount, isVerified, location, tags[], compatibilityScore
```

### Call
```
callId, callType (audio | video), token, signalingUrl
status: idle → connecting → ringing → active → idle
```

### Preferences
```
minAge, maxAge, maxDistanceKm, preferredGenders[], onlyVerified, onlyStudents
```

## Project Structure

```
├── rust-backend/              # Main Rust backend (Axum)
│   ├── src/
│   │   ├── handlers/          # REST + GraphQL endpoint handlers
│   │   ├── services/          # Business logic layer
│   │   ├── middleware/        # Auth, CORS, logging
│   │   ├── graphql.rs         # GraphQL schema & resolvers
│   │   ├── websocket.rs       # WebSocket chat + call signaling
│   │   └── vision/            # Selfie liveness verification
│   ├── migrations/            # PostgreSQL migrations
│   └── k8s/                   # Kubernetes manifests
├── microservices/             # Event-driven microservices
│   ├── gateway/               # API Gateway
│   ├── services/              # Auth, User, Match, Chat, Payment, etc.
│   ├── shared/                # Common lib (auth, config, events, models)
│   ├── k8s/                   # K8s manifests (base + dev/prod overlays)
│   └── docker-compose.yml
├── ambassador-dashboard/      # React/TypeScript analytics dashboard
├── tests/                     # E2E, Load, Contract, Smoke, Fuzz, Chaos
├── vision/                    # Selfie liveness detection
├── location/                  # Geo services, student discount verification
├── protos/                    # gRPC protocol buffers
└── docker-compose.yml         # Dev environment
```

## Quick Start

### Rust Backend
```bash
cd rust-backend
cp .env.example .env       # configure DATABASE_URL, REDIS_URL, etc.
cargo build --release
cargo run                  # serves on http://127.0.0.1:8080
```

### Microservices (Docker Compose)
```bash
cd microservices
docker compose up -d       # all services + Kafka + Postgres + Redis
```

### Ambassador Dashboard
```bash
cd ambassador-dashboard
npm install && npm run dev
```

## Testing

```bash
tests/e2e/run_tests.sh          # End-to-end user flows
tests/load/k6 run load_tests.js # k6 load tests
tests/contract/run_tests.sh     # API contract validation
tests/smoke/run_tests.sh        # Health checks
tests/fuzz/cargo +nightly fuzz  # Fuzz testing
tests/chaos/chaos_tests.sh      # Resilience testing
```

## Deployment

```bash
# Development
docker compose up -d

# Production (Kubernetes)
kubectl apply -k microservices/k8s/overlays/prod/
```

## API Base URLs

| Environment | HTTP | WebSocket |
|------------|------|-----------|
| Development | `http://127.0.0.1:8080` | `ws://127.0.0.1:8080` |
| Production | `https://api.nava.app` | `wss://api.nava.app` |

## Performance Targets

| Metric | Target |
|--------|--------|
| Concurrent connections | 10K+ per node |
| P95 response time | < 500ms |
| P99 response time | < 1000ms |
| WebSocket latency | < 50ms |
| AI match scoring | Compatibility 0-100 |
