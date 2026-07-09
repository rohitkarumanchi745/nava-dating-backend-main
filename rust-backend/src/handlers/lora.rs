//! Per-user LoRA adapter lifecycle (FedLoRA orchestration).
//!
//! The device submits privacy-safe training signal and polls for its adapter.
//! The Python FedLoRA worker (scripts/fedlora_trainer.py) claims jobs, trains,
//! uploads the adapter, and registers a new version through the admin endpoints.
//! No raw chats ever pass through here — only aggregated signal + DP metadata.

use axum::{
    extract::{Path, State},
    http::HeaderMap,
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use crate::{
    auth::{decode_access_token, extract_bearer_token, AdminClaims},
    error::AppError,
    state::AppState,
};

/// Minimum unconsumed signals before a user is eligible for a training round.
const MIN_SIGNALS_FOR_TRAINING: i64 = 20;

// ============================================================================
// Device-facing
// ============================================================================

#[derive(Serialize)]
pub struct AdapterSpecResponse {
    pub available: bool,
    pub user_id: i32,
    pub version: Option<i32>,
    pub url: Option<String>,
    pub sha256: Option<String>,
    pub size_bytes: Option<i64>,
    pub base_model: Option<String>,
}

/// GET /suggestions/adapter — the authenticated user's current active adapter.
pub async fn get_current_adapter(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<AdapterSpecResponse>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let row = sqlx::query_as::<_, (i32, String, String, i64, String)>(
        "SELECT version, storage_url, sha256, size_bytes, base_model \
         FROM user_lora_adapters WHERE user_id = $1 AND status = 'active' \
         ORDER BY version DESC LIMIT 1",
    )
    .bind(user_id)
    .fetch_optional(state.read_pool())
    .await?;

    Ok(Json(match row {
        Some((version, url, sha256, size_bytes, base_model)) => AdapterSpecResponse {
            available: true,
            user_id,
            version: Some(version),
            url: Some(url),
            sha256: Some(sha256),
            size_bytes: Some(size_bytes),
            base_model: Some(base_model),
        },
        None => AdapterSpecResponse {
            available: false,
            user_id,
            version: None,
            url: None,
            sha256: None,
            size_bytes: None,
            base_model: None,
        },
    }))
}

#[derive(Deserialize)]
pub struct SubmitSignalPayload {
    /// Opaque to the API — the trainer interprets it (LoRA-delta / preference
    /// signal). Must NOT contain raw message text.
    pub payload: Value,
    #[serde(default)]
    pub num_samples: i32,
    pub dp_epsilon: Option<f64>,
    pub dp_delta: Option<f64>,
}

/// POST /fl/lora/signal — device submits privacy-safe training signal.
pub async fn submit_lora_signal(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SubmitSignalPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let id = sqlx::query_scalar::<_, i32>(
        "INSERT INTO lora_training_signals (user_id, payload, num_samples, dp_epsilon, dp_delta) \
         VALUES ($1, $2, $3, $4, $5) RETURNING id",
    )
    .bind(user_id)
    .bind(&body.payload)
    .bind(body.num_samples)
    .bind(body.dp_epsilon)
    .bind(body.dp_delta)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(json!({ "accepted": true, "signal_id": id })))
}

// ============================================================================
// Admin / worker-facing
// ============================================================================

/// POST /admin/lora/train — enqueue a training job for every eligible user
/// (enough new signal, no open job). Call on a schedule or manually.
pub async fn admin_enqueue_training(
    State(state): State<AppState>,
    _admin: AdminClaims,
) -> Result<Json<Value>, AppError> {
    let inserted = sqlx::query_scalar::<_, i32>(
        r#"
        WITH eligible AS (
            SELECT user_id
            FROM lora_training_signals
            WHERE consumed = FALSE
            GROUP BY user_id
            HAVING COUNT(*) >= $1
        )
        INSERT INTO lora_training_jobs (user_id, status, round)
        SELECT e.user_id, 'pending',
               COALESCE((SELECT MAX(version) FROM user_lora_adapters a WHERE a.user_id = e.user_id), 0) + 1
        FROM eligible e
        ON CONFLICT DO NOTHING
        RETURNING id
        "#,
    )
    .bind(MIN_SIGNALS_FOR_TRAINING)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(json!({ "enqueued": inserted.len() })))
}

/// GET /admin/lora/jobs/next — worker atomically claims the next pending job
/// and receives that user's unconsumed signals to train on.
pub async fn worker_next_job(
    State(state): State<AppState>,
    _admin: AdminClaims,
) -> Result<Json<Value>, AppError> {
    let claimed = sqlx::query_as::<_, (i32, i32, i32)>(
        r#"
        UPDATE lora_training_jobs
        SET status = 'running', started_at = NOW()
        WHERE id = (
            SELECT id FROM lora_training_jobs
            WHERE status = 'pending'
            ORDER BY created_at
            FOR UPDATE SKIP LOCKED
            LIMIT 1
        )
        RETURNING id, user_id, round
        "#,
    )
    .fetch_optional(&state.db)
    .await?;

    let (job_id, user_id, round) = match claimed {
        Some(t) => t,
        None => return Ok(Json(json!({ "job": Value::Null }))),
    };

    let signals = sqlx::query_scalar::<_, Value>(
        "SELECT payload FROM lora_training_signals WHERE user_id = $1 AND consumed = FALSE",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(json!({
        "job": {
            "job_id": job_id,
            "user_id": user_id,
            "round": round,
            "signals": signals,
        }
    })))
}

#[derive(Deserialize)]
pub struct RegisterAdapterPayload {
    pub job_id: i32,
    pub user_id: i32,
    pub version: i32,
    pub storage_url: String,
    pub sha256: String,
    #[serde(default)]
    pub size_bytes: i64,
    #[serde(default = "default_base_model")]
    pub base_model: String,
    #[serde(default = "default_rank")]
    pub rank: i32,
    #[serde(default)]
    pub metrics: Value,
}

fn default_base_model() -> String { "bitnet-b1.58-2B-4T".to_string() }
fn default_rank() -> i32 { 8 }

/// POST /admin/lora/adapter — worker registers a freshly trained adapter. This
/// activates the new version, supersedes the old one, consumes the signals, and
/// completes the job — all atomically — then pushes a "ready" event to the device.
pub async fn worker_register_adapter(
    State(state): State<AppState>,
    _admin: AdminClaims,
    Json(body): Json<RegisterAdapterPayload>,
) -> Result<Json<Value>, AppError> {
    let mut tx = state.db.begin().await?;

    sqlx::query(
        "UPDATE user_lora_adapters SET status = 'superseded' \
         WHERE user_id = $1 AND status = 'active'",
    )
    .bind(body.user_id)
    .execute(&mut *tx)
    .await?;

    let metrics = if body.metrics.is_null() { json!({}) } else { body.metrics.clone() };
    let adapter_id = sqlx::query_scalar::<_, i32>(
        r#"
        INSERT INTO user_lora_adapters
            (user_id, version, storage_url, sha256, size_bytes, base_model, rank, status, metrics, activated_at)
        VALUES ($1,$2,$3,$4,$5,$6,$7,'active',$8,NOW())
        RETURNING id
        "#,
    )
    .bind(body.user_id)
    .bind(body.version)
    .bind(&body.storage_url)
    .bind(&body.sha256)
    .bind(body.size_bytes)
    .bind(&body.base_model)
    .bind(body.rank)
    .bind(&metrics)
    .fetch_one(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE lora_training_signals SET consumed = TRUE \
         WHERE user_id = $1 AND consumed = FALSE",
    )
    .bind(body.user_id)
    .execute(&mut *tx)
    .await?;

    sqlx::query(
        "UPDATE lora_training_jobs SET status = 'completed', finished_at = NOW() WHERE id = $1",
    )
    .bind(body.job_id)
    .execute(&mut *tx)
    .await?;

    tx.commit().await?;

    // Tell the device a new adapter is ready (durable outbox + cross-pod fanout).
    crate::handlers::publish_user_event(
        &state,
        body.user_id,
        "lora_adapter_ready",
        json!({ "version": body.version, "adapter_id": adapter_id }),
    )
    .await;

    Ok(Json(json!({ "registered": true, "adapter_id": adapter_id })))
}

#[derive(Deserialize)]
pub struct FailJobPayload {
    pub error: Option<String>,
}

/// POST /admin/lora/jobs/{id}/fail — worker reports a failed job.
pub async fn worker_fail_job(
    State(state): State<AppState>,
    _admin: AdminClaims,
    Path(job_id): Path<i32>,
    Json(body): Json<FailJobPayload>,
) -> Result<Json<Value>, AppError> {
    sqlx::query(
        "UPDATE lora_training_jobs SET status = 'failed', finished_at = NOW(), error = $2 WHERE id = $1",
    )
    .bind(job_id)
    .bind(body.error.unwrap_or_else(|| "unspecified".to_string()))
    .execute(&state.db)
    .await?;

    Ok(Json(json!({ "failed": true, "job_id": job_id })))
}
