# Nava - High-Performance Dating Platform

Distributed systems backend for real-time social connections at scale. Built with Rust/Axum microservices, event-driven Kafka architecture, and ML-powered matching.

## Architecture

```
                          ┌──────────────┐
                          │  Mobile App  │
                          │(React Native)│
                          └──────┬───────┘
                                 │
                    ┌────────────▼────────────┐
                    │   API Gateway (Rust)    │
                    │  JWT · Rate Limiting    │
                    └────────────┬────────────┘
          ┌──────────┬──────────┼──────────┬──────────┐
          ▼          ▼          ▼          ▼          ▼
     ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐ ┌────────┐
     │  Auth  │ │  User  │ │ Match  │ │  Chat  │ │Payment │
     │Service │ │Service │ │Service │ │Service │ │Service │
     └───┬────┘ └───┬────┘ └───┬────┘ └───┬────┘ └───┬────┘
         └──────────┴──────────┴──────────┴──────────┘
                               │
                    ┌──────────▼──────────┐
                    │    Apache Kafka     │
                    │   Event Streaming   │
                    └──────────┬──────────┘
              ┌────────────────┼────────────────┐
              ▼                ▼                ▼
        ┌───────────┐  ┌───────────┐   ┌───────────┐
        │Notification│  │ Analytics │   │    ML     │
        │  Service   │  │  Service  │   │  Service  │
        └───────────┘  └───────────┘   └───────────┘

  ┌──────────┐ ┌───────┐ ┌───────┐ ┌─────┐ ┌──────────┐
  │PostgreSQL│ │ Redis │ │ Neo4j │ │ S3  │ │ClickHouse│
  │(Users,   │ │(Cache,│ │(Graph │ │(Media│ │(Analytics│
  │ Payments)│ │ OTP)  │ │ Rel.) │ │     │ │  OLAP)   │
  └──────────┘ └───────┘ └───────┘ └─────┘ └──────────┘
```

## Tech Stack

| Layer | Technologies |
|-------|-------------|
| **Backend** | Rust, Axum, Tokio, SQLx, async-graphql |
| **Messaging** | Apache Kafka, WebSocket pub/sub |
| **Databases** | PostgreSQL 15, Redis 7, Neo4j 5, ClickHouse |
| **ML/CV** | PyTorch, ONNX Runtime, OpenCV, Federated Learning |
| **Payments** | Razorpay, Stripe |
| **Infrastructure** | Docker, Kubernetes, Kustomize, Prometheus, Grafana |
| **Cloud** | AWS S3, CloudFront CDN |
| **Mobile** | React Native / Expo |
| **Dashboard** | React, TypeScript, Vite, Tailwind CSS |

## Microservices

| Service | Port | Responsibility |
|---------|------|----------------|
| API Gateway | 8000 | Request routing, JWT validation, rate limiting |
| Auth Service | 8001 | OTP verification, JWT tokens, session management |
| User Service | 8002 | Profile CRUD, photos, preferences |
| Match Service | 8003 | Discovery algorithm, swipes, compatibility scoring |
| Chat Service | 8004 | Real-time messaging, WebSocket, read receipts |
| Payment Service | 8005 | Razorpay/Stripe, subscriptions, webhooks |
| Ambassador Service | 8006 | Referral program, partner tracking |
| Notification Service | — | FCM/APNs push notifications |
| Analytics Service | 8008 | Event aggregation, ClickHouse OLAP, metrics |

## Key Features

- **Real-time messaging** via WebSocket pub/sub with typing indicators and read receipts
- **ML-powered matching** with multi-dimensional compatibility scoring and geo-filtering
- **Face verification** and liveness detection using ONNX Runtime
- **Voice intros** (30-sec recordings) and video reels discovery
- **Event-driven architecture** with Kafka topics for all domain events
- **Analytics pipeline** — Kafka consumers writing to ClickHouse with materialized views (DAU, hourly event counts)
- **GraphQL API** with DataLoader for N+1 elimination and field-level auth
- **Student discount verification** with location-based pass management
- **Content moderation** — NSFW detection and quality scoring
- **Ambassador dashboard** — React app for referral tracking and performance metrics

## Kafka Event Topics

```
user.events        →  registration, verification, profile updates
payment.events     →  orders, completions, subscriptions, refunds
match.events       →  swipes, matches, unmatches
chat.events        →  messages sent, read receipts
analytics.events   →  generic tracking events
notification.cmds  →  push, email, SMS commands
dlq.events         →  dead letter queue
```

## Project Structure

```
├── rust-backend/           # Main Rust backend (Axum)
│   ├── src/
│   │   ├── handlers/       # REST endpoint handlers
│   │   ├── services/       # Business logic
│   │   ├── middleware/      # Auth, CORS, logging
│   │   ├── graphql.rs      # GraphQL schema
│   │   ├── websocket.rs    # WebSocket handling
│   │   └── vision/         # CV integration
│   ├── migrations/         # SQL migrations
│   └── k8s/                # K8s manifests
├── microservices/          # Event-driven microservices
│   ├── gateway/            # API Gateway
│   ├── services/           # Auth, User, Match, Chat, Payment, etc.
│   ├── shared/             # Common lib (auth, config, events, models)
│   ├── k8s/                # Kubernetes manifests (base + overlays)
│   └── docker-compose.yml
├── ambassador-dashboard/   # React/TypeScript dashboard
├── tests/                  # E2E, Load, Contract, Smoke, Fuzz, Chaos
├── vision/                 # Face recognition, NSFW, liveness models
├── location/               # Geo services, student discounts
├── protos/                 # gRPC protocol buffers
├── main.py                 # Legacy FastAPI monolith
├── docker-compose.yml      # Dev environment
└── Dockerfile
```

## Quick Start

### Rust Backend

```bash
cd rust-backend
cp .env.example .env       # configure DATABASE_URL, REDIS_URL, etc.
cargo build --release
cargo run
```

### Microservices (Docker Compose)

```bash
cd microservices
docker compose up -d       # starts all services + Kafka + Postgres + Redis
```

### Legacy Python API

```bash
python3 -m venv .venv
source .venv/bin/activate
pip install -r requirements.txt
uvicorn main:app --reload
```

### Ambassador Dashboard

```bash
cd ambassador-dashboard
npm install
npm run dev
```

## Testing

```bash
# E2E tests
cd tests/e2e && ./run_tests.sh

# Load tests (k6)
cd tests/load && k6 run load_tests.js

# Contract tests
cd tests/contract && ./run_tests.sh

# Smoke tests
cd tests/smoke && ./run_tests.sh

# Fuzz tests (nightly Rust)
cd tests/fuzz && cargo +nightly fuzz run

# Chaos tests
cd tests/chaos && ./chaos_tests.sh
```

## Deployment

**Development:** Docker Compose with hot-reload

**Production:** Kubernetes with Kustomize overlays

```bash
# Dev
kubectl apply -k microservices/k8s/overlays/dev/

# Prod
kubectl apply -k microservices/k8s/overlays/prod/
```

## Performance Targets

| Metric | Target |
|--------|--------|
| Concurrent connections | 10K+ per node |
| P95 response time | < 500ms |
| P99 response time | < 1000ms |
| Error rate | < 10% |
| WebSocket latency | < 50ms |

## Notes

- Do **not** commit virtualenvs, DB files, or uploaded media
- Large models/datasets should live outside Git (use S3 or Git LFS)
- All state changes are published as Kafka events (eventual consistency)
- Services communicate synchronously only for queries, never for commands
