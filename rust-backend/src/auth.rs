use axum::{
    extract::FromRequestParts,
    http::{HeaderMap, request::Parts},
};
use chrono::{Duration, Utc};
use jsonwebtoken::{decode, encode, Algorithm, DecodingKey, EncodingKey, Header, Validation};
use serde::{Deserialize, Serialize};

use crate::error::AppError;
use crate::state::AppState;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Claims {
    pub sub: String,
    pub exp: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub match_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub call_id: Option<String>,
    #[serde(default)]
    pub is_admin: bool,
}

/// Extractor for admin-only requests - validates JWT and checks admin flag
#[derive(Debug, Clone)]
pub struct AdminClaims(pub Claims);

impl std::ops::Deref for AdminClaims {
    type Target = Claims;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

/// Extractor for authenticated requests - extracts Claims from JWT token
impl FromRequestParts<std::sync::Arc<AppState>> for Claims {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &std::sync::Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| AppError::unauthorized("Missing authorization header"))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .or_else(|| auth_header.strip_prefix("bearer "))
            .ok_or_else(|| AppError::unauthorized("Invalid authorization header format"))?;

        let validation = Validation::new(Algorithm::HS256);
        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(state.config.secret_key.as_bytes()),
            &validation,
        )
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

        Ok(data.claims)
    }
}

/// Extractor for admin-only requests - validates JWT and checks admin flag
impl FromRequestParts<std::sync::Arc<AppState>> for AdminClaims {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &std::sync::Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let claims = Claims::from_request_parts(parts, state).await?;

        if !claims.is_admin {
            return Err(AppError::forbidden("Admin access required"));
        }

        Ok(AdminClaims(claims))
    }
}

// Also implement for non-Arc AppState (used by main routes)
impl FromRequestParts<AppState> for AdminClaims {
    type Rejection = AppError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &AppState,
    ) -> Result<Self, Self::Rejection> {
        let auth_header = parts
            .headers
            .get(axum::http::header::AUTHORIZATION)
            .and_then(|value| value.to_str().ok())
            .ok_or_else(|| AppError::unauthorized("Missing authorization header"))?;

        let token = auth_header
            .strip_prefix("Bearer ")
            .or_else(|| auth_header.strip_prefix("bearer "))
            .ok_or_else(|| AppError::unauthorized("Invalid authorization header format"))?;

        let validation = Validation::new(Algorithm::HS256);
        let data = decode::<Claims>(
            token,
            &DecodingKey::from_secret(state.config.secret_key.as_bytes()),
            &validation,
        )
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

        if !data.claims.is_admin {
            return Err(AppError::forbidden("Admin access required"));
        }

        Ok(AdminClaims(data.claims))
    }
}

/// Create a standard access token for user authentication
pub fn create_access_token(user_id: i64, secret: &str, expires_minutes: i64) -> Result<String, AppError> {
    let exp = Utc::now()
        .checked_add_signed(Duration::minutes(expires_minutes))
        .ok_or_else(|| AppError::internal("Invalid token expiry"))?
        .timestamp() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        exp,
        scope: None,
        match_id: None,
        call_id: None,
        is_admin: false,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| AppError::internal("Failed to encode token"))
}

/// Create an admin access token for administrative users
pub fn create_admin_token(user_id: i32, secret: &str, expires_minutes: i64) -> Result<String, AppError> {
    let exp = Utc::now()
        .checked_add_signed(Duration::minutes(expires_minutes))
        .ok_or_else(|| AppError::internal("Invalid token expiry"))?
        .timestamp() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        exp,
        scope: None,
        match_id: None,
        call_id: None,
        is_admin: true,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| AppError::internal("Failed to encode token"))
}

/// Create a scoped token for call signaling
pub fn create_call_token(
    user_id: i32,
    match_id: &str,
    call_id: &str,
    secret: &str,
    expires_minutes: i64,
) -> Result<String, AppError> {
    let exp = Utc::now()
        .checked_add_signed(Duration::minutes(expires_minutes))
        .ok_or_else(|| AppError::internal("Invalid token expiry"))?
        .timestamp() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        exp,
        scope: Some("call".to_string()),
        match_id: Some(match_id.to_string()),
        call_id: Some(call_id.to_string()),
        is_admin: false,
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| AppError::internal("Failed to encode call token"))
}

/// Create a pair of access and refresh tokens for ambassador login
pub fn create_token_pair(user_id: i64, secret: &str) -> Result<(String, String), AppError> {
    // Access token: 15 minutes
    let access_token = create_access_token(user_id, secret, 15)?;

    // Refresh token: 7 days (10080 minutes)
    let refresh_exp = Utc::now()
        .checked_add_signed(Duration::days(7))
        .ok_or_else(|| AppError::internal("Invalid token expiry"))?
        .timestamp() as usize;

    let refresh_claims = Claims {
        sub: user_id.to_string(),
        exp: refresh_exp,
        scope: Some("refresh".to_string()),
        match_id: None,
        call_id: None,
        is_admin: false,
    };

    let refresh_token = encode(
        &Header::new(Algorithm::HS256),
        &refresh_claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .map_err(|_| AppError::internal("Failed to encode refresh token"))?;

    Ok((access_token, refresh_token))
}

/// Decode and validate an access token, returning the user ID
pub fn decode_access_token(token: &str, secret: &str) -> Result<i32, AppError> {
    let validation = Validation::new(Algorithm::HS256);
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|_| AppError::unauthorized("Invalid token"))?;
    data.claims
        .sub
        .parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token subject"))
}

/// Decode and validate a call token, returning (user_id, match_id, call_id)
pub fn decode_call_token(token: &str, secret: &str) -> Result<(i32, String, String), AppError> {
    let validation = Validation::new(Algorithm::HS256);
    let data = decode::<Claims>(
        token,
        &DecodingKey::from_secret(secret.as_bytes()),
        &validation,
    )
    .map_err(|_| AppError::unauthorized("Invalid call token"))?;

    let scope = data.claims.scope.as_deref().unwrap_or("");
    if scope != "call" {
        return Err(AppError::unauthorized("Token is not a call token"));
    }

    let user_id = data
        .claims
        .sub
        .parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token subject"))?;
    let match_id = data
        .claims
        .match_id
        .ok_or_else(|| AppError::unauthorized("Missing match_id in call token"))?;
    let call_id = data
        .claims
        .call_id
        .ok_or_else(|| AppError::unauthorized("Missing call_id in call token"))?;

    Ok((user_id, match_id, call_id))
}

/// Extract bearer token from Authorization header
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

/// Extract token from query parameter (for WebSocket connections)
pub fn extract_query_token(query: &str) -> Option<String> {
    for pair in query.split('&') {
        let mut kv = pair.splitn(2, '=');
        if let (Some(key), Some(value)) = (kv.next(), kv.next()) {
            if key == "token" {
                return Some(value.to_string());
            }
        }
    }
    None
}
