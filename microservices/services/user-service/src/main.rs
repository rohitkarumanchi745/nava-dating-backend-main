//! User Service - Handles user profiles and preferences
//!
//! Endpoints:
//! - GET  /me              - Get current user profile
//! - PUT  /profile         - Update profile
//! - PUT  /preferences     - Update preferences
//! - POST /photos          - Upload photos
//! - GET  /users/:id       - Get user by ID (internal)

use axum::{
    extract::{Path, State},
    routing::{get, put},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use shared::config::{get_env, get_env_port};
use shared::{AppError, AppResult};

#[derive(Clone)]
struct AppState {
    db: PgPool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder().with_max_level(Level::INFO).finish();
    tracing::subscriber::set_global_default(subscriber)?;
    dotenvy::dotenv().ok();

    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&get_env("DATABASE_URL"))
        .await?;

    info!("Connected to database");

    let app = Router::new()
        .route("/health", get(health))
        .route("/me", get(get_profile))
        .route("/profile", put(update_profile))
        .route("/users/:id", get(get_user_by_id))
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::new(AppState { db }));

    let port = get_env_port("USER_SERVICE_PORT", 8002);
    info!("👤 User Service starting on 0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"service": "user-service", "status": "healthy"}))
}

#[derive(Debug, Serialize)]
struct UserProfile {
    id: i32,
    name: Option<String>,
    bio: Option<String>,
    age: Option<i32>,
    gender: Option<String>,
    photos: Vec<String>,
    interests: Vec<String>,
    is_verified: bool,
}

async fn get_profile(State(state): State<Arc<AppState>>) -> AppResult<Json<UserProfile>> {
    // In real impl, extract user_id from JWT
    let user_id = 1; // Placeholder
    
    let profile = sqlx::query_as::<_, (i32, Option<String>, Option<String>, Option<i32>, Option<String>, bool)>(
        "SELECT user_id, name, bio, age, gender, is_verified FROM user_profiles WHERE user_id = $1"
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Profile not found".into()))?;

    Ok(Json(UserProfile {
        id: profile.0,
        name: profile.1,
        bio: profile.2,
        age: profile.3,
        gender: profile.4,
        photos: vec![],
        interests: vec![],
        is_verified: profile.5,
    }))
}

#[derive(Debug, Deserialize)]
struct UpdateProfileRequest {
    name: Option<String>,
    bio: Option<String>,
    age: Option<i32>,
    gender: Option<String>,
}

async fn update_profile(
    State(state): State<Arc<AppState>>,
    Json(req): Json<UpdateProfileRequest>,
) -> AppResult<Json<serde_json::Value>> {
    let user_id = 1; // Placeholder
    
    sqlx::query(
        r#"
        INSERT INTO user_profiles (user_id, name, bio, age, gender)
        VALUES ($1, $2, $3, $4, $5)
        ON CONFLICT (user_id) DO UPDATE SET
            name = COALESCE($2, user_profiles.name),
            bio = COALESCE($3, user_profiles.bio),
            age = COALESCE($4, user_profiles.age),
            gender = COALESCE($5, user_profiles.gender)
        "#
    )
    .bind(user_id)
    .bind(&req.name)
    .bind(&req.bio)
    .bind(&req.age)
    .bind(&req.gender)
    .execute(&state.db)
    .await?;

    Ok(Json(serde_json::json!({"success": true})))
}

async fn get_user_by_id(
    State(state): State<Arc<AppState>>,
    Path(id): Path<i32>,
) -> AppResult<Json<UserProfile>> {
    let profile = sqlx::query_as::<_, (i32, Option<String>, Option<String>, Option<i32>, Option<String>, bool)>(
        "SELECT user_id, name, bio, age, gender, is_verified FROM user_profiles WHERE user_id = $1"
    )
    .bind(id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("User not found".into()))?;

    Ok(Json(UserProfile {
        id: profile.0,
        name: profile.1,
        bio: profile.2,
        age: profile.3,
        gender: profile.4,
        photos: vec![],
        interests: vec![],
        is_verified: profile.5,
    }))
}
