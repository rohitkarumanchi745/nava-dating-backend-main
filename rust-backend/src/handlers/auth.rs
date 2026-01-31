//! Authentication endpoints - OTP flow
//!
//! Endpoints:
//! - POST /auth/send-otp - Send OTP to phone number
//! - POST /auth/verify-otp - Verify OTP and get access token

use std::collections::HashMap;

use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};

use crate::{
    auth::create_access_token,
    error::AppError,
    models::*,
    state::AppState,
};

#[derive(Deserialize)]
pub struct SendOtpPayload {
    pub phone_number: String,
}

#[derive(Serialize)]
pub struct SendOtpResponse {
    pub message: &'static str,
    pub otp: &'static str,
}

/// Send OTP to phone number
/// POST /auth/send-otp
pub async fn send_otp(
    Query(params): Query<HashMap<String, String>>,
    body: Option<Json<SendOtpPayload>>,
) -> Result<Json<SendOtpResponse>, AppError> {
    let phone_number = body
        .map(|Json(payload)| payload.phone_number)
        .or_else(|| params.get("phone_number").cloned())
        .ok_or_else(|| AppError::bad_request("phone_number is required"))?;

    if phone_number.trim().is_empty() {
        return Err(AppError::bad_request("phone_number is required"));
    }

    // TODO: In production, integrate with SMS provider (Twilio, AWS SNS, etc.)
    Ok(Json(SendOtpResponse {
        message: "OTP sent successfully",
        otp: "1234", // Mock OTP for development
    }))
}

#[derive(Deserialize)]
pub struct VerifyOtpPayload {
    pub phone_number: String,
    pub otp: String,
}

/// Verify OTP and return access token
/// POST /auth/verify-otp
pub async fn verify_otp(
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
    body: Option<Json<VerifyOtpPayload>>,
) -> Result<Json<VerifyOtpResponse>, AppError> {
    let payload = body.map(|Json(payload)| payload).unwrap_or_else(|| {
        VerifyOtpPayload {
            phone_number: params.get("phone_number").cloned().unwrap_or_default(),
            otp: params.get("otp").cloned().unwrap_or_default(),
        }
    });

    if payload.phone_number.trim().is_empty() || payload.otp.trim().is_empty() {
        return Err(AppError::bad_request("phone_number and otp are required"));
    }

    // TODO: In production, verify against stored OTP with expiration
    if payload.otp != "1234" {
        return Err(AppError::bad_request("Invalid OTP"));
    }

    // Check if user exists
    let row = sqlx::query_as::<_, UserAuthRow>(
        "SELECT id, is_profile_complete FROM users WHERE phone_number = $1",
    )
    .bind(&payload.phone_number)
    .fetch_optional(&state.db)
    .await?;

    let (user_id, is_new_user, is_profile_complete) = match row {
        Some(user) => (user.id, false, user.is_profile_complete.unwrap_or(false)),
        None => {
            // Create new user
            let result = sqlx::query_scalar::<_, i32>(
                r#"
                INSERT INTO users (phone_number, is_active, is_profile_complete, created_at, updated_at)
                VALUES ($1, TRUE, FALSE, NOW(), NOW())
                RETURNING id
                "#,
            )
            .bind(&payload.phone_number)
            .fetch_one(&state.db)
            .await?;
            (result, true, false)
        }
    };

    // Update last_active
    let _ = sqlx::query("UPDATE users SET last_active = NOW() WHERE id = $1")
        .bind(user_id)
        .execute(&state.db)
        .await;

    let token = create_access_token(
        user_id,
        &state.config.secret_key,
        state.config.access_token_expire_minutes,
    )?;

    Ok(Json(VerifyOtpResponse {
        access_token: token,
        token_type: "bearer",
        user_id,
        is_new_user,
        is_profile_complete,
    }))
}
