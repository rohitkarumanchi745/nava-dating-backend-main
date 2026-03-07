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
          ┌────────────────────┼────────────────────┐
          ▼                    ▼                     ▼
   ┌────────────┐     ┌──────────────┐      ┌────────────┐
   │  ML Engine │     │ Apache Kafka │      │  Vision    │
   │ RL · LinUCB│     │   Events     │      │  Pipeline  │
   │  FedAvg    │     │              │      │ Face·NSFW  │
   └────────────┘     └──────┬───────┘      └────────────┘
                   ┌─────────┼─────────┐
                   ▼         ▼         ▼
            ┌───────────┐ ┌──────┐ ┌──────┐
            │Notification│ │Analyt│ │  ML  │
            │  Service   │ │ ics  │ │Train │
            └───────────┘ └──────┘ └──────┘

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

### Discovery & Matching (GraphQL + ML)

| Operation | Type | Description |
|-----------|------|-------------|
| `discover(filters: { useAi, limit })` | Query | Get swipeable profiles ranked by RL agent with `compatibilityScore` |
| `likeUser(targetUserId)` | Mutation | Like a profile → feeds RL agent, returns `{ success, isMutual, matchId }` |
| `passUser(targetUserId)` | Mutation | Skip a profile → feeds RL agent with negative signal |
| `matches` | Query | Get all matches with partner details (mutual + received likes) |

### ML Computation Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/ml/rl/rank` | POST | Rank candidate user IDs using Q-learning RL agent |
| `/ml/linucb/score` | POST | Score a candidate arm using LinUCB contextual bandit |
| `/ml/stats` | GET | RL agent stats (epsilon, replay buffer), LinUCB stats, FL stats |
| `/ml/embeddings` | POST/GET | Store/retrieve user embedding vectors |
| `/ml/bandit` | POST/GET | Store/retrieve LinUCB arm states (A-matrix, b-vector) |
| `/ml/reward` | POST | Log reward signal for ML training |
| `/ml/events` | GET | Get training events for offline analysis |
| `/ml/scores/bulk` | POST | Bulk update attractiveness scores |
| `/fl/register` | POST | Register FL client for federated learning |
| `/fl/round/start` | POST | Start new FL round |
| `/fl/update` | POST | Submit client model update |
| `/fl/aggregate` | POST | FedAvg aggregation with differential privacy |
| `/fl/model` | GET | Get active global model weights for deployment |

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
| **Databases** | PostgreSQL 16, Redis 7, Neo4j 5, ClickHouse |
| **Event Streaming** | Apache Kafka (user, payment, match, chat, analytics topics) |
| **ML Engine** | Q-Learning RL (14-dim state, epsilon-greedy), LinUCB Contextual Bandit (UCB scoring), FedAvg with Differential Privacy |
| **Computer Vision** | tract-onnx (ArcFace, FER+, NSFW, NIMA, Liveness), face verification, emotion detection |
| **LLM** | LLaMA 3 (content labeling, moderation), batch inference pipeline |
| **Recommendations** | RL-scored discovery ranking, pgvector 512-dim embeddings, collaborative filtering |
| **File Storage** | AWS S3 + CloudFront CDN (photos, voice intros, reels) |
| **Payments** | Apple StoreKit 2 (iOS), RevenueCat (React Native), Razorpay, Stripe |
| **Infrastructure** | Docker, Kubernetes (EKS), Kustomize, Prometheus, Grafana, GitHub Actions CI/CD |
| **iOS** | SwiftUI, Combine, StoreKit 2 |
| **Cross-Platform** | React Native, Expo, TypeScript |
| **Dashboard** | React, TypeScript, Vite, Tailwind CSS |

## ML & AI Architecture

### In-Memory ML Engine (Rust)

All ML computation runs in-process for sub-millisecond scoring latency:

| Component | Algorithm | Details |
|-----------|-----------|---------|
| **RL Agent** | Q-Learning | 14-dim state (7 user + 7 candidate features), epsilon-greedy (0.3→0.01, decay 0.995), per-user model blending (70% global + 30% personal), 10K experience replay buffer |
| **LinUCB Bandit** | Contextual Bandit | UCB scoring with Gauss-Jordan matrix inverse, per-arm A-matrix + b-vector, alpha=0.6, observation decay 0.995, JSONB persistence to PostgreSQL |
| **FedAvg** | Federated Learning | Weighted averaging by sample count, Laplace noise differential privacy (scale 0.1), min 2 clients per round, global learning rate 0.1 |

**Discovery Flow:**
```
SQL candidates → RL scoring → Re-rank by Q-value → Return to client
     ↓                                                    ↓
  Filters (age, distance,           Like/Pass feeds back into
   verified, not-yet-swiped)         RL agent training loop
```

**Feature Vector (7 dimensions per user):**
1. Age (normalized 18-60)
2. Attractiveness score
3. Profile completeness
4. Verification score (selfie + student)
5. Activity score (7-day interactions)
6. Photo count
7. Height (normalized)

### On-Device Federated Learning
- **Privacy-preserving model aggregation** across clients (min 10 clients, 10% fraction per round)
- **Differential Privacy** — Noise multiplier (1.0) + gradient clipping (norm 1.0) for user data protection
- **Config:** `FL_ENABLED`, `FL_MIN_CLIENTS`, `FL_CLIENT_FRACTION`, `FL_LOCAL_EPOCHS`, `FL_LEARNING_RATE`, `FL_DP_ENABLED`

### LLM Integration (LLaMA 3)
- **Content Labeling** — Automated profile/bio moderation and tagging
- **Batch Inference** — Configurable batch size (10) with retry logic (max 3)
- **Config:** `LLM_ENABLED`, `LLM_API_URL`, `LLM_MODEL_NAME=llama3`, `LLM_BATCH_SIZE`

### Computer Vision Pipeline
- **Face Recognition** — ArcFace embedding extraction + cosine similarity matching
- **Selfie Liveness Detection** — LBP entropy + FFT frequency + HSV color analysis (weights 0.4/0.4/0.2)
- **Emotion Detection** — FER+ 8-emotion classification
- **NSFW Detection** — Content quality scoring and moderation
- **Image Quality** — NIMA aesthetic scoring

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

## Project Structure

```
├── rust-backend/              # Main Rust backend (Axum)
│   ├── src/
│   │   ├── handlers/          # REST + GraphQL endpoint handlers (150+)
│   │   ├── ml/                # ML computation engine
│   │   │   ├── rl_agent.rs    # Q-learning RL for discovery ranking
│   │   │   ├── linucb.rs      # LinUCB contextual bandit
│   │   │   ├── federated.rs   # FedAvg aggregation + differential privacy
│   │   │   ├── features.rs    # 7-dim user feature extraction
│   │   │   └── math.rs        # softmax, laplace noise, cosine similarity
│   │   ├── services/          # Business logic layer
│   │   ├── middleware/        # Auth, CORS, rate limiting, dual-write
│   │   ├── graphql.rs         # GraphQL schema & resolvers
│   │   ├── websocket.rs       # WebSocket chat + call signaling
│   │   └── vision/            # Face recognition, liveness, emotion, NSFW
│   ├── k8s/                   # Kubernetes manifests
│   │   ├── base/              # Ingress, NetworkPolicy, PDB, ServiceAccount
│   │   ├── overlays/dev/      # Dev: in-cluster Postgres + Redis
│   │   └── overlays/prod/     # Prod: RDS + ElastiCache, higher resources
│   ├── monitoring/            # Prometheus rules + alerts
│   ├── migrations/            # PostgreSQL migrations
│   └── Dockerfile             # Multi-stage build, non-root, healthcheck
├── microservices/             # Event-driven microservices
│   ├── gateway/               # API Gateway
│   ├── services/              # Auth, User, Match, Chat, Payment, etc.
│   ├── shared/                # Common lib (auth, config, events, models)
│   ├── k8s/                   # K8s manifests (base + dev/prod overlays)
│   └── docker-compose.yml
├── ambassador-dashboard/      # React/TypeScript analytics dashboard
├── tests/                     # E2E, Load, Contract, Smoke, Fuzz, Chaos
├── vision/                    # Face recognition, liveness, NSFW detection (PyTorch/ONNX)
├── location/                  # Geo services, student discount verification
├── protos/                    # gRPC protocol buffers
├── .github/workflows/         # CI/CD (lint, test, build, deploy to EKS)
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

## Deployment (AWS EKS)

### Development (in-cluster databases)
```bash
kubectl create namespace nava-dev
kubectl apply -k rust-backend/k8s/overlays/dev/
```

### Production (RDS + ElastiCache)
```bash
kubectl create namespace nava-prod

# Configure secrets (use AWS Secrets Manager in production)
kubectl create secret generic nava-secrets \
  --from-env-file=.env.production -n nava-prod

# Deploy
kubectl apply -k rust-backend/k8s/overlays/prod/

# Verify
kubectl get pods -n nava-prod
kubectl get hpa -n nava-prod
```

### CI/CD Pipeline
Automated via GitHub Actions (`.github/workflows/rust-ci.yml`):
1. **Test** — `cargo fmt`, `cargo clippy`, `cargo test`
2. **Build** — Multi-stage Docker build, push to ECR
3. **Deploy** — Rolling update to EKS with auto-rollback on failure

### Kubernetes Features
- **HPA** — Auto-scales 3→20 pods based on CPU (65%) and memory (75%)
- **PDB** — Minimum 2 pods always available during upgrades
- **Rolling updates** — Zero-downtime with `maxUnavailable: 0`
- **Topology spread** — Pods distributed across AZs
- **Network policies** — Restricted pod-to-pod traffic
- **IRSA** — IAM Roles for Service Accounts (no embedded credentials)
- **ALB Ingress** — TLS via ACM, WebSocket sticky sessions

## Testing

```bash
tests/e2e/run_tests.sh          # End-to-end user flows
tests/load/k6 run load_tests.js # k6 load tests
tests/contract/run_tests.sh     # API contract validation
tests/smoke/run_tests.sh        # Health checks
tests/fuzz/cargo +nightly fuzz  # Fuzz testing
tests/chaos/chaos_tests.sh      # Resilience testing
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
| ML scoring latency | < 1ms (in-memory) |
| Match accuracy | 99.9% (RL-powered scoring) |

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
compatibilityScore (RL-scored), professionTitle, isVerified,
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

## Revenue Model

### 1. Subscriptions (Passes)

| Pass | Duration | Radius | Features |
|------|----------|--------|----------|
| **Free** | — | — | Basic discovery, limited likes |
| **Hourly** | 1 hr | 2 mi | Exact distance, enhanced discovery |
| **Daily** | 24 hr | 5 mi | Exact distance, enhanced discovery |
| **Weekly** | 7 days | 10 mi | Exact distance, enhanced discovery |
| **Monthly** | 30 days | 25 mi | Exact distance, enhanced discovery |
| **Ultra** | Unlimited | Unlimited | City names, exact distance, all features |

**Premium Tiers (iOS StoreKit / RevenueCat):**

| Tier | Key Features |
|------|-------------|
| **Gold** | Unlimited likes, see who likes you, 5 super likes/day, 1 boost/month, advanced filters |
| **Platinum** | All Gold + priority matching, read receipts, weekly boost, undo swipe |
| **Ultra** | All Platinum + priority support, exclusive events, unlimited super likes |

**Consumables:** Boosts (1x, 5x), Super Likes (5x), Spotlight (1hr)

### 2. Ad Monetization

Multi-network ad serving with location and language targeting:

| Ad Type | Revenue | Placement |
|---------|---------|-----------|
| **Banner** | Low | Non-intrusive, persistent |
| **Interstitial** | Medium | Between screens |
| **Native** | Medium | In-feed discovery |
| **Rewarded** | High | User-initiated for rewards |

**Ad Networks:** Google AdMob, Meta Audience Network, Unity Ads

**Targeting:**
- Location-based (country, state, city — India-first with regional targeting)
- Language-based (Telugu, Hindi, Tamil, Kannada, + 9 more languages)
- Platform-aware (iOS/Android)

**Rewarded Ad Rewards:**

| Reward | Amount | Trigger |
|--------|--------|---------|
| Boost | 1 free boost | Watch ad on boost screen |
| Super Like | 1 super like | Watch ad on super like screen |
| Premium Hours | 2 hours | Watch ad |
| Extra Likes | 5 likes | Watch ad |
| Profile View | 1 reveal | Watch ad |

### 3. Ambassador Program

Referral-driven growth engine with tiered ambassador compensation:

| Tier | Monthly Target | Monthly Stipend | Bonus/Signup |
|------|---------------|-----------------|--------------|
| **Campus** | 50 signups | ₹10,000 | ₹50 |
| **Regional** | 200 signups | ₹25,000 | ₹40 |
| **City** | 500 signups | ₹50,000 | ₹30 |

- Referral code tracking with signup attribution
- Real-time performance dashboard (React)
- Daily/weekly/monthly breakdown analytics
- Leaderboard and earnings tracking

### 4. Student Discounts

University-tiered pricing to drive campus adoption:

| Tier | Discount | Eligibility |
|------|----------|-------------|
| **Ivy / Top Private** | 30% | Top private universities |
| **Top 50 / Top Public** | 20% | Top 50 public universities |
| **State University** | 15% | State universities |
| **Graduate Student** | 15% | Graduate programs |
| **Other** | 10% | Any .edu email |
| **Alumni** | 5% | Verified alumni |

### 5. Payment Gateways

| Gateway | Market | Integration |
|---------|--------|-------------|
| **Apple StoreKit 2** | iOS (US/Global) | Server-side receipt validation |
| **RevenueCat** | Cross-platform | Subscription management + webhook sync |
| **Razorpay** | India | UPI, cards, wallets, netbanking |
| **Stripe** | Global | Cards, subscriptions, webhooks |

## Monitoring & Alerting

- **Prometheus metrics** at `/metrics` — request counts, errors, DB pool, WebSocket connections, uptime
- **AlertManager rules** — High error rate (>5%), high latency (P95 >2s), DB pool exhaustion (>85%), Redis failures
- **Health probes** — `/health` (basic), `/ready` (K8s readiness), `/live` (K8s liveness)
