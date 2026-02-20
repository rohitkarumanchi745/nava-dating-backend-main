# NAVA Platform - Microservices Architecture

A high-performance dating platform built with Rust microservices.

## Architecture Overview

```
                    ┌─────────────────┐
                    │   Mobile App    │
                    │  (React Native) │
                    └────────┬────────┘
                             │
                             ▼
                    ┌─────────────────┐
                    │   API Gateway   │
                    │   (Port 8000)   │
                    └────────┬────────┘
                             │
        ┌────────────────────┼────────────────────┐
        │                    │                    │
        ▼                    ▼                    ▼
┌───────────────┐   ┌───────────────┐   ┌───────────────┐
│ Auth Service  │   │ User Service  │   │ Match Service │
│  (Port 8001)  │   │  (Port 8002)  │   │  (Port 8003)  │
└───────────────┘   └───────────────┘   └───────────────┘
        │                    │                    │
        ▼                    ▼                    ▼
┌───────────────┐   ┌───────────────┐   ┌───────────────┐
│ Chat Service  │   │Payment Service│   │Ambassador Svc │
│  (Port 8004)  │   │  (Port 8005)  │   │  (Port 8006)  │
└───────────────┘   └───────────────┘   └───────────────┘
        │                    │                    │
        └────────────────────┼────────────────────┘
                             │
        ┌────────────────────┼────────────────────┐
        │                    │                    │
        ▼                    ▼                    ▼
┌───────────────┐   ┌───────────────┐   ┌───────────────┐
│   PostgreSQL  │   │     Redis     │   │     Neo4j     │
│   (Port 5432) │   │  (Port 6379)  │   │  (Port 7687)  │
└───────────────┘   └───────────────┘   └───────────────┘
```

## Services

| Service | Port | Description |
|---------|------|-------------|
| Gateway | 8000 | API Gateway - Routes requests, handles auth |
| Auth | 8001 | Authentication - OTP, JWT tokens |
| User | 8002 | User profiles, preferences |
| Match | 8003 | Discovery, likes, matching |
| Chat | 8004 | Real-time messaging via WebSocket |
| Payment | 8005 | Subscriptions, Razorpay integration |
| Ambassador | 8006 | Ambassador program, referrals |

## Tech Stack

- **Language**: Rust 1.75+
- **Web Framework**: Axum
- **Database**: PostgreSQL 15 (SQLx)
- **Cache**: Redis 7
- **Graph DB**: Neo4j 5 (for social graph)
- **Auth**: JWT with RS256
- **Container**: Docker & Docker Compose

## Quick Start

### Prerequisites

- Docker & Docker Compose
- Rust 1.75+ (for local development)

### Run with Docker

```bash
cd microservices

# Start all services
docker-compose up -d

# View logs
docker-compose logs -f

# Stop services
docker-compose down
```

### Local Development

```bash
cd microservices

# Start infrastructure only
docker-compose up -d postgres redis neo4j

# Run a specific service
cargo run --package auth-service

# Run all services (separate terminals)
cargo run --package gateway
cargo run --package auth-service
cargo run --package user-service
cargo run --package match-service
cargo run --package chat-service
cargo run --package payment-service
cargo run --package ambassador-service
```

## API Endpoints

### Gateway (8000)

All requests go through the gateway which handles authentication and routing.

```
POST /api/auth/send-otp      → Auth Service
POST /api/auth/verify-otp    → Auth Service
POST /api/auth/refresh       → Auth Service

GET  /api/users/me           → User Service
PUT  /api/users/profile      → User Service

GET  /api/discover           → Match Service
POST /api/like/:id           → Match Service
POST /api/pass/:id           → Match Service
GET  /api/matches            → Match Service

GET  /api/messages/:match_id → Chat Service
POST /api/messages/:match_id → Chat Service
WS   /api/ws/:match_id       → Chat Service

GET  /api/plans              → Payment Service
POST /api/subscribe          → Payment Service
GET  /api/subscription       → Payment Service

POST /api/ambassador/apply   → Ambassador Service
GET  /api/ambassador/status  → Ambassador Service
```

## Environment Variables

Create a `.env` file:

```env
# Database
DATABASE_URL=postgres://nava:nava_secret@localhost:5432/nava_db

# Redis
REDIS_URL=redis://localhost:6379

# Neo4j
NEO4J_URI=bolt://localhost:7687
NEO4J_USER=neo4j
NEO4J_PASSWORD=nava_secret

# JWT
JWT_SECRET=your-super-secret-jwt-key-change-in-production
JWT_EXPIRY=3600

# Twilio (for OTP)
TWILIO_ACCOUNT_SID=your_account_sid
TWILIO_AUTH_TOKEN=your_auth_token
TWILIO_PHONE_NUMBER=+1234567890

# Razorpay
RAZORPAY_KEY_ID=your_key_id
RAZORPAY_KEY_SECRET=your_key_secret

# Service Ports
GATEWAY_PORT=8000
AUTH_SERVICE_PORT=8001
USER_SERVICE_PORT=8002
MATCH_SERVICE_PORT=8003
CHAT_SERVICE_PORT=8004
PAYMENT_SERVICE_PORT=8005
AMBASSADOR_SERVICE_PORT=8006
```

## Project Structure

```
microservices/
├── Cargo.toml              # Workspace config
├── Cargo.lock
├── docker-compose.yml      # All services + infra
│
├── shared/                 # Shared library
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── auth.rs         # JWT utilities
│       ├── config.rs       # Service config
│       ├── error.rs        # Error handling
│       └── models.rs       # Shared models
│
├── gateway/                # API Gateway
│   ├── Cargo.toml
│   ├── Dockerfile
│   └── src/main.rs
│
└── services/
    ├── auth-service/
    │   ├── Cargo.toml
    │   ├── Dockerfile
    │   └── src/main.rs
    │
    ├── user-service/
    │   ├── Cargo.toml
    │   ├── Dockerfile
    │   └── src/main.rs
    │
    ├── match-service/
    │   ├── Cargo.toml
    │   ├── Dockerfile
    │   └── src/main.rs
    │
    ├── chat-service/
    │   ├── Cargo.toml
    │   ├── Dockerfile
    │   └── src/main.rs
    │
    ├── payment-service/
    │   ├── Cargo.toml
    │   ├── Dockerfile
    │   └── src/main.rs
    │
    └── ambassador-service/
        ├── Cargo.toml
        ├── Dockerfile
        └── src/main.rs
```

## Database Schema

The services share a PostgreSQL database with the following main tables:

- `users` - User accounts
- `user_profiles` - Profile information
- `swipes` - Like/pass actions
- `matches` - Mutual matches
- `messages` - Chat messages
- `subscriptions` - Payment subscriptions
- `ambassadors` - Ambassador program data

## Inter-Service Communication

Services communicate via HTTP REST:
- Gateway routes external requests to services
- Services can call each other directly using service URLs
- WebSocket connections for real-time chat

## Scaling

Each service can be scaled independently:

```bash
docker-compose up -d --scale match-service=3
```

For production, use Kubernetes with:
- Horizontal Pod Autoscaler
- Service mesh (Istio/Linkerd)
- Distributed tracing (Jaeger)

## Monitoring

Recommended stack:
- **Metrics**: Prometheus + Grafana
- **Logging**: ELK Stack or Loki
- **Tracing**: Jaeger or Zipkin

## License

MIT
