use axum::http::HeaderMap;
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::error::AppError;

#[derive(Debug, Serialize, Deserialize)]
struct Claims {
    sub: String,
    exp: usize,
}

pub fn create_access_token(user_id: i64, secret: &str, expires_minutes: i64) -> Result<String, AppError> {
    let exp = Utc::now()
        .checked_add_signed(Duration::minutes(expires_minutes))
        .ok_or_else(|| AppError::internal("Invalid token expiry"))?
        .timestamp() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        exp,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| AppError::internal("Failed to encode token"))
}

pub fn decode_access_token(token: &str, secret: &str) -> Result<i64, AppError> {
    let validation = Validation::new(Algorithm::HS256);
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|_| AppError::unauthorized("Invalid token"))?;
    data.claims
        .sub
        .parse::<i64>()
        .map_err(|_| AppError::unauthorized("Invalid token subject"))
}

pub fn extract_bearer_token(headers: &HeaderMap) -> Result<String, AppError> {
    let auth = headers
        .get(axum::http::header::AUTHORIZATION)
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| AppError::unauthorized("Missing authorization header"))?;

    let mut parts = auth.splitn(2, ' ');
    let scheme = parts.next().unwrap_or("");
    let token = parts.next().unwrap_or("");
    if scheme.eq_ignore_ascii_case("bearer") && !token.is_empty() {
        Ok(token.to_string())
    } else {
        Err(AppError::unauthorized("Invalid authorization header"))
    }
}
