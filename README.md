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

 ┌──────────────────────────────────────────────────┐
 │              Data Layer                          │
 │  ┌──────────┐ ┌───────┐ ┌───────┐ ┌─────┐       │
 │  │PostgreSQL│ │ Redis │ │ Neo4j │ │ S3  │       │
 │  │ Primary  │ │(Cache,│ │(Graph │ │(Media│      │
 │  │(Writes)  │ │ OTP)  │ │ Rel.) │ │ CDN)│      │
 │  └────┬─────┘ └───────┘ └───────┘ └─────┘       │
 │  ┌────▼─────┐ ┌───────────┐ ┌──────────┐        │
 │  │ PgBouncer│ │  Read     │ │ClickHouse│        │
 │  │ (Conn    │ │  Replica  │ │(Analytics│        │
 │  │  Pool)   │ │  (Reads)  │ │  OLAP)   │        │
 │  └──────────┘ └───────────┘ └──────────┘        │
 └──────────────────────────────────────────────────┘
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
| **Databases** | PostgreSQL 16 (primary + read replicas), PgBouncer (connection pooling), Redis 7, Neo4j 5, ClickHouse |
| **Event Streaming** | Apache Kafka (user, payment, match, chat, analytics topics) |
| **ML Engine** | Q-Learning RL (14-dim state, epsilon-greedy), LinUCB Contextual Bandit (UCB scoring), FedAvg with Differential Privacy |
| **Computer Vision** | tract-onnx (ArcFace, FER+, NSFW, NIMA, Liveness), face verification, emotion detection |
| **LLM** | LLaMA 3 (content labeling, moderation), batch inference pipeline |
| **Recommendations** | RL-scored discovery ranking, pgvector 512-dim embeddings, collaborative filtering |
| **File Storage** | AWS S3 + CloudFront CDN (photos, voice intros, reels) |
| **Payments** | Apple StoreKit 2 (iOS), RevenueCat (React Native), Razorpay, Stripe |
| **Infrastructure** | Docker, Kubernetes (EKS), Kustomize, PgBouncer, Prometheus, Grafana, Alertmanager, PagerDuty, GitHub Actions CI/CD |
| **iOS** | SwiftUI, Combine, StoreKit 2 |
| **Cross-Platform** | React Native, Expo, TypeScript |
| **Dashboard** | React, TypeScript, Vite, Tailwind CSS |

## Libraries, Packages & Connectors

### Rust Crates (Backend)

| Category | Crate | Version | Purpose |
|----------|-------|---------|---------|
| **Web Framework** | `axum` | 0.8 | HTTP server with JSON, WebSocket, multipart support |
| | `axum-extra` | 0.10 | Typed headers |
| | `tower` | 0.5 | Middleware stack (timeout, rate limit, load-shed) |
| | `tower-http` | 0.6 | CORS, compression (gzip), tracing, request-id, timeout |
| **Async Runtime** | `tokio` | 1.x | Multi-threaded async runtime with signal handling, fs, parking_lot |
| | `futures` | 0.3 | Async combinators |
| | `tokio-stream` | 0.1 | Stream utilities for async |
| | `async-trait` | 0.1 | Async trait support |
| **Database** | `sqlx` | 0.7 | Async PostgreSQL driver with compile-time query checking, chrono, JSON, UUID, Decimal |
| | `redis` | 0.24 | Async Redis client with connection manager + tokio integration |
| | `neo4rs` | 0.8 | Async Neo4j Bolt protocol driver |
| **Serialization** | `serde` | 1.x | Serialization/deserialization framework |
| | `serde_json` | 1.x | JSON parsing and generation |
| | `rust_decimal` | 1.33 | Decimal arithmetic for financial calculations |
| **Authentication** | `jsonwebtoken` | 9 | JWT token encode/decode (HS256/RS256) |
| | `bcrypt` | 0.15 | Password hashing (bcrypt) |
| **GraphQL** | `async-graphql` | 7.0 | GraphQL server with chrono, UUID, DataLoader |
| | `async-graphql-axum` | 7.0 | Axum integration for GraphQL |
| **Time & UUID** | `chrono` | 0.4 | Date/time handling with serde |
| | `uuid` | 1.x | UUID v4 generation with serde |
| **Computer Vision** | `tract-onnx` | 0.21 | ONNX model inference (ArcFace, FER+, NSFW, NIMA, Liveness) |
| | `image` | 0.25 | Image decoding, resizing, pixel manipulation |
| | `base64` | 0.22 | Base64 encoding/decoding for image data |
| **ML** | `rand` | 0.8 | Random number generation (epsilon-greedy, Laplace noise) |
| **HTTP Client** | `reqwest` | 0.12 | HTTP client with rustls-tls (S3, external APIs) |
| **Crypto** | `sha2` | 0.10 | SHA-256 hashing (AWS Signature V4) |
| | `hmac` | 0.12 | HMAC authentication (CloudFront signed URLs) |
| | `hex` | 0.4 | Hex encoding for crypto operations |
| **Email** | `lettre` | 0.11 | SMTP email client with TLS (OTP, notifications) |
| **Observability** | `tracing` | 0.1 | Structured logging and distributed tracing |
| | `tracing-subscriber` | 0.3 | Log output with env-filter and JSON format |
| | `metrics` | 0.22 | Application metrics (counters, gauges) |
| | `metrics-exporter-prometheus` | 0.13 | Prometheus metrics endpoint |
| **OpenTelemetry** | `tracing-opentelemetry` | 0.22 | OpenTelemetry tracing bridge (optional) |
| | `opentelemetry` | 0.21 | OpenTelemetry API (optional) |
| | `opentelemetry_sdk` | 0.21 | OpenTelemetry SDK with tokio runtime (optional) |
| | `opentelemetry-otlp` | 0.14 | OTLP exporter with tonic + metrics (optional) |
| **Error Handling** | `thiserror` | 1.0 | Derive macros for error types |
| **Validation** | `validator` | 0.16 | Input validation with derive macros |
| **Environment** | `dotenvy` | 0.15 | `.env` file loading |

### Rust Crates (Microservices)

| Category | Crate | Version | Purpose |
|----------|-------|---------|---------|
| **Event Streaming** | `rdkafka` | 0.36 | Apache Kafka client (producer + consumer) with SSL |
| **gRPC** | `tonic` | 0.10 | gRPC framework for inter-service communication |
| | `prost` | 0.12 | Protocol Buffers code generation |
| **Error Handling** | `anyhow` | 1.x | Error context and chaining |

### Python Packages (ML & API)

| Category | Package | Version | Purpose |
|----------|---------|---------|---------|
| **Web Framework** | `fastapi` | 0.116 | Async REST API framework |
| | `uvicorn` | 0.35 | ASGI server |
| | `uvloop` | 0.21 | High-performance event loop |
| | `starlette` | 0.47 | ASGI toolkit (FastAPI dependency) |
| **Database** | `sqlalchemy` | 2.0 | SQL ORM + connection pooling |
| | `sqlmodel` | 0.0.24 | Pydantic + SQLAlchemy models |
| | `asyncpg` | 0.30 | Async PostgreSQL driver |
| | `psycopg2-binary` | 2.9 | Sync PostgreSQL driver |
| | `redis` | 6.4 | Redis client |
| **Deep Learning** | `torch` | 2.3 | PyTorch deep learning framework |
| | `torchvision` | 0.18 | Vision models and transforms |
| | `torchaudio` | 2.3 | Audio processing |
| **Computer Vision** | `opencv-python` | 4.9 | Image/video processing |
| | `face-recognition` | 1.3 | Face detection and recognition (dlib-based) |
| | `dlib-bin` | 19.24 | Face landmark detection |
| | `pillow` | 11.3 | Image manipulation |
| **ML/Data Science** | `scikit-learn` | 1.3 | Machine learning algorithms |
| | `numpy` | 1.26 | Numerical computing |
| | `pandas` | 2.0 | Data manipulation and analysis |
| | `scipy` | 1.10 | Scientific computing |
| | `datafusion` | 41.0 | SQL query engine for analytics |
| **GraphQL** | `strawberry-graphql` | 0.157 | GraphQL server with FastAPI integration |
| **gRPC** | `grpcio` | 1.76 | gRPC client/server |
| | `grpcio-tools` | 1.76 | Protocol Buffers compiler |
| **Payments** | `stripe` | 12.5 | Stripe payment processing SDK |
| **Event Streaming** | `kafka-python` | 2.0 | Apache Kafka producer/consumer |
| **Auth & Crypto** | `python-jose` | 3.5 | JWT token handling |
| | `bcrypt` | 4.3 | Password hashing |
| | `cryptography` | 45.0 | TLS, X.509, encryption |
| | `passlib` | 1.7 | Password hashing utilities |
| **Geolocation** | `reverse_geocoder` | 1.5 | Coordinate to city/country lookup |
| **Validation** | `pydantic` | 2.11 | Data validation and serialization |
| **WebSocket** | `websockets` | 15.0 | WebSocket client/server |
| **HTTP** | `requests` | 2.32 | HTTP client for external APIs |
| **Environment** | `python-dotenv` | 1.1 | `.env` file loading |

### NPM Packages (Ambassador Dashboard)

| Category | Package | Version | Purpose |
|----------|---------|---------|---------|
| **UI Framework** | `react` | 18.2 | UI component library |
| | `react-dom` | 18.2 | React DOM renderer |
| | `react-router-dom` | 6.20 | Client-side routing |
| **Charts** | `recharts` | 2.10 | Data visualization (performance charts, leaderboards) |
| **HTTP** | `axios` | 1.6 | HTTP client for API calls |
| **State Management** | `zustand` | 4.4 | Lightweight state management |
| **Styling** | `tailwindcss` | 3.3 | Utility-first CSS framework |
| | `tailwind-merge` | 2.1 | Merge Tailwind classes |
| | `clsx` | 2.0 | Conditional CSS class joining |
| **Date** | `date-fns` | 3.0 | Date formatting and manipulation |
| **Build** | `vite` | 5.0 | Build tool and dev server |
| | `typescript` | 5.3 | TypeScript compiler |

### External Services & Connectors

| Service | Purpose | Integration |
|---------|---------|-------------|
| **PostgreSQL 16** | Primary database (users, matches, payments, ML features) | `sqlx` (Rust), `asyncpg` (Python) |
| **Redis 7** | Cache, rate limiting, OTP storage, session management, instance heartbeat | `redis` crate (Rust), `redis` package (Python) |
| **Neo4j 5** | Social graph relationships, friend-of-friend discovery | `neo4rs` (Bolt protocol) |
| **Apache Kafka** | Event streaming (user, payment, match, chat, analytics topics) | `rdkafka` (Rust), `kafka-python` (Python) |
| **ClickHouse** | OLAP analytics database (DAU, event aggregation) | HTTP API |
| **AWS S3** | Photo, voice intro, reel storage | `reqwest` with AWS Signature V4 |
| **AWS CloudFront** | CDN for media delivery | Signed URLs via `hmac` + `sha2` |
| **AWS EKS** | Kubernetes cluster for production | `kubectl`, Kustomize |
| **AWS ECR** | Docker image registry | GitHub Actions push |
| **AWS RDS** | Managed PostgreSQL (production) | ExternalName K8s service |
| **AWS ElastiCache** | Managed Redis (production) | ExternalName K8s service |
| **AWS ACM** | TLS certificates for ALB | ALB Ingress annotation |
| **AWS SES** | Transactional email | SMTP via `lettre` |
| **Apple StoreKit 2** | iOS in-app purchases and subscriptions | Server-side receipt validation |
| **RevenueCat** | Cross-platform subscription management | Webhook sync |
| **Razorpay** | India payments (UPI, cards, wallets, netbanking) | REST API + webhooks |
| **Stripe** | Global card payments and subscriptions | `stripe` Python SDK + webhooks |
| **Twilio** | SMS OTP delivery | REST API |
| **Firebase (FCM)** | Android push notifications | HTTP API |
| **Apple (APNs)** | iOS push notifications | HTTP/2 API |
| **SendGrid / SMTP** | Email delivery (OTP, notifications) | SMTP transport |
| **Prometheus** | Metrics collection and alerting | `/metrics` endpoint |
| **Grafana** | Metrics visualization and dashboards | Prometheus data source |
| **InfluxDB** | Time-series metrics (load testing) | k6 integration |
| **Confluent Schema Registry** | Kafka message schema validation | HTTP API |

### Build & DevOps Tools

| Tool | Purpose |
|------|---------|
| **Docker** | Container builds (multi-stage Rust, Python) |
| **Kubernetes** | Container orchestration (EKS) |
| **Kustomize** | K8s manifest management (base + overlays) |
| **GitHub Actions** | CI/CD (test, build, push, deploy) |
| **k6** | Load testing |
| **cargo** | Rust build system and package manager |
| **protoc / prost** | Protocol Buffers code generation |

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
│   ├── deploy/                # Ops runbook, SLO alerts, PgBouncer + PostgreSQL configs
│   ├── monitoring/            # Prometheus scrape config, Alertmanager routing
│   ├── migrations/            # PostgreSQL migrations (incl. hash-partitioned swipes)
│   └── Dockerfile             # Multi-stage build, non-root, healthcheck
├── microservices/             # Event-driven microservices
│   ├── gateway/               # API Gateway
│   ├── services/              # Auth, User, Match, Chat, Payment, etc.
│   ├── shared/                # Common lib (auth, config, events, models)
│   ├── k8s/                   # K8s manifests (base + dev/prod overlays)
│   └── docker-compose.yml
├── ambassador-dashboard/      # React/TypeScript analytics dashboard
├── tests/                     # E2E, Load (k6 PgBouncer+replica), Contract, Smoke, Fuzz, Chaos
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
1. **Test** — `cargo fmt`, `cargo clippy`, `cargo test --lib`
2. **DB Integration Tests** — PostgreSQL 16 service container, runs migrations, swipes partition regression test, statement_timeout verification (direct + PgBouncer modes)
3. **Build** — Multi-stage Docker build, push to ECR
4. **Deploy** — Rolling update to EKS with auto-rollback on failure

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
# Unit & integration tests (63 tests)
cd rust-backend && cargo test --lib

# Database integration tests (CI-automated)
psql -f rust-backend/tests/swipes_partition_test.sql    # Partition regression
psql -f rust-backend/tests/statement_timeout_test.sql   # Timeout verification

# Load testing (PgBouncer + replica validation)
k6 run tests/load/k6-pgbouncer-replica.js               # ML fallback, replica lag, pool saturation

# Other test suites
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

| Metric | Target | SLO Alert |
|--------|--------|-----------|
| Availability | 99.9% (43 min/month) | Error rate > 0.1% for 5m |
| API latency (p99) | < 500ms | > 500ms for 5m |
| Discover latency (p99) | < 200ms | > 200ms for 5m |
| ML scoring latency | < 10ms avg (in-memory) | > 10ms for 5m |
| ML fallback rate | < 5% | > 5% warning, > 20% critical |
| WebSocket connections | 10K+ per node | > 8,000 for 5m |
| WebSocket latency | < 50ms | — |
| DB pool utilization | < 80% | > 80% for 3m |
| Write TPS capacity | ~5,000-8,000 | > 500 TPS sustained 10m |
| Replica lag | < 2s | > 2s auto-fallback to primary |

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

### Prometheus Metrics (`/metrics`)

| Metric | Type | Description |
|--------|------|-------------|
| `app_http_requests_total` | Counter | Total HTTP requests by method/path/status |
| `http_request_duration_seconds` | Histogram | Request latency by endpoint |
| `app_db_pool_size` / `app_db_pool_idle` | Gauge | Database connection pool utilization |
| `app_websocket_connections` | Gauge | Active WebSocket connections |
| `app_ml_fallback_total` | Counter | Discover requests that fell back to attractiveness scoring |
| `app_discover_requests_total` | Counter | Total discover requests (for fallback rate calculation) |
| `app_ml_avg_scoring_latency_us` | Gauge | Average ML scoring latency in microseconds |
| `app_vision_unavailable_total` | Counter | Vision endpoint requests when sidecar unavailable |
| `app_swipe_writes_total` | Counter | Total swipe writes (like + pass) for TPS monitoring |
| `app_replica_lag_ms` | Gauge | Read replica replication lag in milliseconds |
| `app_replica_healthy` | Gauge | Read replica health (1=healthy, 0=degraded) |
| `app_reads_from_replica` | Counter | Reads served by replica |
| `app_reads_fallback_to_primary` | Counter | Reads that fell back to primary |
| `dlq_entries_pending` | Gauge | Pending DLQ entries by queue |

### SLO Definitions

| SLO | Target | Alert Threshold |
|-----|--------|----------------|
| **Availability** | 99.9% (43 min/month) | Error rate > 0.1% for 5m |
| **API Latency** | p99 < 500ms | > 500ms for 5m |
| **Discover Latency** | p99 < 200ms | > 200ms for 5m |
| **DB Pool** | < 80% utilization | > 80% for 3m |
| **Payment DLQ** | < 50 pending | > 50 for 10m |
| **ML Scoring** | avg < 10ms | > 10ms for 5m |
| **ML Fallback Rate** | < 5% | > 5% for 5m (warning), > 20% (critical) |
| **WebSocket Capacity** | < 8,000 connections | > 8,000 for 5m |

### Alert Routing

| Severity | Channels |
|----------|----------|
| **Warning** | Slack (`#nava-platform-alerts`, `#nava-payments-alerts`) |
| **Critical** | Slack urgent channels + PagerDuty on-call |
| **Security** | `#nava-security-alerts` + email to security team |

Alert rules defined in `rust-backend/deploy/slo-alerts.yml`. Alertmanager config in `rust-backend/monitoring/alertmanager.yaml`.

### Health Probes

| Endpoint | Purpose | Details |
|----------|---------|---------|
| `/health` | Basic liveness | Returns 200 if app is running |
| `/health/detailed` | Component health | DB pool, Redis, replica status, ML engine |
| `/ready` | K8s readiness | Checks DB connectivity |
| `/metrics` | Prometheus scrape | All counters, gauges, histograms |

## Database Architecture

### Connection Pooling (PgBouncer)
- **Mode:** Transaction pooling (connection returned after each transaction)
- **Pool:** 50 server connections, 1,000 max client connections
- **Config:** `rust-backend/deploy/pgbouncer.ini`
- **Note:** Session-level `SET` statements are lost between transactions — use `SET LOCAL` within transactions or configure in `postgresql.conf`

### Read Replicas
- **Primary pool** (`state.db`) — all writes + fallback reads
- **Replica pool** (`state.db_read`) — read-heavy queries (discover, matches, profiles, admin stats, embeddings, reels)
- **Health check:** Background task every 5s via `pg_last_xact_replay_timestamp()`
- **Auto-fallback:** If replica lag > 2s, all reads automatically route to primary
- **~15 read-heavy handlers** migrated to replica: profile, discover, matches, spots, admin stats, embeddings, bandit arms, training events, reels, learned patterns, payment reads

### Swipes Partitioning
- **Strategy:** Hash-partitioned by `from_user_id` into 8 partitions
- **Benefit:** Write distribution across partitions, preserves UNIQUE constraint for ON CONFLICT upserts
- **CI enforced:** Partition regression test runs on every push/PR

### Write Scaling Roadmap
- **Current capacity:** ~5,000-8,000 write TPS on 4vCPU/16GB primary (adequate for ~50K DAU)
- **Sharding triggers:** Pool utilization >60% sustained, write p99 >100ms, WAL >500MB/min, DAU >100K
- **Phase 1:** Functional sharding (swipes DB, messages DB, events DB)
- **Phase 2:** Horizontal sharding by `user_id % N`
- Full details in `rust-backend/deploy/ops-runbook.md`

## Graceful Degradation

| Component | When Unavailable | User Impact |
|-----------|-----------------|-------------|
| **Vision sidecar** | 503 on `/vision/analyze`, `/verify/selfie` | Photo analysis skipped, verification unavailable |
| **ML ranking** | 2s timeout → attractiveness score fallback | Lower-quality discover rankings |
| **ML `record_swipe`** | Fire-and-forget (`tokio::spawn`) | Zero impact on swipe latency |
| **Read replica** | Auto-fallback to primary (lag >2s) | No user impact, higher primary load |
| **Neo4j (graph)** | Dual-write manager queues, PG-only fallback | No user impact |
| **Redis (cache)** | App runs without cache | Slightly slower responses |

### Webhook Resilience
- Razorpay and Stripe webhooks catch processing failures and auto-enqueue to DLQ
- Always return 200 to payment gateway (prevents infinite retries)
- DLQ entries can be retried or manually reviewed via `/api/payments/dlq/*` endpoints

## Operations

Full ops runbook at `rust-backend/deploy/ops-runbook.md` covering:
- K8s secret rotation procedure (`SECRET_KEY_FILE` pattern)
- SLO definitions and burn rate windows
- Alert response playbooks for every alert
- ML fallback investigation and remediation
- PgBouncer admin commands and scaling
- Read replica monitoring and scaling
- Write scaling roadmap and sharding strategy
