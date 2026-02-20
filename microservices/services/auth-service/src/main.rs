//! Auth Service - Handles authentication and authorization
//!
//! Endpoints:
//! - POST /send-otp     - Send OTP to phone number
//! - POST /verify-otp   - Verify OTP and get tokens
//! - POST /refresh      - Refresh access token
//! - POST /validate     - Validate token (internal)
//! - POST /logout       - Invalidate refresh token

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use shared::auth::{create_access_token, create_refresh_token, decode_token};
use shared::config::{get_env, get_env_or, get_env_port};
use shared::{AppError, AppResult};

/// Application state
#[derive(Clone)]
struct AppState {
    db: PgPool,
    jwt_secret: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber)?;

    dotenvy::dotenv().ok();

    // Database connection
    let database_url = get_env("DATABASE_URL");
    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&database_url)
        .await?;

    info!("Connected to database");

    let state = AppState {
        db,
        jwt_secret: get_env_or("JWT_SECRET", "dev-secret-change-in-production"),
    };

    let app = Router::new()
        .route("/health", get(health))
        .route("/send-otp", post(send_otp))
        .route("/verify-otp", post(verify_otp))
        .route("/refresh", post(refresh_token))
        .route("/validate", post(validate_token))
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::new(state));

    let port = get_env_port("AUTH_SERVICE_PORT", 8001);
    let addr = format!("0.0.0.0:{}", port);

    info!("🔐 Auth Service starting on {}", addr);

    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Health check
async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({
        "service": "auth-service",
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

// ============================================
// Request/Response types
// ============================================

#[derive(Debug, Deserialize)]
struct SendOtpRequest {
    phone_number: String,
}

#[derive(Debug, Serialize)]
struct SendOtpResponse {
    message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    otp: Option<String>, // Only in dev mode
}

#[derive(Debug, Deserialize)]
struct VerifyOtpRequest {
    phone_number: String,
    otp: String,
}

#[derive(Debug, Serialize)]
struct VerifyOtpResponse {
    access_token: String,
    refresh_token: String,
    user_id: i32,
    is_new_user: bool,
    is_profile_complete: bool,
}

#[derive(Debug, Deserialize)]
struct RefreshRequest {
    refresh_token: String,
}

#[derive(Debug, Serialize)]
struct RefreshResponse {
    access_token: String,
}

#[derive(Debug, Deserialize)]
struct ValidateRequest {
    token: String,
}

#[derive(Debug, Serialize)]
struct ValidateResponse {
    valid: bool,
    user_id: Option<i32>,
}

// ============================================
// Handlers
// ============================================

/// Send OTP to phone number
async fn send_otp(
    State(state): State<Arc<AppState>>,
    Json(req): Json<SendOtpRequest>,
) -> AppResult<Json<SendOtpResponse>> {
    // Generate 6-digit OTP
    let otp: String = (0..6)
        .map(|_| rand::random::<u8>() % 10)
        .map(|d| char::from_digit(d as u32, 10).unwrap())
        .collect();

    // Store OTP in database (expires in 5 minutes)
    sqlx::query(
        r#"
        INSERT INTO otp_codes (phone_number, code, expires_at)
        VALUES ($1, $2, NOW() + INTERVAL '5 minutes')
        ON CONFLICT (phone_number) 
        DO UPDATE SET code = $2, expires_at = NOW() + INTERVAL '5 minutes'
        "#,
    )
    .bind(&req.phone_number)
    .bind(&otp)
    .execute(&state.db)
    .await?;

    // In production, send OTP via SMS (Twilio, etc.)
    // For now, return OTP in response (dev mode only)
    let is_dev = std::env::var("ENVIRONMENT").unwrap_or_default() != "production";

    Ok(Json(SendOtpResponse {
        message: "OTP sent successfully".to_string(),
        otp: if is_dev { Some(otp) } else { None },
    }))
}

/// Verify OTP and return tokens
async fn verify_otp(
    State(state): State<Arc<AppState>>,
    Json(req): Json<VerifyOtpRequest>,
) -> AppResult<Json<VerifyOtpResponse>> {
    // Verify OTP
    let valid = sqlx::query_scalar::<_, bool>(
        r#"
        SELECT EXISTS(
            SELECT 1 FROM otp_codes 
            WHERE phone_number = $1 AND code = $2 AND expires_at > NOW()
        )
        "#,
    )
    .bind(&req.phone_number)
    .bind(&req.otp)
    .fetch_one(&state.db)
    .await?;

    if !valid {
        return Err(AppError::BadRequest("Invalid or expired OTP".to_string()));
    }

    // Delete used OTP
    sqlx::query("DELETE FROM otp_codes WHERE phone_number = $1")
        .bind(&req.phone_number)
        .execute(&state.db)
        .await?;

    // Find or create user
    let user = sqlx::query_as::<_, (i32, bool)>(
        r#"
        WITH new_user AS (
            INSERT INTO users (phone_number, is_active, created_at)
            VALUES ($1, true, NOW())
            ON CONFLICT (phone_number) DO NOTHING
            RETURNING id, true as is_new
        )
        SELECT id, false as is_new FROM users WHERE phone_number = $1
        UNION ALL
        SELECT * FROM new_user
        LIMIT 1
        "#,
    )
    .bind(&req.phone_number)
    .fetch_one(&state.db)
    .await?;

    let (user_id, is_new_user) = user;

    // Check if profile is complete
    let is_profile_complete = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM user_profiles WHERE user_id = $1 AND name IS NOT NULL)",
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(false);

    // Generate tokens
    let access_token = create_access_token(user_id, &state.jwt_secret, 15)?;
    let refresh_token = create_refresh_token(user_id, &state.jwt_secret)?;

    Ok(Json(VerifyOtpResponse {
        access_token,
        refresh_token,
        user_id,
        is_new_user,
        is_profile_complete,
    }))
}

/// Refresh access token
async fn refresh_token(
    State(state): State<Arc<AppState>>,
    Json(req): Json<RefreshRequest>,
) -> AppResult<Json<RefreshResponse>> {
    // Decode refresh token
    let claims = decode_token(&req.refresh_token, &state.jwt_secret)?;

    // Verify it's a refresh token
    if claims.scope.as_deref() != Some("refresh") {
        return Err(AppError::Unauthorized);
    }

    let user_id = claims.user_id()?;

    // Generate new access token
    let access_token = create_access_token(user_id, &state.jwt_secret, 15)?;

    Ok(Json(RefreshResponse { access_token }))
}

/// Validate token (internal endpoint for other services)
async fn validate_token(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ValidateRequest>,
) -> Json<ValidateResponse> {
    match decode_token(&req.token, &state.jwt_secret) {
        Ok(claims) => Json(ValidateResponse {
            valid: true,
            user_id: claims.user_id().ok(),
        }),
        Err(_) => Json(ValidateResponse {
            valid: false,
            user_id: None,
        }),
    }
}

// Add rand dependency for OTP generation
use rand::Rng;
