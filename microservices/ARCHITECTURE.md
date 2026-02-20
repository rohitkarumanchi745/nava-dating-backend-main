# Nava Platform - Microservices Architecture

## Overview

Event-driven microservices architecture using Apache Kafka for asynchronous communication.

```
                                    ┌─────────────────┐
                                    │   Mobile App    │
                                    │  (React Native) │
                                    └────────┬────────┘
                                             │
                                             ▼
┌────────────────────────────────────────────────────────────────────────────┐
│                            API Gateway (Rust/Axum)                          │
│  • JWT Validation  • Rate Limiting  • Request Routing  • Load Balancing    │
└────────────────────────────────────────────────────────────────────────────┘
         │              │              │              │              │
         ▼              ▼              ▼              ▼              ▼
    ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐   ┌─────────┐
    │  Auth   │   │  User   │   │ Payment │   │ Matching│   │  Chat   │
    │ Service │   │ Service │   │ Service │   │ Service │   │ Service │
    └────┬────┘   └────┬────┘   └────┬────┘   └────┬────┘   └────┬────┘
         │              │              │              │              │
         └──────────────┴──────────────┴──────────────┴──────────────┘
                                       │
                                       ▼
                        ┌──────────────────────────────┐
                        │      Apache Kafka Cluster     │
                        │  ┌─────────────────────────┐  │
                        │  │        Topics           │  │
                        │  │  • user.events          │  │
                        │  │  • payment.events       │  │
                        │  │  • match.events         │  │
                        │  │  • notification.events  │  │
                        │  │  • analytics.events     │  │
                        │  └─────────────────────────┘  │
                        └──────────────────────────────┘
                                       │
         ┌─────────────────────────────┼─────────────────────────────┐
         ▼                             ▼                             ▼
    ┌─────────┐                  ┌─────────┐                   ┌─────────┐
    │Notifica-│                  │Analytics│                   │   ML    │
    │  tion   │                  │ Service │                   │ Service │
    │ Service │                  │         │                   │         │
    └─────────┘                  └─────────┘                   └─────────┘


┌─────────────────────────────────────────────────────────────────────────────┐
│                              Data Stores                                     │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐       │
│  │PostgreSQL│  │  Redis   │  │  Neo4j   │  │   S3     │  │Clickhouse│       │
│  │ (Users,  │  │ (Cache,  │  │ (Graph   │  │ (Media)  │  │(Analytics│       │
│  │ Payments)│  │ Sessions)│  │Relations)│  │          │  │   OLAP)  │       │
│  └──────────┘  └──────────┘  └──────────┘  └──────────┘  └──────────┘       │
└─────────────────────────────────────────────────────────────────────────────┘
```

## Services

### 1. API Gateway
- **Port:** 8080
- **Responsibilities:**
  - JWT validation and user context injection
  - Rate limiting (Redis-backed)
  - Request routing to microservices
  - Response aggregation
  - WebSocket connection management

### 2. Auth Service
- **Port:** 8081
- **Database:** PostgreSQL (auth schema)
- **Topics Produced:** `user.registered`, `user.verified`, `user.login`
- **Responsibilities:**
  - OTP generation and verification
  - JWT token management
  - Session management
  - Phone number verification

### 3. User Service
- **Port:** 8082
- **Database:** PostgreSQL (users schema)
- **Topics Produced:** `user.profile.updated`, `user.premium.activated`
- **Topics Consumed:** `user.registered`, `payment.subscription.activated`
- **Responsibilities:**
  - Profile management
  - Preferences
  - Photo/media management
  - Premium status

### 4. Payment Service
- **Port:** 8083
- **Database:** PostgreSQL (payments schema)
- **Topics Produced:** `payment.order.created`, `payment.completed`, `payment.failed`, `payment.subscription.activated`
- **Topics Consumed:** `user.registered`
- **Responsibilities:**
  - Razorpay integration
  - Stripe integration
  - Order management
  - Subscription lifecycle
  - Webhook processing

### 5. Matching Service
- **Port:** 8084
- **Database:** PostgreSQL + Neo4j
- **Topics Produced:** `match.created`, `match.liked`, `match.passed`
- **Topics Consumed:** `user.profile.updated`, `user.premium.activated`
- **Responsibilities:**
  - Discovery algorithm
  - Swipe processing
  - Match creation
  - Compatibility scoring

### 6. Chat Service
- **Port:** 8085
- **Database:** PostgreSQL (messages schema) + Redis
- **Topics Produced:** `chat.message.sent`, `chat.message.read`
- **Topics Consumed:** `match.created`
- **Responsibilities:**
  - Real-time messaging (WebSocket)
  - Message persistence
  - Read receipts
  - Typing indicators

### 7. Notification Service
- **Port:** 8086
- **Database:** PostgreSQL (notifications schema)
- **Topics Consumed:** `match.created`, `chat.message.sent`, `payment.completed`, `user.verified`
- **Responsibilities:**
  - Push notifications (FCM/APNs)
  - Email notifications
  - In-app notifications
  - Notification preferences

### 8. Analytics Service
- **Port:** 8087
- **Database:** ClickHouse
- **Topics Consumed:** ALL events
- **Responsibilities:**
  - Event aggregation
  - Real-time dashboards
  - ML training data pipeline
  - Business metrics

## Kafka Topics

### Topic Naming Convention
```
{domain}.{entity}.{action}
```

### Topics Schema

#### user.events
```json
{
  "event_id": "uuid",
  "event_type": "user.registered | user.verified | user.profile.updated | user.deleted",
  "user_id": "int",
  "timestamp": "ISO8601",
  "data": {
    // Event-specific payload
  },
  "metadata": {
    "source_service": "string",
    "correlation_id": "uuid",
    "version": "1"
  }
}
```

#### payment.events
```json
{
  "event_id": "uuid",
  "event_type": "order.created | payment.completed | payment.failed | subscription.activated | subscription.cancelled | refund.processed",
  "user_id": "int",
  "timestamp": "ISO8601",
  "data": {
    "order_id": "string",
    "amount_cents": "int",
    "currency": "string",
    "gateway": "razorpay | stripe",
    "product_id": "string"
  },
  "metadata": {
    "source_service": "payment-service",
    "correlation_id": "uuid",
    "idempotency_key": "string"
  }
}
```

#### match.events
```json
{
  "event_id": "uuid",
  "event_type": "swipe.like | swipe.pass | match.created | match.unmatched",
  "user_id": "int",
  "timestamp": "ISO8601",
  "data": {
    "target_user_id": "int",
    "match_id": "string",
    "score": "float"
  },
  "metadata": {
    "source_service": "matching-service",
    "correlation_id": "uuid"
  }
}
```

#### notification.commands
```json
{
  "command_id": "uuid",
  "command_type": "send.push | send.email | send.sms",
  "user_id": "int",
  "timestamp": "ISO8601",
  "data": {
    "template": "string",
    "variables": {},
    "channels": ["push", "email"]
  }
}
```

## Service Communication

### Synchronous (HTTP/gRPC)
- API Gateway → Services (request/response)
- Service → Service (only for queries, not commands)

### Asynchronous (Kafka)
- All state changes published as events
- Services subscribe to relevant topics
- Eventual consistency model

## Data Ownership

| Service | Owns | Caches |
|---------|------|--------|
| Auth | sessions, otps, tokens | - |
| User | users, profiles, photos | - |
| Payment | orders, transactions, subscriptions | user_id |
| Matching | swipes, matches, scores | user profiles |
| Chat | messages, conversations | user presence |
| Notification | notification_logs, preferences | user tokens |

## Deployment

### Kubernetes Resources per Service
- Deployment (2+ replicas)
- Service (ClusterIP)
- HorizontalPodAutoscaler
- PodDisruptionBudget
- ConfigMap
- Secret

### Infrastructure
- Kafka: Confluent Cloud or self-hosted (3 brokers)
- PostgreSQL: RDS/Cloud SQL with read replicas
- Redis: ElastiCache/MemoryStore cluster
- Neo4j: Aura or self-hosted cluster

## Migration Strategy

### Phase 1: Infrastructure (Week 1)
- Set up Kafka cluster
- Create shared event library
- Set up service mesh (optional)

### Phase 2: Extract Services (Weeks 2-4)
1. Auth Service (lowest dependencies)
2. Payment Service (isolated domain)
3. Notification Service (consumer only)
4. User Service
5. Matching Service
6. Chat Service

### Phase 3: API Gateway (Week 5)
- Route traffic through gateway
- Deprecate monolith endpoints

### Phase 4: Cleanup (Week 6)
- Remove monolith
- Performance tuning
- Documentation
