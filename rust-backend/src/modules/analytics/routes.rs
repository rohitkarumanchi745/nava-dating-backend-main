//! Analytics HTTP routes — exposed under /analytics/*.

use std::sync::Arc;

use axum::{
    extract::{Query, State},
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use tokio::sync::RwLock;

use super::datafusion_engine::DataFusionEngine;

/// Shared analytics state.
#[derive(Clone)]
pub struct AnalyticsState {
    pub engine: Arc<RwLock<DataFusionEngine>>,
}

pub fn analytics_routes(state: AnalyticsState) -> Router {
    Router::new()
        .route("/refresh", post(refresh))
        .route("/dau", get(dau))
        .route("/funnel", get(funnel))
        .route("/match-rate", get(match_rate))
        .route("/surfaces", get(surfaces))
        .route("/students", get(students))
        .route("/hourly", get(hourly))
        .route("/bandit", get(bandit))
        .route("/query", post(custom_query))
        .with_state(state)
}

#[derive(Deserialize)]
struct RefreshParams {
    #[serde(default = "default_lookback")]
    lookback_hours: i64,
}
fn default_lookback() -> i64 { 72 }

async fn refresh(
    State(state): State<AnalyticsState>,
    Query(params): Query<RefreshParams>,
) -> impl IntoResponse {
    let mut engine = state.engine.write().await;
    match engine.refresh(params.lookback_hours).await {
        Ok(()) => (StatusCode::OK, Json(serde_json::json!({
            "status": "refreshed",
            "lookback_hours": params.lookback_hours,
            "refreshed_at": engine.last_refresh().map(|t| t.to_rfc3339()),
        }))),
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, Json(serde_json::json!({
            "error": e,
        }))),
    }
}

async fn ensure_refreshed(engine: &RwLock<DataFusionEngine>) {
    let needs_refresh = {
        let e = engine.read().await;
        e.last_refresh().is_none()
    };
    if needs_refresh {
        let mut e = engine.write().await;
        let _ = e.refresh(72).await;
    }
}

#[derive(Deserialize)]
struct DauParams {
    #[serde(default = "default_days")]
    days: i64,
}
fn default_days() -> i64 { 7 }

async fn dau(
    State(state): State<AnalyticsState>,
    Query(params): Query<DauParams>,
) -> impl IntoResponse {
    ensure_refreshed(&state.engine).await;
    let engine = state.engine.read().await;
    match engine.dau(params.days).await {
        Ok(rows) => (StatusCode::OK, Json(serde_json::json!({ "dau": rows }))),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))),
    }
}

async fn funnel(State(state): State<AnalyticsState>) -> impl IntoResponse {
    ensure_refreshed(&state.engine).await;
    let engine = state.engine.read().await;
    match engine.engagement_funnel().await {
        Ok(rows) => (StatusCode::OK, Json(serde_json::json!({ "funnel": rows }))),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))),
    }
}

async fn match_rate(State(state): State<AnalyticsState>) -> impl IntoResponse {
    ensure_refreshed(&state.engine).await;
    let engine = state.engine.read().await;
    match engine.match_rate_by_gender().await {
        Ok(rows) => (StatusCode::OK, Json(serde_json::json!({ "match_rate": rows }))),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))),
    }
}

async fn surfaces(State(state): State<AnalyticsState>) -> impl IntoResponse {
    ensure_refreshed(&state.engine).await;
    let engine = state.engine.read().await;
    match engine.surface_performance().await {
        Ok(rows) => (StatusCode::OK, Json(serde_json::json!({ "surfaces": rows }))),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))),
    }
}

async fn students(State(state): State<AnalyticsState>) -> impl IntoResponse {
    ensure_refreshed(&state.engine).await;
    let engine = state.engine.read().await;
    match engine.student_demographics().await {
        Ok(rows) => (StatusCode::OK, Json(serde_json::json!({ "students": rows }))),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))),
    }
}

async fn hourly(State(state): State<AnalyticsState>) -> impl IntoResponse {
    ensure_refreshed(&state.engine).await;
    let engine = state.engine.read().await;
    match engine.hourly_activity().await {
        Ok(rows) => (StatusCode::OK, Json(serde_json::json!({ "hourly": rows }))),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))),
    }
}

async fn bandit(State(state): State<AnalyticsState>) -> impl IntoResponse {
    ensure_refreshed(&state.engine).await;
    let engine = state.engine.read().await;
    match engine.bandit_arm_performance().await {
        Ok(rows) => (StatusCode::OK, Json(serde_json::json!({ "bandit": rows }))),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))),
    }
}

#[derive(Deserialize)]
struct CustomQueryRequest {
    sql: String,
}

async fn custom_query(
    State(state): State<AnalyticsState>,
    Json(req): Json<CustomQueryRequest>,
) -> impl IntoResponse {
    // Block write-like keywords
    let lowered = req.sql.trim().to_lowercase();
    for forbidden in ["insert", "update", "delete", "drop", "alter", "create", "truncate"] {
        if lowered.starts_with(forbidden) {
            return (StatusCode::BAD_REQUEST, Json(serde_json::json!({
                "error": "Only SELECT queries are allowed"
            })));
        }
    }

    ensure_refreshed(&state.engine).await;
    let engine = state.engine.read().await;
    match engine.query(&req.sql).await {
        Ok(rows) => (StatusCode::OK, Json(serde_json::json!({ "rows": rows, "count": rows.len() }))),
        Err(e) => (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": e }))),
    }
}
