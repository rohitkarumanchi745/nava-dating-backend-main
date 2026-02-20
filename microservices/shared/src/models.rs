//! Shared data models used across microservices

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::FromRow;

/// User model - basic info shared across services
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct User {
    pub id: i32,
    pub phone_number: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
}

/// User profile - extended info
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct UserProfile {
    pub user_id: i32,
    pub bio: Option<String>,
    pub age: Option<i32>,
    pub gender: Option<String>,
    pub location: Option<String>,
    pub photos: Vec<String>,
    pub interests: Vec<String>,
    pub is_verified: bool,
}

/// Match between two users
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Match {
    pub id: i32,
    pub user1_id: i32,
    pub user2_id: i32,
    pub status: String,
    pub created_at: DateTime<Utc>,
}

/// Chat message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMessage {
    pub id: i32,
    pub match_id: i32,
    pub sender_id: i32,
    pub content: String,
    pub created_at: DateTime<Utc>,
    pub read_at: Option<DateTime<Utc>>,
}

/// Subscription info
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Subscription {
    pub id: i32,
    pub user_id: i32,
    pub tier: String,
    pub status: String,
    pub expires_at: Option<DateTime<Utc>>,
}

/// Ambassador info
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct Ambassador {
    pub id: i32,
    pub user_id: i32,
    pub tier: String,
    pub referral_code: String,
    pub status: String,
}

/// Inter-service request/response types
pub mod api {
    use serde::{Deserialize, Serialize};

    #[derive(Debug, Serialize, Deserialize)]
    pub struct ValidateTokenRequest {
        pub token: String,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct ValidateTokenResponse {
        pub valid: bool,
        pub user_id: Option<i32>,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct GetUserRequest {
        pub user_id: i32,
    }

    #[derive(Debug, Serialize, Deserialize)]
    pub struct ServiceHealth {
        pub service: String,
        pub status: String,
        pub version: String,
    }
}
