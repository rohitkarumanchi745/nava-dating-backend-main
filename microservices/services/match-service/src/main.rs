//! Match Service - Handles discovery, likes, and matches
//!
//! Endpoints:
//! - GET  /discover        - Get profiles to discover
//! - POST /like/:id        - Like a user
//! - POST /pass/:id        - Pass on a user  
//! - GET  /matches         - Get user's matches

use axum::{
    extract::{Path, State},
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
        .route("/discover", get(discover))
        .route("/like/:id", post(like_user))
        .route("/pass/:id", post(pass_user))
        .route("/matches", get(get_matches))
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::new(AppState { db }));

    let port = get_env_port("MATCH_SERVICE_PORT", 8003);
    info!("💕 Match Service starting on 0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"service": "match-service", "status": "healthy"}))
}

#[derive(Debug, Serialize)]
struct DiscoverProfile {
    id: i32,
    name: String,
    age: i32,
    bio: Option<String>,
    photos: Vec<String>,
    compatibility_score: Option<i32>,
}

async fn discover(State(state): State<Arc<AppState>>) -> AppResult<Json<Vec<DiscoverProfile>>> {
    let user_id = 1; // Placeholder - extract from JWT
    
    let profiles = sqlx::query_as::<_, (i32, Option<String>, Option<i32>, Option<String>)>(
        r#"
        SELECT p.user_id, p.name, p.age, p.bio
        FROM user_profiles p
        WHERE p.user_id != $1
        AND p.user_id NOT IN (
            SELECT target_user_id FROM swipes WHERE user_id = $1
        )
        LIMIT 20
        "#
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    let result: Vec<DiscoverProfile> = profiles
        .into_iter()
        .map(|(id, name, age, bio)| DiscoverProfile {
            id,
            name: name.unwrap_or_default(),
            age: age.unwrap_or(0),
            bio,
            photos: vec![],
            compatibility_score: Some(85), // Placeholder
        })
        .collect();

    Ok(Json(result))
}

#[derive(Debug, Serialize)]
struct LikeResponse {
    success: bool,
    is_match: bool,
    match_id: Option<i32>,
}

async fn like_user(
    State(state): State<Arc<AppState>>,
    Path(target_id): Path<i32>,
) -> AppResult<Json<LikeResponse>> {
    let user_id = 1; // Placeholder
    
    // Record the like
    sqlx::query(
        "INSERT INTO swipes (user_id, target_user_id, action) VALUES ($1, $2, 'like') ON CONFLICT DO NOTHING"
    )
    .bind(user_id)
    .bind(target_id)
    .execute(&state.db)
    .await?;

    // Check if it's a mutual like (match)
    let mutual = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM swipes WHERE user_id = $1 AND target_user_id = $2 AND action = 'like')"
    )
    .bind(target_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    let match_id = if mutual {
        // Create match
        let id = sqlx::query_scalar::<_, i32>(
            r#"
            INSERT INTO matches (user1_id, user2_id, status, created_at)
            VALUES ($1, $2, 'active', NOW())
            RETURNING id
            "#
        )
        .bind(user_id.min(target_id))
        .bind(user_id.max(target_id))
        .fetch_one(&state.db)
        .await?;
        Some(id)
    } else {
        None
    };

    Ok(Json(LikeResponse {
        success: true,
        is_match: mutual,
        match_id,
    }))
}

async fn pass_user(
    State(state): State<Arc<AppState>>,
    Path(target_id): Path<i32>,
) -> AppResult<Json<serde_json::Value>> {
    let user_id = 1; // Placeholder
    
    sqlx::query(
        "INSERT INTO swipes (user_id, target_user_id, action) VALUES ($1, $2, 'pass') ON CONFLICT DO NOTHING"
    )
    .bind(user_id)
    .bind(target_id)
    .execute(&state.db)
    .await?;

    Ok(Json(serde_json::json!({"success": true})))
}

#[derive(Debug, Serialize)]
struct MatchInfo {
    match_id: i32,
    user_id: i32,
    name: String,
    photo: Option<String>,
    matched_at: String,
}

async fn get_matches(State(state): State<Arc<AppState>>) -> AppResult<Json<Vec<MatchInfo>>> {
    let user_id = 1; // Placeholder
    
    let matches = sqlx::query_as::<_, (i32, i32, i32, chrono::DateTime<chrono::Utc>)>(
        r#"
        SELECT m.id, m.user1_id, m.user2_id, m.created_at
        FROM matches m
        WHERE (m.user1_id = $1 OR m.user2_id = $1)
        AND m.status = 'active'
        ORDER BY m.created_at DESC
        "#
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    let result: Vec<MatchInfo> = matches
        .into_iter()
        .map(|(match_id, user1_id, user2_id, created_at)| {
            let other_user_id = if user1_id == user_id { user2_id } else { user1_id };
            MatchInfo {
                match_id,
                user_id: other_user_id,
                name: "User".to_string(), // Would fetch from user-service
                photo: None,
                matched_at: created_at.to_rfc3339(),
            }
        })
        .collect();

    Ok(Json(result))
}
