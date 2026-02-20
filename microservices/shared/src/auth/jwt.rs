//! JWT authentication utilities shared across services

use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::AppError;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Claims {
    pub sub: String,        // user_id
    pub exp: usize,         // expiration
    pub iat: usize,         // issued at
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

impl Claims {
    pub fn user_id(&self) -> Result<i32, AppError> {
        self.sub.parse().map_err(|_| AppError::Unauthorized)
    }
}

/// Create a JWT access token
pub fn create_access_token(user_id: i32, secret: &str, expiry_minutes: i64) -> Result<String, AppError> {
    let now = Utc::now();
    let exp = now + Duration::minutes(expiry_minutes);

    let claims = Claims {
        sub: user_id.to_string(),
        exp: exp.timestamp() as usize,
        iat: now.timestamp() as usize,
        scope: Some("access".to_string()),
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| AppError::internal("Failed to create token"))
}

/// Create a refresh token (longer expiry)
pub fn create_refresh_token(user_id: i32, secret: &str) -> Result<String, AppError> {
    let now = Utc::now();
    let exp = now + Duration::days(7);

    let claims = Claims {
        sub: user_id.to_string(),
        exp: exp.timestamp() as usize,
        iat: now.timestamp() as usize,
        scope: Some("refresh".to_string()),
    };

    encode(
        &Header::default(),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| AppError::internal("Failed to create refresh token"))
}

/// Decode and validate a JWT token
pub fn decode_token(token: &str, secret: &str) -> Result<Claims, AppError> {
    decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &Validation::default(),
    )
    .map(|data| data.claims)
    .map_err(|_| AppError::Unauthorized)
}

/// Extract token from Authorization header
pub fn extract_bearer_token(header: &str) -> Option<&str> {
    header.strip_prefix("Bearer ").or_else(|| header.strip_prefix("bearer "))
}
