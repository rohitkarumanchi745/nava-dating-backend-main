# NAVA Backend API Documentation

## Overview

NAVA is a dating platform backend built with Rust, featuring REST APIs, GraphQL, and WebSocket support for real-time features.

**Base URL**: `https://api.nava.dating` (production) | `http://localhost:8080` (development)

## Authentication

All protected endpoints require a JWT bearer token in the Authorization header:

```
Authorization: Bearer <token>
```

### Obtain Token

1. **Send OTP**: `POST /auth/otp/send`
2. **Verify OTP**: `POST /auth/otp/verify` → Returns `access_token`

---

## Endpoints

### Health & Monitoring

| Method | Endpoint | Auth | Description |
|--------|----------|------|-------------|
| GET | `/health` | No | Basic health check |
| GET | `/health/detailed` | No | Extended health with metrics |
| GET | `/ready` | No | Kubernetes readiness probe |
| GET | `/live` | No | Kubernetes liveness probe |
| GET | `/metrics` | No | Prometheus metrics |

#### GET /health
```json
{
  "status": "ok",
  "db": "ok",
  "vision": "enabled"
}
```

#### GET /health/detailed
```json
{
  "status": "healthy",
  "instance_id": "nava-prod-1",
  "uptime_secs": 3600,
  "db": {
    "status": "healthy",
    "pool_size": 100,
    "pool_idle": 85
  },
  "redis": {
    "status": "healthy",
    "connected": true
  },
  "neo4j": "connected",
  "vision": "enabled",
  "metrics": {
    "requests_total": 10000,
    "requests_active": 5,
    "errors_total": 10,
    "websocket_connections": 150
  }
}
```

---

### Authentication

| Method | Endpoint | Auth | Description |
|--------|----------|------|-------------|
| POST | `/auth/otp/send` | No | Send OTP to phone |
| POST | `/auth/otp/verify` | No | Verify OTP, get token |

#### POST /auth/otp/send
**Request:**
```json
{
  "phone_number": "+1234567890"
}
```

**Response:**
```json
{
  "message": "OTP sent successfully",
  "otp": "1234"  // Only in development
}
```

#### POST /auth/otp/verify
**Request:**
```json
{
  "phone_number": "+1234567890",
  "otp": "1234"
}
```

**Response:**
```json
{
  "access_token": "eyJ...",
  "token_type": "bearer",
  "user_id": 123,
  "is_new_user": false,
  "is_profile_complete": true
}
```

---

### Profile Management

| Method | Endpoint | Auth | Description |
|--------|----------|------|-------------|
| GET | `/api/profile/me` | Yes | Get current user profile |
| POST | `/api/profile/complete` | Yes | Complete profile (multipart) |
| PUT | `/api/profile/update` | Yes | Update profile |
| POST | `/api/profile/photos` | Yes | Upload photos |
| DELETE | `/api/profile/photos/{index}` | Yes | Delete photo |

#### POST /api/profile/complete
**Content-Type:** `multipart/form-data`

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| name | string | Yes | Display name (2-100 chars) |
| dob | string | Yes | Date of birth (YYYY-MM-DD) |
| gender | string | Yes | male, female, non_binary, other |
| profile_photo_1 | file | Yes | Primary photo (JPEG/PNG, max 10MB) |
| profile_photo_2 | file | Yes | Secondary photo |
| profile_photo_3 | file | Yes | Tertiary photo |

**Response:**
```json
{
  "success": true,
  "user_id": 123,
  "photo_insights": [
    {
      "quality": 0.85,
      "smile_detected": true,
      "authenticity": 0.92,
      "attractiveness": 7.5
    }
  ]
}
```

---

### Discovery & Matching

| Method | Endpoint | Auth | Description |
|--------|----------|------|-------------|
| GET | `/api/discover` | Yes | Get discovery feed |
| POST | `/api/like/{user_id}` | Yes | Like a user |
| POST | `/api/pass/{user_id}` | Yes | Pass on a user |
| GET | `/api/matches` | Yes | Get mutual matches |

#### GET /api/discover
**Query Parameters:**
| Param | Type | Default | Description |
|-------|------|---------|-------------|
| limit | int | 20 | Max profiles to return |
| offset | int | 0 | Pagination offset |

**Response:**
```json
{
  "profiles": [
    {
      "id": 456,
      "name": "Jane",
      "age": 25,
      "gender": "female",
      "bio": "Love hiking and coffee",
      "photos": ["url1", "url2", "url3"],
      "distance_km": 5.2,
      "compatibility_score": 0.85,
      "is_verified": true
    }
  ],
  "has_more": true
}
```

#### POST /api/like/{user_id}
**Response:**
```json
{
  "success": true,
  "is_mutual": true,
  "match_id": "match_abc123",
  "message": "It's a match!"
}
```

---

### Preferences

| Method | Endpoint | Auth | Description |
|--------|----------|------|-------------|
| GET | `/api/preferences` | Yes | Get user preferences |
| PUT | `/api/preferences` | Yes | Update preferences |

#### PUT /api/preferences
**Request:**
```json
{
  "min_age": 21,
  "max_age": 35,
  "preferred_genders": ["female"],
  "max_distance_km": 50,
  "only_verified": false,
  "only_students": false
}
```

---

### Location

| Method | Endpoint | Auth | Description |
|--------|----------|------|-------------|
| PUT | `/api/location` | Yes | Update user location |
| GET | `/api/location/search` | Yes | Search locations |

#### PUT /api/location
**Request:**
```json
{
  "latitude": 40.7128,
  "longitude": -74.0060,
  "location_text": "New York, NY"
}
```

---

### Messaging

| Method | Endpoint | Auth | Description |
|--------|----------|------|-------------|
| GET | `/api/messages/{match_id}` | Yes | Get messages for a match |
| POST | `/api/messages/{match_id}` | Yes | Send a message |
| PUT | `/api/messages/{message_id}/read` | Yes | Mark message as read |

#### POST /api/messages/{match_id}
**Request:**
```json
{
  "content": "Hey! How's it going?"
}
```

**Response:**
```json
{
  "id": 789,
  "sender_id": 123,
  "content": "Hey! How's it going?",
  "sent_at": "2024-01-15T10:30:00Z",
  "is_read": false
}
```

---

### Subscriptions

| Method | Endpoint | Auth | Description |
|--------|----------|------|-------------|
| GET | `/api/subscription/status` | Yes | Get subscription status |
| POST | `/api/subscription/sync` | Yes | Sync with RevenueCat |
| POST | `/webhook/revenuecat` | No | RevenueCat webhook |

#### GET /api/subscription/status
**Response:**
```json
{
  "is_premium": true,
  "pass_type": "monthly",
  "expires_at": "2024-02-15T00:00:00Z",
  "features": {
    "unlimited_likes": true,
    "see_who_likes_you": true,
    "advanced_filters": true
  }
}
```

---

### Student Verification

| Method | Endpoint | Auth | Description |
|--------|----------|------|-------------|
| POST | `/api/verify/student` | Yes | Submit student verification |
| GET | `/api/verify/student/status` | Yes | Check verification status |

#### POST /api/verify/student
**Content-Type:** `multipart/form-data`

| Field | Type | Required | Description |
|-------|------|----------|-------------|
| university | string | Yes | University name |
| student_email | string | Yes | .edu email address |
| graduation_year | int | Yes | Expected graduation year |
| student_id_photo | file | No | Student ID card photo |

---

### Calls (Video/Voice)

| Method | Endpoint | Auth | Description |
|--------|----------|------|-------------|
| POST | `/api/call/initiate` | Yes | Start a call |
| POST | `/api/call/{call_id}/accept` | Yes | Accept incoming call |
| POST | `/api/call/{call_id}/end` | Yes | End call |
| GET | `/api/call/token` | Yes | Get call access token |

---

### Admin Endpoints

| Method | Endpoint | Auth | Description |
|--------|----------|------|-------------|
| GET | `/admin/stats` | Admin | Platform statistics |
| GET | `/admin/users` | Admin | List users |
| POST | `/admin/users/{id}/ban` | Admin | Ban a user |

**Note:** Admin endpoints require `is_admin: true` in JWT claims.

---

## WebSocket Endpoints

### Chat WebSocket
**URL:** `wss://api.nava.dating/ws/chat?match_id={match_id}&token={token}`

**Messages (Client → Server):**
```json
{
  "type": "message",
  "content": "Hello!"
}
```
```json
{
  "type": "typing"
}
```
```json
{
  "type": "read",
  "message_id": 789
}
```

**Messages (Server → Client):**
```json
{
  "type": "message",
  "sender_id": 456,
  "content": "Hello!",
  "message_id": 790,
  "timestamp": "2024-01-15T10:31:00Z"
}
```

### Call WebSocket
**URL:** `wss://api.nava.dating/ws/call?call_id={call_id}&token={token}`

**Signaling Messages:**
- `offer` - WebRTC SDP offer
- `answer` - WebRTC SDP answer
- `ice` - ICE candidate
- `join` - Join call
- `leave` - Leave call
- `end` - End call

---

## GraphQL

**Endpoint:** `POST /graphql`

### Example Query
```graphql
query {
  me {
    id
    name
    age
    matches {
      id
      partner {
        name
        photos
      }
    }
  }
}
```

### Example Mutation
```graphql
mutation {
  likeUser(userId: 456) {
    success
    isMutual
    matchId
  }
}
```

### Complexity Limits
- **Max Depth:** 7 levels
- **Max Complexity:** 200 points

---

## Error Responses

All errors follow this format:
```json
{
  "detail": "Error message here"
}
```

### HTTP Status Codes
| Code | Description |
|------|-------------|
| 200 | Success |
| 201 | Created |
| 400 | Bad Request (validation error) |
| 401 | Unauthorized (invalid/missing token) |
| 403 | Forbidden (insufficient permissions) |
| 404 | Not Found |
| 429 | Too Many Requests (rate limited) |
| 500 | Internal Server Error |

---

## Rate Limiting

| Tier | Limit | Burst |
|------|-------|-------|
| Free | 60 req/min | 15 |
| Premium | 120 req/min | 30 |

Rate limit headers:
```
X-RateLimit-Limit: 120
X-RateLimit-Remaining: 115
X-RateLimit-Reset: 1705312800
```

---

## Pagination

List endpoints support cursor-based pagination:

```
GET /api/discover?limit=20&offset=0
```

Response includes:
```json
{
  "data": [...],
  "has_more": true,
  "next_offset": 20
}
```

---

## Changelog

### v1.0.0 (Current)
- Initial API release
- Authentication, profiles, discovery, matching
- WebSocket chat and calls
- GraphQL support
- Student verification
- Subscription management
