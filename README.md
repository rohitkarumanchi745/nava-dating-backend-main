# Nava - High-Performance Dating Platform Backend

Production backend powering the Nava dating apps (SwiftUI iOS + React Native cross-platform). Built as a Rust/Axum modular monolith with in-process event bus, DataFusion SQL analytics, real-time WebSocket chat & calling, and ML-powered matching.

## Architecture

```
            ┌───────────────┐     ┌────────────────────┐
            │   iOS App     │     │  React Native App  │
            │   (SwiftUI)   │     │   (Expo / RN)      │
            └───────┬───────┘     └─────────┬──────────┘
                    └──────────┬────────────┘
                               │
         ┌─────────────────────▼──────────────────────┐
         │        Rust Modular Monolith (Axum)        │
         │   JWT · Rate Limiting · GraphQL · REST · WS │
         │                                             │
         │  ┌────────┐ ┌────────┐ ┌────────┐ ┌──────┐ │
         │  │  Auth  │ │  User  │ │  Match │ │ Chat │ │
         │  │Handler │ │Handler │ │Handler │ │  WS  │ │
         │  └────────┘ └────────┘ └────────┘ └──────┘ │
         │  ┌────────┐ ┌──────────────┐ ┌───────────┐ │
         │  │Payment │ │ Notification │ │ Analytics │ │
         │  │Handler │ │   Module     │ │  Module   │ │
         │  └────────┘ └──────────────┘ └───────────┘ │
         │                                             │
         │  ┌──────────────────────────────────────┐   │
         │  │      In-Process Event Bus            │   │
         │  │   (tokio::broadcast, typed events)   │   │
         │  └──────────────────────────────────────┘   │
         │                                             │
         │  ┌────────────┐  ┌────────────┐             │
         │  │  ML Engine │  │  Vision    │             │
         │  │ RL · LinUCB│  │  Pipeline  │             │
         │  │  FedAvg    │  │ Face·NSFW  │             │
         │  └────────────┘  └────────────┘             │
         │                                             │
         │  ┌────────────────────────────────────┐     │
         │  │  DataFusion SQL Analytics Engine   │     │
         │  │  Arrow RecordBatch · In-Process SQL│     │
         │  └────────────────────────────────────┘     │
         └─────────────────────────────────────────────┘
                               │
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
| `discover(filters: { useAi, limit })` | Query | Get swipeable profiles ranked by RL agent with `compatibilityScore`, `superLikedYou` tag |
| `likeUser(targetUserId)` | Mutation | Like a profile → feeds RL agent, returns `{ success, isMutual, matchId }` |
| `passUser(targetUserId)` | Mutation | Skip a profile → feeds RL agent with negative signal |
| `matches` | Query | Get all matches with partner details (mutual + received likes) |
| `searchUniversities(query, limit)` | Query | Autocomplete university search with student counts |
| `universityProfiles(universityId, gender, limit)` | Query | Browse profiles from a university with gender filter |
| `unifiedSearch(query, gender, limit)` | Query | Combined search across universities and user profiles |

### ML Computation Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/ml/rl/rank` | POST | Rank candidate user IDs using Q-learning RL agent |
| `/ml/linucb/score` | POST | Score a candidate arm using LinUCB contextual bandit |
| `/ml/stats` | GET | RL agent stats (epsilon, replay buffer, scoring latency), LinUCB stats, FL stats, shadow agreement rate |
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
| `/fl/training-data` | GET | Get labeled swipe pairs (28-dim state + reward labels) for on-device FL training |
| `/fl/local-data` | POST | Report local dataset stats (sample count, quality score) |

### LLM Content Labeling

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/llm/queue` | POST | Queue a reel/profile for LLM labeling |
| `/llm/batch` | GET | Get a batch of items for LLM labeling |
| `/llm/labels/reel` | POST/GET | Submit/retrieve reel labels (genre, mood, tags) |
| `/llm/labels/message` | POST | Submit message labels (toxicity, intent) |
| `/llm/labels/user` | POST | Submit user-level labels from profile analysis |
| `/llm/failed` | POST | Mark labeling job as failed |
| `/llm/export` | GET | Export training snapshot for offline analysis |

### Super Like

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/match/super-like` | POST | Super like a user — checks consumable balance (Platinum unlimited, Gold 5/day, purchased packs), records swipe, feeds RL agent with 3× reward signal |
| `/profiles/super-like` | POST | Alias for `/match/super-like` |

Super-likers appear higher in discover feed (+0.15 ML score boost) and are tagged with `superLikedYou: true`.

### University & Student Discovery

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/universities/search` | GET | Search universities by name (autocomplete) |
| `/universities/countries` | GET | List countries with universities |
| `/universities/discover` | GET | Discover profiles from same university |
| `/universities/{id}/profiles` | GET | Browse all profiles from a university with `?gender=male\|female&limit=50` filter |
| `/universities/reels` | GET | University-specific reel feed |
| `/universities/passes` | POST/GET | Purchase/get student discovery passes |

### Global Student Search

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/search/students` | GET | Search student profiles by name with gender filter |
| `/search/students/suggestions` | GET | Quick search suggestions (universities + profiles) |
| `/search/unified` | GET | Combined search across universities and user profiles |
| `/search/student/{id}` | GET | View detailed student profile |
| `/search/student/{id}/like` | POST | Like a student from search results |
| `/search/student/{id}/message` | POST | Send direct message from search |

### Reel Conversations & Match Flow

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/reels/conversation` | GET | Get conversation history for a reel |
| `/reels/patterns` | GET | Get learned content preferences from reel engagement |
| `/reels/inbox` | GET | Get messages received from reel viewers |
| `/reels/reply` | POST | Reply to a reel message |
| `/reels/message/read` | POST | Mark reel messages as read |
| `/reels/match-request` | POST | Request a match from reel conversation (after mutual engagement) |
| `/reels/match-accept` | POST | Accept or decline a reel match request → creates real match on accept |

### Cross-Surface Actions

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/profiles/{id}` | GET | Get any user's public profile |
| `/reels/{reel_id}/like-creator` | POST | Like a reel creator directly from their reel |

### Admin

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/admin/stats` | GET | Comprehensive admin statistics (users, matches, revenue) |
| `/admin/secrets/status` | GET | Check which secrets would change on rotation |

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
| **Event Bus** | In-process `tokio::broadcast` (typed DomainEvent enum, 23 event variants, 4096 capacity) |
| **Analytics Engine** | Apache DataFusion v44 (Arrow RecordBatch, in-process SQL over Postgres data) |
| **ML Engine** | Q-Learning RL (28-dim state, epsilon-greedy), LinUCB Contextual Bandit (UCB scoring), Shadow Scoring, FedAvg with Differential Privacy, On-Device Personalization Head, Cold-Start Biasing, Notification Click Predictor |
| **Computer Vision** | tract-onnx (ArcFace, FER+, NSFW CNN/ViT, NIMA, Liveness), blur/low-light detection, photo ranking, duplicate face detection |
| **Trust & Safety** | Graph anomaly detection (Neo4j), device fingerprinting, GBDT behavioral classifiers, ban evasion detection |
| **Content Moderation** | NLP toxicity/hate/harassment classifiers, URL/spam filters, messaging graph anomaly detection, moderation transparency + appeals |
| **LLM** | LLaMA 3 (content labeling, moderation), batch inference pipeline |
| **Recommendations** | RL-scored discovery ranking, pgvector 512-dim embeddings, collaborative filtering, content freshness decay |
| **Media Pipeline** | Responsive image variants (150/400/1080px), AV1/WEBP transcoding, adaptive bitrate reels, CDN smallest-rendition serving |
| **File Storage** | AWS S3 + CloudFront CDN (photos, voice intros, reels) |
| **Payments** | Apple StoreKit 2 (iOS), RevenueCat (React Native), Razorpay, Stripe |
| **Notification Intelligence** | Thompson Sampling bandit (variant selection), send-time optimization, per-user daily caps, quiet hours, opt-out respect, shadow-mode A/B canary |
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
| **Analytics** | `datafusion` | 44 | Apache DataFusion SQL engine (Arrow RecordBatch, in-process OLAP) |
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
| **Randomness** | `rand` | 0.8 | Thompson Sampling (Beta distribution), notification variant selection |

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
| **RL Agent** | Q-Learning | 28-dim state (14 user + 14 candidate features), epsilon-greedy (0.3→0.01, decay 0.995), per-user model blending (70% global + 30% personal), 10K experience replay buffer, warm-start from DB checkpoint |
| **LinUCB Bandit** | Contextual Bandit | UCB scoring with Gauss-Jordan matrix inverse, per-arm A-matrix + b-vector, alpha=0.6, observation decay 0.995, JSONB persistence to PostgreSQL, warm-start on boot |
| **Shadow Scoring** | RL vs LinUCB | Top-half agreement tracking between RL and LinUCB rankings for model comparison and observability |
| **FedAvg** | Federated Learning | Weighted averaging by sample count, Laplace noise differential privacy (scale 0.1), min 2 clients per round, global learning rate 0.1 |
| **Personalization Head** | On-Device FL | 33-param last-layer (32 weights + 1 bias), devices fine-tune via SGD on local swipe outcomes, FedAvg deltas aggregated server-side with DP |
| **Cold-Start Biasing** | Affinity FL | Per-intent-bucket weight adjustments (5-dim: interest, language, intent, CF, geo), EMA smoothing (0.7/0.3), ±0.3 clamping, DP noise |
| **Notif Click Predictor** | Engagement FL | 6-feature on-device logistic regression (hour, day, category, recency, daily count, match flag), federated updates, feeds bandit priors |
| **Federation Safety** | Privacy Boundary | Explicit allow/deny list for federated data, gradient clipping (\|val\| < 100), NaN/Inf validation, dimension checks per update type |
| **Geo Scorer** | Gravity Model | Haversine distance + local density smoothing (KDE bandwidth 50km), gravity beta=1.5, configurable max distance + units (km/miles) |
| **Affinity Scorer** | Multi-Signal Overlap | Interest Jaccard (35%), language overlap (15%), intent alignment (20%), collaborative filtering via CoLikeMatrix (30%) |
| **Engagement Scorer** | Churn + Timing | Logistic regression churn predictor (5 features), per-user send-time histograms (24 hourly buckets), Thompson Sampling notification bandit |
| **CoLike Matrix** | Collaborative Filtering | User-user co-like signals rebuilt periodically, feeds into affinity scorer CF weight |

**Discovery Ranking Blend (multi-signal):**
```
SQL candidates → Multi-signal scoring → Re-rank by blended score → Return to client
     ↓                ↓                                                 ↓
  Filters       ┌─── 55% RL score (Q-learning)                  Like/Pass feeds back
  (age, dist,   ├─── 20% Geo score (gravity model + density)    into RL + LinUCB training
   verified)    ├─── 20% Affinity score (interest/lang/intent/CF)(every 10 swipes → checkpoint)
                ├─── + churn boost (up to 0.15 for at-risk users)
                └─── + super like boost (+0.15 if candidate super-liked you)
                      LinUCB shadow scoring (observability, top-half agreement)
```

**2s timeout on ML ranking** — if scoring takes too long, falls back to `attractiveness_score` ordering. Fallback rate tracked via `app_ml_fallback_total` with SLO alert at >5%.

**Feature Vector (14 dimensions per user):**

| # | Feature | Category | Range | Source |
|---|---------|----------|-------|--------|
| 1 | `age_norm` | Profile | 0-1 | Normalized age (18-60) |
| 2 | `attractiveness` | Profile | 0-1 | Attractiveness score from users table |
| 3 | `profile_completeness` | Profile | 0-1 | Ratio of filled fields (9 total) |
| 4 | `verification_score` | Profile | 0-1 | Selfie (0.5) + student (0.5) verification |
| 5 | `photo_count` | Profile | 0-1 | Normalized photo count (max 4) |
| 6 | `height_norm` | Profile | 0-1 | Normalized height (140-200cm) |
| 7 | `has_profession` | Profile | 0/1 | Whether profession_category is set |
| 8 | `gender_enc` | Profile | 0-1 | male=0.0, female=1.0, non_binary=0.5 |
| 9 | `intent_enc` | Richness | 0-1 | relationship=1.0, casual=0.5, friendship=0.25 |
| 10 | `language_count` | Richness | 0-1 | Languages spoken (capped at 5) |
| 11 | `interest_count` | Richness | 0-1 | Interests listed (capped at 10) |
| 12 | `activity_score` | Engagement | 0-1 | 7-day interaction count (normalized) |
| 13 | `like_rate` | Swipe | 0-1 | Fraction of swipes that were likes |
| 14 | `match_rate` | Swipe | 0-1 | Fraction of likes that became mutual matches |

**RL state** = user features (14) + candidate features (14) = **28 dimensions**

**Feature Defaults:** Population-level means computed from DB on startup, with hardcoded neutral fallback if DB is unavailable. Used when per-user features can't be fetched.

**Model Persistence:**
- RL checkpoint saved to `fl_models` table (versioned, active flag) every 10 swipes
- LinUCB arms saved to `bandit_arm_stats` table (A-matrix, b-vector, pulls, reward)
- Both warm-started from DB on service boot

### On-Device Federated Learning
- **Privacy-preserving model aggregation** across clients (min 10 clients, 10% fraction per round)
- **Differential Privacy** — Noise multiplier (1.0) + gradient clipping (norm 1.0) for user data protection
- **FL Training Data** — `/fl/training-data` endpoint provides labeled swipe pairs (like=1.0, pass=0.0, mutual=1.5) with 28-dim combined state vectors and feature schema
- **Personalization Head** — 33-param last-layer fine-tuned on-device; server aggregates deltas via FedAvg with DP noise; head norm monitored for weight divergence (alert at L2 > 10.0)
- **Cold-Start Biasing** — Per-intent affinity weight adjustments aggregated from early swipe patterns; EMA smoothing prevents oscillation; clamped to ±0.3 to prevent wild swings
- **Notification Click Predictor** — 6-feature on-device logistic regression predicting push notification open probability; federated updates feed Thompson Sampling bandit priors
- **Federation Safety Boundary** — Explicit allow/deny for federated data: behavioral gradients (allowed), face embeddings/device-risk/location (forbidden); gradient clipping at |val| < 100, NaN/Inf rejection
- **Config:** `FL_ENABLED`, `FL_MIN_CLIENTS`, `FL_CLIENT_FRACTION`, `FL_LOCAL_EPOCHS`, `FL_LEARNING_RATE`, `FL_DP_ENABLED`

### LLM Integration (LLaMA 3)
- **Content Labeling** — Automated profile/bio moderation and tagging
- **Batch Inference** — Configurable batch size (10) with retry logic (max 3)
- **Config:** `LLM_ENABLED`, `LLM_API_URL`, `LLM_MODEL_NAME=llama3`, `LLM_BATCH_SIZE`

### Computer Vision Pipeline
- **Face Recognition** — ArcFace embedding extraction + cosine similarity matching
- **Selfie Liveness Detection** — LBP entropy + FFT frequency + HSV color analysis (weights 0.4/0.4/0.2)
- **Emotion Detection** — FER+ 8-emotion classification
- **NSFW Detection** — CNN/ViT classification for explicit content, nudity scoring, and moderation flagging
- **Image Quality** — NIMA aesthetic scoring + blur detection (Laplacian variance) + low-light detection (luminance histogram)
- **Photo Ranking** — Composite score from aesthetic quality, face ratio checks (face area / frame area), blur/noise levels; auto-ranks user photos for optimal profile ordering
- **Duplicate Face Detection** — Cross-user ArcFace embedding comparison to detect stolen/catfish photos

### Trust & Safety ML
- **Graph Anomaly Detection** — Neo4j-based models detecting suspicious swipe/match/message patterns (ring detection, velocity anomalies, fan-out clusters)
- **Device Fingerprinting** — Device model, OS version, screen resolution, timezone, language; hashed fingerprint stored for multi-account detection and ban evasion tracking
- **Behavioral Classifiers (GBDT)** — Gradient-boosted decision tree models on behavioral signals: swipe velocity, like-to-match ratio, message response time distribution, report frequency; produces per-user trust score (0-1)
- **Ban Evasion Detection** — Device fingerprint + IP + behavioral similarity matching against banned accounts; auto-flags accounts with >80% similarity

### Content Moderation Pipeline

| Layer | Technique | Coverage |
|-------|-----------|----------|
| **Visual** | CNN/ViT NSFW detection, face/liveness verification, duplicate face detection | Photos, selfies, reels |
| **Text (NLP)** | Toxicity classifiers (hate, harassment, threats), intent detection | Bios, chat messages, reel captions |
| **Spam** | URL/link detection, keyword blocklist, regex patterns, message frequency throttling | Chat, bios, reel captions |
| **Graph** | Anomaly detection on messaging/swipe graphs, coordinated behavior detection | Cross-user interaction patterns |

**Moderation Transparency:**
- Blocked/blurred photos and muted messages include user-facing reason codes (e.g., `nsfw_detected`, `spam_url`, `hate_speech`)
- Lightweight appeal path: users can submit appeals via `/moderation/appeal` with the decision trace ID
- All moderation decisions logged with trace IDs for support team review and audit
- Moderation dashboard shows false positive rates, appeal outcomes, and per-model accuracy

### Content Freshness & Anti-Gaming
- **Profile Decay Scoring** — Time-weighted freshness decay on profiles and media; stale profiles (no activity >30 days) receive reduced discover ranking via exponential decay multiplier
- **Media Freshness** — Photos/reels older than 90 days flagged for refresh prompt; fresh content receives temporary ranking boost
- **Profile Edit Rate Limiting** — Sliding window rate limits on profile field updates (max 10 edits/hour, 50/day) to prevent gaming/A-B testing of bios and photos
- **Score Recalculation** — Attractiveness and ML scores periodically recomputed to reflect current engagement patterns, not historical peaks

### Media Optimization
- **Responsive Image Variants** — Pre-generate multiple photo sizes on upload: thumbnail (150px), card (400px), full (1080px), original
- **Modern Formats** — AV1/WEBP transcoding for avatars and thumbnails; JPEG fallback for older clients
- **Smallest Rendition Serving** — CDN serves the smallest acceptable rendition based on `Accept` header, device pixel ratio, and requested viewport size
- **Reel Compression** — Adaptive bitrate encoding for video reels (720p/1080p); HLS-ready segments for streaming

### Client Adaptive Behavior
- **Battery-Aware Throttling** — Client reports battery level + charging state; server defers non-critical work (reel transcoding notifications, background sync) when battery <20%
- **Temperature Throttling** — Thermal state reported by client; server reduces media quality and defers heavy uploads when device is thermally throttled
- **Network-Class Adaptation** — Client reports connection type (WiFi/5G/4G/3G/2G); server adjusts response payloads:
  - **2G/3G:** Compressed JSON, thumbnail-only photos, no auto-play reels
  - **4G/WiFi:** Full payloads, card-size photos, reel previews
  - **5G/WiFi:** Full resolution, HD reels, prefetch next page
- **Deferred Uploads** — Reel and high-res photo uploads queued client-side when on low battery (<15%) or 2G; auto-resume on WiFi/charging

### Notification Intelligence
- **Thompson Sampling Bandit** — Beta-distributed arms for 4 notification categories (NewMatch, ReEngage, Like, Message), each with 3 copy variants; shadow mode logs bandit choice but sends control for safe A/B
- **Send-Time Optimization** — Per-user engagement hour histograms (24 buckets), defers notifications outside peak activity windows
- **Policy Gate Chain** — 6-stage check: opt-out → daily cap (12/day) → cooldown (5 min) → quiet hours (22:00-07:00 local) → send-time activity → variant selection
- **Region-Aware Timezone** — 3-tier resolution: device offset → country_code mapping (50+ ISO codes) → UTC fallback
- **Variant Logging** — All sends logged to `notification_outcomes` table with variant_id, category, sent_at, opened_at for offline evaluation
- **Shadow-Mode Canary** — Bandit selects variant but always sends control; evaluate uplift with safety gates before promoting to live

### Background Jobs
- **Neo4j Sync** — Dual-write consistency: health check (30s), incremental sync (60s), full sync (1h), queue processor for failed operations
- **DLQ Processor** — Auto-retry dead letter queue entries with exponential backoff; stats endpoint at `/api/payments/dlq/stats`
- **Send-Time Refresh** — Notification send-time histograms rebuilt every 6 hours from engagement data

## Modular Monolith Architecture

Single Rust binary with clean domain boundaries, replacing the previous microservices + Kafka setup.

### Domain Modules

| Module | Location | Responsibility |
|--------|----------|----------------|
| Auth | `handlers/` | Phone OTP, JWT tokens, session management |
| User | `handlers/` | Profile CRUD, photos, voice intros, preferences |
| Match | `handlers/` | Discovery algorithm, swipes, AI compatibility scoring |
| Chat | `handlers/`, `websocket.rs` | WebSocket messaging + call signaling, typing indicators, read receipts |
| Payment | `handlers/payments.rs`, `services/payments/` | Apple StoreKit 2, Razorpay, Stripe, subscriptions, webhooks, DLQ |
| Ambassador | `handlers/` | Referral program, partner tracking |
| Notification | `modules/notifications/` | FCM/APNs push, email (SMTP), SMS (Twilio), in-app; Thompson Sampling variant selection, send-time optimization, per-user daily caps, quiet hours |
| Analytics | `modules/analytics/` | DataFusion SQL engine (7 pre-built queries + custom SQL), ClickHouse OLAP sink |
| Events | `modules/events/` | In-process event bus (`tokio::broadcast`, typed `DomainEvent` enum) |

### Event Bus (replaces Kafka)

```
DomainEvent::UserRegistered       →  welcome notification, analytics
DomainEvent::UserVerified         →  engagement notification
DomainEvent::MatchCreated         →  match notification to both users
DomainEvent::SwipeLike            →  RL agent training (1× reward)
DomainEvent::SwipeSuperLike       →  super like notification + RL agent training (3× reward)
DomainEvent::SwipePass            →  RL agent training (negative signal)
DomainEvent::MessageSent          →  push notification to recipient
DomainEvent::PaymentCompleted     →  premium activation, analytics
DomainEvent::SubscriptionActivated →  confirmation notification
DomainEvent::ReferralSignup       →  ambassador commission tracking
DomainEvent::ReelMessage/Reply    →  reel conversation notifications
DomainEvent::ReelMatchRequested   →  match request notification
DomainEvent::ReelMatchAccepted    →  match accepted notification + real match creation
DomainEvent::AnalyticsEvent       →  ClickHouse sink
DomainEvent::SendPush/Email/Sms  →  notification delivery (FCM, APNs, SMTP, Twilio)
```

### DataFusion Analytics Endpoints

| Endpoint | Method | Description |
|----------|--------|-------------|
| `/analytics/refresh` | POST | Reload Postgres tables into Arrow RecordBatches |
| `/analytics/dau` | GET | Daily active users (configurable lookback) |
| `/analytics/funnel` | GET | Engagement funnel (registration → match → message) |
| `/analytics/match-rate` | GET | Match rate breakdown by gender |
| `/analytics/surfaces` | GET | Surface/discovery performance metrics |
| `/analytics/students` | GET | Student demographic breakdown |
| `/analytics/hourly` | GET | Hourly activity patterns |
| `/analytics/bandit` | GET | Bandit arm performance (ML A/B results) |
| `/analytics/query` | POST | Custom SQL (SELECT-only, write keywords blocked) |

## Project Structure

```
├── rust-backend/              # Modular Monolith (single Axum binary)
│   ├── src/
│   │   ├── handlers/          # REST + GraphQL endpoint handlers (170+)
│   │   ├── modules/           # Domain modules (modular monolith)
│   │   │   ├── events/        # In-process event bus (tokio::broadcast)
│   │   │   │   └── mod.rs     # EventBus, DomainEvent (23 variants), EventEnvelope
│   │   │   ├── notifications/ # Notification module (replaces notification-service)
│   │   │   │   ├── mod.rs     # NotificationModule, event listener, handler dispatch
│   │   │   │   ├── policy.rs  # Thompson Sampling bandit, send-time optimization, daily caps, quiet hours
│   │   │   │   ├── providers.rs # Push (FCM/APNs), Email (SMTP), SMS (Twilio), In-App
│   │   │   │   └── push/      # FCM HTTP v1, APNs JWT, device registry
│   │   │   └── analytics/     # Analytics module (replaces analytics-service)
│   │   │       ├── mod.rs     # AnalyticsModule, ClickHouse event sink
│   │   │       ├── datafusion_engine.rs # DataFusion SQL engine, Arrow RecordBatch loading
│   │   │       ├── clickhouse.rs # ClickHouse HTTP client, materialized views
│   │   │       └── routes.rs  # Analytics HTTP endpoints (DAU, funnel, match-rate, custom SQL)
│   │   ├── ml/                # ML computation engine (in-process, sub-ms latency)
│   │   │   ├── mod.rs         # MlService: warm-start, shadow scoring, checkpoint persistence
│   │   │   ├── rl_agent.rs    # Q-learning RL (28-dim state, per-user model blending)
│   │   │   ├── linucb.rs      # LinUCB contextual bandit (Gauss-Jordan, per-arm A/b)
│   │   │   ├── federated.rs   # FedAvg aggregation + Laplace DP + PersonalizationHead + ColdStart + NotifPredictor + FederationSafety
│   │   │   ├── features.rs    # 14-dim feature extraction + population defaults + FL training data
│   │   │   ├── affinity.rs    # Interest/language/intent overlap + collaborative filtering
│   │   │   ├── engagement.rs  # Churn prediction + send-time optimization + notification bandit
│   │   │   ├── geo.rs         # Gravity model distance scoring + density smoothing
│   │   │   └── math.rs        # softmax, laplace noise, cosine similarity
│   │   ├── services/          # Business logic layer
│   │   │   ├── freshness.rs   # Profile decay scoring + media freshness
│   │   │   ├── media_optimizer.rs # Responsive variants, AV1/WEBP, smallest-rendition
│   │   │   ├── moderation.rs  # Text toxicity, spam, URL detection
│   │   │   ├── trust_safety.rs # Device fingerprinting, behavioral classifiers
│   │   │   ├── photo_pipeline.rs # Photo quality scoring, duplicate face detection
│   │   │   └── payments/      # Stripe/Razorpay retry logic, DLQ processor
│   │   ├── middleware/        # Auth, CORS, rate limiting, dual-write, client-adaptive
│   │   │   ├── security.rs    # Input sanitization, email/phone redaction
│   │   │   ├── client_adaptive.rs # Battery/thermal/network-class throttling
│   │   │   └── dual_write.rs  # PostgreSQL ↔ Neo4j consistency with circuit breaker
│   │   ├── graphql.rs         # GraphQL schema & resolvers
│   │   ├── websocket.rs       # WebSocket chat + call signaling
│   │   └── vision/            # Face recognition, liveness, emotion, NSFW
│   ├── k8s/                   # Kubernetes manifests
│   │   ├── base/              # Ingress, NetworkPolicy, PDB, ServiceAccount
│   │   ├── overlays/dev/      # Dev: in-cluster Postgres + Redis
│   │   └── overlays/prod/     # Prod: RDS + ElastiCache, higher resources
│   ├── deploy/                # Ops runbook, SLO alerts, PgBouncer + PostgreSQL configs, canary scripts
│   ├── monitoring/            # Prometheus scrape config, Alertmanager routing
│   ├── migrations/            # PostgreSQL migrations (incl. hash-partitioned swipes)
│   └── Dockerfile             # Multi-stage build, non-root, healthcheck
├── microservices/             # Legacy microservices (superseded by modular monolith)
│   ├── gateway/               # API Gateway (now handled by rust-backend directly)
│   ├── services/              # Auth, User, Match, Chat, Payment, Notification, Analytics
│   ├── shared/                # Common lib (auth, config, events, models)
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

### Legacy Microservices (superseded)
```bash
# The microservices/ directory is preserved for reference but all functionality
# is now consolidated in the rust-backend modular monolith.
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
# Unit & integration tests (127 tests: 84 unit + 43 integration)
cd rust-backend && cargo test

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
| Notif throttle rate | < 40% | > 40% blocked/deferred for 15m |
| FL head weight norm | < 10.0 | > 10.0 for 15m (weight divergence) |

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
voiceIntroUrl, hasVoiceIntro, hasReels, superLikedYou
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

### Internal Database Tables

| Table | Purpose |
|-------|---------|
| `notification_outcomes` | Sent notifications with variant_id, category, sent_at, opened_at for offline eval |
| `notification_preferences` | Per-user opt-outs, daily caps, quiet hours |
| `in_app_notifications` | In-app notification feed |
| `fl_local_data` | Per-user FL local dataset stats (sample count, quality) |
| `llm_labeling_queue` | Queue of reels/messages for LLM labeling |
| `reel_llm_labels` | LLM-generated labels for reels (genre, mood, tags) |
| `message_llm_labels` | NLP labels for messages (toxicity, intent) |
| `user_llm_labels` | User-level labels from profile analysis |
| `llm_training_snapshots` | Exported snapshots for model training |
| `device_fingerprints` | Device model, OS, screen, timezone, language hashes |
| `trust_safety_events` | Flagged anomalies (ring detection, velocity) |
| `moderation_appeals` | User appeals of moderation decisions |
| `content_moderation` | Moderation decisions (blurred, muted, etc.) |
| `photo_quality_log` | Photo scoring and rejection reasons |
| `media_renditions` | Image variants (150px, 400px, 1080px) |
| `user_content_preferences` | Learned engagement scores with content genres |
| `universities` | University database (name, country, domain) |
| `student_discovery_swipes` | Swipes from university-specific discovery |
| `payment_retry_tracking` | Webhook retry state for idempotency |

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
| `app_photos_rejected_quality` | Counter | Photos rejected by aesthetic/blur/light scoring |
| `app_moderation_actions_total` | Counter | Content moderation decisions applied |
| `app_trust_safety_flags_total` | Counter | Trust & safety suspicious behavior flags |
| `app_upload_deferrals_total` | Counter | Uploads deferred due to client battery/thermal state |
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
| `app_fl_rounds_completed` | Counter | Federated learning aggregation rounds completed |
| `app_fl_head_weight_norm` | Gauge | L2 norm of personalization head weights (stability signal) |
| `app_fl_cold_start_buckets` | Gauge | Number of active cold-start intent buckets |
| `app_fl_notif_predictor_norm` | Gauge | L2 norm of notification click predictor weights |
| `app_fl_dp_enabled` | Gauge | Whether differential privacy is enabled for FL (1/0) |
| `notif_sent_total` | Counter | Total notifications sent (notification service) |
| `notif_blocked_cap` | Counter | Notifications blocked by daily cap |
| `notif_blocked_cooldown` | Counter | Notifications blocked by cooldown period |
| `notif_blocked_optout` | Counter | Notifications blocked by user opt-out |
| `notif_deferred_quiet` | Counter | Notifications deferred due to quiet hours |
| `notif_deferred_timing` | Counter | Notifications deferred by send-time optimizer |
| `notif_engagement_success` | Counter | Notification opens/clicks (success) |
| `notif_engagement_failure` | Counter | Notification ignores (failure) |
| `notif_variant_sends` | Gauge | Per-variant send count (bandit arms) |
| `notif_variant_expected_rate` | Gauge | Per-variant expected open rate (Thompson Sampling) |

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
| **Notif Throttle Rate** | < 40% blocked/deferred | > 40% for 15m |
| **Notif Engagement** | > 5% open rate | < 5% for 2h |
| **Notif Variant Skew** | No single variant > 80% | > 80% for 1h |
| **FL Aggregation** | Continuous rounds | Stalled for 6h (warning) |
| **FL Weight Stability** | Head norm < 10.0 | > 10.0 for 15m |
| **FL DP Enabled** | Always on in prod | Disabled for 5m (critical) |
| **Canary Staleness** | Evaluate within 48h | Shadow mode > 48h without eval |

### Alert Routing

| Severity | Channels |
|----------|----------|
| **Warning** | Slack (`#nava-platform-alerts`, `#nava-payments-alerts`) |
| **Critical** | Slack urgent channels + PagerDuty on-call |
| **Security** | `#nava-security-alerts` + email to security team |

**Alert Groups (15):** availability, latency, database, payments, ML scoring, vision, WebSocket, infrastructure, replica, PgBouncer, query, notification policy, content pipeline, FL stability, canary.

Alert rules defined in `rust-backend/deploy/slo-alerts.yml` (38 alerts). Alertmanager config in `rust-backend/monitoring/alertmanager.yaml`.

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
| **Notification bandit** | Shadow mode sends control variant | No variant optimization, baseline behavior |
| **FL aggregation** | Stalled rounds, head weights frozen | Personalization stops improving, cold-start biases stale |
| **Notif click predictor** | Falls back to uniform bandit priors | Bandit explores without informative priors |

### Webhook Resilience
- Razorpay and Stripe webhooks catch processing failures and auto-enqueue to DLQ
- Always return 200 to payment gateway (prevents infinite retries)
- DLQ entries can be retried or manually reviewed via `/api/payments/dlq/*` endpoints

## Operations

Full ops runbook at `rust-backend/deploy/ops-runbook.md` covering:
- K8s secret rotation procedure (`SECRET_KEY_FILE` pattern)
- SLO definitions and burn rate windows
- Alert response playbooks for every alert (38 alerts across 15 groups)
- ML fallback investigation and remediation
- PgBouncer admin commands and scaling
- Read replica monitoring and scaling
- Write scaling roadmap and sharding strategy
- Notification policy operations (bandit, caps, quiet hours, send-time)
- Shadow-mode canary rollout procedure (6-step with SQL validation)
- FL aggregation monitoring and weight drift investigation

### Shadow-Mode Canary Script

Operational script at `rust-backend/deploy/canary-shadow-notif.sh` for safe notification variant rollout:

```bash
./canary-shadow-notif.sh --enable     # Enable shadow mode (bandit selects but sends control)
./canary-shadow-notif.sh --monitor    # Check canary health: metrics, alerts, DB outcomes
./canary-shadow-notif.sh --evaluate   # Evaluate uplift with safety gates (sample size, skew, throttle)
./canary-shadow-notif.sh --promote    # Promote bandit to live (sends winning variant)
./canary-shadow-notif.sh --rollback   # Roll back to shadow mode
```

**Guardrails:** min 200 samples, throttle rate < 40%, variant skew < 80%, uplift threshold 2pp.
Dry-run test harness at `rust-backend/deploy/test-canary-guardrails.sh` (28 tests).
