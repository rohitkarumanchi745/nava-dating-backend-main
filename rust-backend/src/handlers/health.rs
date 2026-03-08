//! Health check endpoints for monitoring and Kubernetes probes
//!
//! Endpoints:
//! - GET /health - Basic health check
//! - GET /health/detailed - Extended health with metrics
//! - GET /ready - Kubernetes readiness probe
//! - GET /live - Kubernetes liveness probe

use axum::{
    extract::State,
    http::StatusCode,
    Json,
};
use serde::Serialize;
use std::sync::atomic::Ordering;

use crate::state::AppState;

/// Basic health check response
#[derive(Serialize)]
pub struct HealthResponse {
    pub status: &'static str,
    pub db: &'static str,
    pub vision: &'static str,
}

/// Extended health check response for load balancers and monitoring
#[derive(Serialize)]
pub struct ExtendedHealthResponse {
    pub status: &'static str,
    pub instance_id: String,
    pub uptime_secs: u64,
    pub db: DbHealthStatus,
    pub redis: RedisHealthStatus,
    pub neo4j: &'static str,
    pub vision: &'static str,
    pub metrics: HealthMetrics,
}

#[derive(Serialize)]
pub struct DbHealthStatus {
    pub status: &'static str,
    pub pool_size: u32,
    pub pool_idle: u32,
}

#[derive(Serialize)]
pub struct RedisHealthStatus {
    pub status: &'static str,
    pub connected: bool,
}

#[derive(Serialize)]
pub struct HealthMetrics {
    pub requests_total: u64,
    pub requests_active: u64,
    pub errors_total: u64,
    pub websocket_connections: u64,
}

/// Basic health check
/// GET /health
pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let db_ok = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();
    let vision_ok = state.vision.is_some();
    let response = HealthResponse {
        status: if db_ok { "ok" } else { "degraded" },
        db: if db_ok { "ok" } else { "down" },
        vision: if vision_ok { "enabled" } else { "disabled" },
    };
    let status = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(response))
}

/// Detailed health check for load balancer and monitoring systems
/// GET /health/detailed
pub async fn health_detailed(State(state): State<AppState>) -> (StatusCode, Json<ExtendedHealthResponse>) {
    // Database health
    let db_ok = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();
    let pool_size = state.db.size();
    let pool_idle = state.db.num_idle();

    // Redis health
    let redis_ok = if let Some(ref redis) = state.redis {
        let redis_service = crate::redis_service::RedisService::new(redis.clone());
        redis_service.ping().await.unwrap_or(false)
    } else {
        false
    };

    // Neo4j health
    let neo4j_ok = state.graph_service.is_some();

    // Vision health
    let vision_ok = state.vision.is_some();

    // Uptime
    let uptime = state.start_time.elapsed().as_secs();

    // Metrics
    let metrics = HealthMetrics {
        requests_total: state.metrics.requests_total.load(Ordering::Relaxed),
        requests_active: state.metrics.requests_active.load(Ordering::Relaxed),
        errors_total: state.metrics.errors_total.load(Ordering::Relaxed),
        websocket_connections: state.metrics.websocket_connections.load(Ordering::Relaxed),
    };

    let overall_status = if db_ok { "healthy" } else { "degraded" };
    let status_code = if db_ok { StatusCode::OK } else { StatusCode::SERVICE_UNAVAILABLE };

    let response = ExtendedHealthResponse {
        status: overall_status,
        instance_id: state.config.instance_id.clone(),
        uptime_secs: uptime,
        db: DbHealthStatus {
            status: if db_ok { "healthy" } else { "unhealthy" },
            pool_size,
            pool_idle: pool_idle as u32,
        },
        redis: RedisHealthStatus {
            status: if redis_ok { "healthy" } else { "unavailable" },
            connected: redis_ok,
        },
        neo4j: if neo4j_ok { "connected" } else { "unavailable" },
        vision: if vision_ok { "enabled" } else { "disabled" },
        metrics,
    };

    (status_code, Json(response))
}

/// Kubernetes Readiness Probe
/// Used by K8s to determine if pod should receive traffic
/// GET /ready
pub async fn readiness_probe(State(state): State<AppState>) -> StatusCode {
    let db_ready = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();
    if db_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

/// Kubernetes Liveness Probe
/// Used by K8s to determine if pod should be restarted
/// GET /live
pub async fn liveness_probe() -> StatusCode {
    StatusCode::OK
}

/// Admin: check which secrets would change if reloaded.
/// GET /admin/secrets/status
/// Does NOT apply changes (K8s rolling restart handles that).
/// Returns list of secrets that differ between current config and file/env.
pub async fn secrets_status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let mut current = state.config.clone();
    let changed = current.reload_secrets();
    Json(serde_json::json!({
        "secrets_pending_rotation": changed,
        "count": changed.len(),
        "note": "Secrets are applied on pod restart. Use K8s rolling restart to rotate.",
    }))
}
