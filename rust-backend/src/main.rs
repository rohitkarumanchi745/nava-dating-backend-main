mod auth;
mod config;
mod error;
mod handlers;
mod models;
mod state;
mod vision;

use axum::{routing::{get, post}, Router};
use config::Config;
use sqlx::postgres::PgPoolOptions;
use tower_http::cors::{Any, CorsLayer};
use tracing::{error, info};

use handlers::{
    health, profile_me, profile_status, send_otp, update_profile, verify_otp, verify_selfie,
    vision_analyze,
};
use state::AppState;
use std::sync::Arc;
use tokio::sync::Mutex;
use vision::VisionAnalyzer;

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt()
        .with_env_filter("info")
        .init();

    let config = Config::from_env();
    if config.database_url.is_empty() {
        error!("DATABASE_URL is not set");
        std::process::exit(1);
    }
    if !config.database_url.starts_with("postgres://")
        && !config.database_url.starts_with("postgresql://")
    {
        error!("DATABASE_URL must be Postgres for this Rust service");
        std::process::exit(1);
    }

    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&config.database_url)
        .await
        .unwrap_or_else(|err| {
            error!("failed to connect to database: {err}");
            std::process::exit(1);
        });

    let bind_addr = config.bind_addr.clone();
    let vision = if config.vision_enabled {
        Some(Arc::new(Mutex::new(
            VisionAnalyzer::load(&config).unwrap_or_else(|err| {
                error!("failed to load vision models: {err}");
                std::process::exit(1);
            }),
        )))
    } else {
        None
    };
    let state = AppState { db, config, vision };

    let app = Router::new()
        .route("/health", get(health))
        .route("/send-otp", post(send_otp))
        .route("/verify-otp", post(verify_otp))
        .route("/update-profile", post(update_profile))
        .route("/profile/status", get(profile_status))
        .route("/profile/me", get(profile_me))
        .route("/verify/selfie", post(verify_selfie))
        .route("/vision/analyze", post(vision_analyze))
        .with_state(state)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        );

    info!("listening on {}", bind_addr);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap_or_else(|err| {
            error!("failed to bind: {err}");
            std::process::exit(1);
        });

    if let Err(err) = axum::serve(listener, app).await {
        error!("server error: {err}");
    }
}
