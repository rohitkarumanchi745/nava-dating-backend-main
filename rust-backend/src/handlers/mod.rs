// Graph-powered endpoints module
pub mod graph_handlers;
// Web payments module (Razorpay + Stripe)
pub mod payments;
// Ads monetization module
pub mod ads;
// Ambassador program module
pub mod ambassador;
// Per-user LoRA adapter lifecycle (FedLoRA orchestration)
pub mod lora;
// Agentic auto-match endpoints
pub mod matchmaker;
// GNN embedding worker endpoints
pub mod gnn;
// Visual embedding worker endpoints
pub mod visual;
// CoreML photo search + attestation verification
pub mod clip;

use std::collections::HashMap;
use std::path::Path;

use axum::{
    extract::{Multipart, Path as AxumPath, Query, State, WebSocketUpgrade},
    http::{HeaderMap, StatusCode},
    response::IntoResponse,
    Json,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{Datelike, NaiveDate, NaiveDateTime, Utc};
use image::codecs::jpeg::JpegEncoder;
use image::{ColorType, DynamicImage};
use rand::Rng;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::fs;
use tokio::task;
use uuid::Uuid;

use crate::{
    auth::{create_access_token, create_call_token, decode_access_token, extract_bearer_token, AdminClaims},
    config::Config,
    error::AppError,
    models::*,
    services::photo_pipeline::{self, PhotoVerdict, PipelineTimeouts},
    state::AppState,
    vision::VisionAnalysis,
    websocket,
};

// ============================================================================
// Constants
// ============================================================================

const ALLOWED_GENDERS: [&str; 4] = ["male", "female", "non_binary", "other"];

// ============================================================================
// Health Check
// ============================================================================

#[derive(Serialize)]
pub struct HealthResponse {
    status: &'static str,
    db: &'static str,
    vision: &'static str,
}

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

/// Extended health check response for load balancers and monitoring
#[derive(Serialize)]
pub struct ExtendedHealthResponse {
    pub status: &'static str,
    pub instance_id: String,
    pub uptime_secs: u64,
    pub db: DbHealthStatus,
    pub redis: RedisHealthStatus,
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

/// Detailed health check for load balancer and monitoring systems
/// GET /health/detailed
pub async fn health_detailed(State(state): State<AppState>) -> (StatusCode, Json<ExtendedHealthResponse>) {
    use std::sync::atomic::Ordering;

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
        vision: if vision_ok { "enabled" } else { "disabled" },
        metrics,
    };

    (status_code, Json(response))
}

/// Kubernetes Readiness Probe
/// Used by K8s to determine if pod should receive traffic
/// Configure in deployment.yaml:
///   readinessProbe:
///     httpGet:
///       path: /ready
///       port: 8080
///     initialDelaySeconds: 5
///     periodSeconds: 10
/// GET /ready
pub async fn readiness_probe(State(state): State<AppState>) -> StatusCode {
    // Check database connectivity - required for readiness
    let db_ready = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();

    // For readiness, we only require DB (Redis and Neo4j are optional)
    if db_ready {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    }
}

/// Kubernetes Liveness Probe
/// Used by K8s to determine if pod should be restarted
/// Configure in deployment.yaml:
///   livenessProbe:
///     httpGet:
///       path: /live
///       port: 8080
///     initialDelaySeconds: 15
///     periodSeconds: 20
/// GET /live
pub async fn liveness_probe() -> StatusCode {
    // If this endpoint responds, the service is alive
    StatusCode::OK
}

/// Admin: check which secrets would change if reloaded from file/env.
/// GET /admin/secrets/status
/// Does NOT apply changes (K8s rolling restart handles that).
pub async fn secrets_status(State(state): State<AppState>) -> Json<Value> {
    let mut current = state.config.clone();
    let changed = current.reload_secrets();
    Json(json!({
        "secrets_pending_rotation": changed,
        "count": changed.len(),
        "note": "Secrets are applied on pod restart. Use K8s rolling restart to rotate.",
    }))
}

// ============================================================================
// Authentication - OTP Flow
// ============================================================================

#[derive(Deserialize)]
pub struct SendOtpPayload {
    phone_number: String,
}

#[derive(Serialize)]
pub struct SendOtpResponse {
    message: &'static str,
    otp: &'static str,
}

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

    // Mock OTP - in production, integrate with SMS provider
    Ok(Json(SendOtpResponse {
        message: "OTP sent successfully",
        otp: "1234",
    }))
}

#[derive(Deserialize)]
pub struct VerifyOtpPayload {
    phone_number: String,
    otp: String,
}

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
            let result = sqlx::query_scalar::<_, i64>(
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

// ============================================================================
// Profile Management
// ============================================================================

pub async fn update_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let vision = state.vision.clone();

    let mut name: Option<String> = None;
    let mut dob_raw: Option<String> = None;
    let mut gender: Option<String> = None;
    let mut university: Option<String> = None;
    let mut university_location: Option<String> = None;
    let mut study: Option<String> = None;
    // Collect raw photo bytes first; pipeline runs after multipart parsing.
    let mut photo_bytes: Vec<Option<Vec<u8>>> = vec![None, None, None];

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::bad_request("Invalid multipart data"))?
    {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "name" => {
                let value = read_text_field(&mut field, 256).await?;
                if !value.trim().is_empty() {
                    name = Some(value);
                }
            }
            "dob" => {
                let value = read_text_field(&mut field, 32).await?;
                if !value.trim().is_empty() {
                    dob_raw = Some(value);
                }
            }
            "gender" => {
                let value = read_text_field(&mut field, 32).await?;
                if !value.trim().is_empty() {
                    gender = Some(value.to_lowercase());
                }
            }
            "university" => {
                let value = read_text_field(&mut field, 256).await?;
                if !value.trim().is_empty() {
                    university = Some(value);
                }
            }
            "university_location" => {
                let value = read_text_field(&mut field, 256).await?;
                if !value.trim().is_empty() {
                    university_location = Some(value);
                }
            }
            "study" => {
                let value = read_text_field(&mut field, 200).await?;
                if !value.trim().is_empty() {
                    study = Some(value);
                }
            }
            "profile_photo_1" | "profile_photo_2" | "profile_photo_3" => {
                let idx = match field_name.as_str() {
                    "profile_photo_1" => 0,
                    "profile_photo_2" => 1,
                    "profile_photo_3" => 2,
                    _ => continue,
                };
                let content_type = field
                    .content_type()
                    .map(|value| value.to_string())
                    .unwrap_or_default();
                if !content_type.starts_with("image/") {
                    return Err(AppError::bad_request(format!(
                        "Photo {} must be an image",
                        idx + 1
                    )));
                }
                let bytes = read_binary_field(&mut field, state.config.max_photo_bytes).await?;
                photo_bytes[idx] = Some(bytes);
            }
            _ => {}
        }
    }

    let mut name = name.ok_or_else(|| AppError::bad_request("name is required"))?;
    let dob_raw = dob_raw.ok_or_else(|| AppError::bad_request("dob is required"))?;
    let gender = gender.ok_or_else(|| AppError::bad_request("gender is required"))?;

    if !ALLOWED_GENDERS.contains(&gender.as_str()) {
        return Err(AppError::bad_request("Invalid gender"));
    }

    let dob = NaiveDate::parse_from_str(dob_raw.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::bad_request("dob must be YYYY-MM-DD"))?;
    if calculate_age(dob) < 18 {
        return Err(AppError::bad_request("Must be at least 18 years old"));
    }

    // Validate all 3 photos are present
    let mut raw_photos: Vec<(usize, Vec<u8>)> = Vec::new();
    for (idx, entry) in photo_bytes.into_iter().enumerate() {
        match entry {
            Some(bytes) => raw_photos.push((idx + 1, bytes)),
            None => {
                return Err(AppError::bad_request(format!(
                    "profile_photo_{} is required",
                    idx + 1
                )))
            }
        }
    }

    // ── Run async photo pipeline on each photo ──────────────────────────
    let pipeline_timeouts = PipelineTimeouts::default();
    let mut saved_paths = Vec::new();
    let mut insights = Vec::new();
    let mut avg_attractiveness: Option<f64> = None;
    let mut attractiveness_sum = 0.0;
    let mut attractiveness_count = 0;
    let mut rejection_reason = String::new();

    let upload_dir = &state.config.upload_dir;
    fs::create_dir_all(upload_dir)
        .await
        .map_err(|_| AppError::internal("Failed to create upload directory"))?;

    for (idx, bytes) in raw_photos.into_iter() {
        let photo_slot = format!("profile_photo_{}", idx);

        // Run the full pipeline: resize → quality → NSFW → liveness → duplicate-face
        let pipeline_result = photo_pipeline::run_pipeline(
            bytes,
            user_id,
            &photo_slot,
            vision.clone(),
            state.moderation.clone(),
            &state.db,
            &pipeline_timeouts,
        )
        .await?;

        // Track metrics
        if pipeline_result.verdict == PhotoVerdict::Rejected {
            state.metrics.inc_photos_rejected();
            // Find the failing stage for the user-facing message
            if let Some(failed) = pipeline_result.stages.iter().find(|s| !s.passed) {
                rejection_reason = format!(
                    "Photo {} rejected: {}",
                    idx,
                    failed.detail.as_deref().unwrap_or("policy violation")
                );
            }
            // Clean up any files already saved in this request
            cleanup_files(&saved_paths).await;
            return Err(AppError::bad_request(&rejection_reason));
        }

        if pipeline_result.verdict == PhotoVerdict::NeedsReview {
            state.metrics.inc_moderation_actions();
        }

        // Save processed image to disk (EXIF-stripped, resized)
        let image = pipeline_result
            .processed_image
            .as_ref()
            .ok_or_else(|| AppError::internal("Pipeline did not produce image"))?;

        let filename = format!(
            "{}_photo_{}_{}_{}.jpg",
            user_id,
            idx,
            Utc::now().timestamp(),
            Uuid::new_v4()
        );
        let path = Path::new(upload_dir).join(&filename);
        let jpeg_bytes = encode_jpeg(image)
            .map_err(|_| AppError::internal("Failed to encode image"))?;
        if let Err(err) = fs::write(&path, &jpeg_bytes).await {
            cleanup_files(&saved_paths).await;
            return Err(AppError::internal(format!(
                "Failed to save photo: {err}"
            )));
        }
        // Store URL path (not filesystem path) so it's directly usable by clients
        saved_paths.push(format!("/uploads/{}", filename));

        // Build insights from pipeline quality scores
        if let Some(ref qr) = pipeline_result.quality {
            attractiveness_sum += qr.composite_score as f64;
            attractiveness_count += 1;
            insights.push(json!({
                "quality": qr.composite_score,
                "blur_score": qr.blur_score,
                "low_light_score": qr.low_light_score,
                "face_ratio": qr.face_ratio,
                "flags": qr.flags,
                "verdict": pipeline_result.verdict,
                "stages": pipeline_result.stages,
            }));
        } else {
            insights.push(json!({
                "quality": null,
                "verdict": pipeline_result.verdict,
                "stages": pipeline_result.stages,
            }));
        }

        // Generate renditions in the background (non-blocking)
        let base_key = format!("users/{}/photos/{}", user_id, filename);
        let renditions = photo_pipeline::generate_renditions(image, &base_key);
        // Store rendition metadata (S3 upload would go here in production)
        let rendition_meta: Vec<_> = renditions
            .iter()
            .map(|r| json!({
                "name": r.rendition.name,
                "format": r.rendition.format,
                "width": r.rendition.width,
                "height": r.rendition.height,
                "size_bytes": r.rendition.size_bytes,
                "key": r.rendition.key,
            }))
            .collect();

        // Log renditions to media_renditions table
        let _ = sqlx::query(
            r#"INSERT INTO media_renditions (user_id, original_key, renditions)
               VALUES ($1, $2, $3)"#,
        )
        .bind(user_id as i64)
        .bind(&base_key)
        .bind(serde_json::to_value(&rendition_meta).unwrap_or_default())
        .execute(&state.db)
        .await;
    }

    if attractiveness_count > 0 {
        avg_attractiveness = Some(attractiveness_sum / attractiveness_count as f64);
    }

    let csv_paths = saved_paths.join(",");
    let photos_json = sqlx::types::Json(saved_paths.clone());

    // Identity lock: once student-verified, users.name is immutable.
    // Any new value from the client is redirected into users.display_name.
    // Rationale: name search relies on users.name being a stable verified key.
    let verified_row = sqlx::query_as::<_, (Option<bool>, Option<String>)>(
        "SELECT is_student_verified, name FROM users WHERE id = $1"
    )
    .bind(user_id).fetch_one(&state.db).await.ok();
    let (is_verified, current_name) = match verified_row {
        Some((v, n)) => (v.unwrap_or(false), n),
        None => (false, None),
    };

    let display_name_update: Option<String> = if is_verified && current_name.as_deref().map(|c| c != name.as_str()).unwrap_or(false) {
        let new_display = name.clone();
        name = current_name.unwrap_or(name); // pin to verified value
        Some(new_display)
    } else {
        None
    };

    let result = sqlx::query(
        r#"
        UPDATE users
        SET name = $1,
            dob = $2,
            gender = $3,
            profile_photo_url = $4,
            profile_photo_1 = $5,
            profile_photo_2 = $6,
            profile_photo_3 = $7,
            profile_photos = $8,
            attractiveness_score = $9,
            university = COALESCE($11, university),
            location_text = COALESCE($12, location_text),
            profession_title = COALESCE($13, profession_title),
            is_profile_complete = TRUE,
            updated_at = NOW(),
            last_photo_updated_at = NOW()
        WHERE id = $10
        "#,
    )
    .bind(&name)
    .bind(dob)
    .bind(&gender)
    .bind(&csv_paths)
    .bind(saved_paths.get(0).cloned())
    .bind(saved_paths.get(1).cloned())
    .bind(saved_paths.get(2).cloned())
    .bind(photos_json)
    .bind(avg_attractiveness)
    .bind(user_id)
    .bind(&university)
    .bind(&university_location)
    .bind(&study)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        cleanup_files(&saved_paths).await;
        return Err(AppError::not_found("User not found"));
    }

    // If verified user tried to change name, persist it as display_name instead.
    if let Some(ref dn) = display_name_update {
        let _ = sqlx::query("UPDATE users SET display_name = $1, updated_at = NOW() WHERE id = $2")
            .bind(dn).bind(user_id).execute(&state.db).await;
    }

    // Create default preferences if not exist
    let _ = sqlx::query(
        r#"
        INSERT INTO user_preferences (user_id, min_age, max_age, max_distance, created_at, updated_at)
        VALUES ($1, 18, 50, $2, NOW(), NOW())
        ON CONFLICT (user_id) DO NOTHING
        "#,
    )
    .bind(user_id)
    .bind(state.config.default_max_distance_km)
    .execute(&state.db)
    .await;

    Ok(Json(json!({
        "message": "Profile updated successfully",
        "photos": saved_paths,
        "photo_insights": insights,
    })))
}

pub async fn update_bio(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdateBioRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    if payload.bio.len() > 500 {
        return Err(AppError::bad_request("Bio must be 500 characters or less"));
    }

    let result = sqlx::query("UPDATE users SET bio = $1, updated_at = NOW() WHERE id = $2")
        .bind(&payload.bio)
        .bind(user_id)
        .execute(&state.db)
        .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::not_found("User not found"));
    }

    Ok(Json(json!({ "message": "Bio updated successfully" })))
}

/// POST /users/display-name
/// Update the mutable UI name. Does NOT change users.name (search key) for
/// verified users. Free-text, max 60 chars. Falls back to users.name in reads
/// when empty/null.
#[derive(Deserialize)]
pub struct UpdateDisplayNameRequest { pub display_name: String }

pub async fn update_display_name(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdateDisplayNameRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let trimmed = payload.display_name.trim();
    if trimmed.is_empty() {
        // Empty → clear display_name, fall back to users.name in reads.
        sqlx::query("UPDATE users SET display_name = NULL, updated_at = NOW() WHERE id = $1")
            .bind(user_id).execute(&state.db).await?;
        return Ok(Json(json!({ "display_name": null, "cleared": true })));
    }
    if trimmed.chars().count() > 60 {
        return Err(AppError::bad_request("display_name must be 60 characters or less"));
    }

    sqlx::query("UPDATE users SET display_name = $1, updated_at = NOW() WHERE id = $2")
        .bind(trimmed).bind(user_id).execute(&state.db).await?;

    Ok(Json(json!({ "display_name": trimmed })))
}

/// POST /profile/display-name-in-search
/// Toggle whether display_name appears as an alias in /search/students results.
/// Default: FALSE. Verified users.name always appears regardless.
#[derive(Deserialize)]
pub struct ShowDisplayNameInSearchRequest { pub enabled: bool }

pub async fn set_show_display_name_in_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ShowDisplayNameInSearchRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    sqlx::query("UPDATE users SET show_display_name_in_search = $1, updated_at = NOW() WHERE id = $2")
        .bind(payload.enabled).bind(user_id).execute(&state.db).await?;

    Ok(Json(json!({ "show_display_name_in_search": payload.enabled })))
}

/// POST /profile/show-verified-name
/// Toggle whether OTHER users viewing this person's profile see users.name.
/// Default: TRUE. Owner always sees their own verified name.
/// Does NOT affect name search — search always uses users.name.
#[derive(Deserialize)]
pub struct ShowVerifiedNameRequest { pub enabled: bool }

pub async fn set_show_verified_name(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ShowVerifiedNameRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    sqlx::query("UPDATE users SET show_verified_name = $1, updated_at = NOW() WHERE id = $2")
        .bind(payload.enabled).bind(user_id).execute(&state.db).await?;

    Ok(Json(json!({ "show_verified_name": payload.enabled })))
}

// ============================================================================
// Voice Intro Upload
// ============================================================================

#[derive(Deserialize)]
pub struct VoiceIntroRequest {
    pub voice_url: Option<String>,
    pub duration_seconds: i32,
}

pub async fn upload_voice_intro(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let mut voice_url: Option<String> = None;
    let mut duration: Option<i32> = None;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::bad_request("Invalid multipart data"))?
    {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "voice_url" => {
                let value = read_text_field(&mut field, 1024).await?;
                if !value.trim().is_empty() {
                    voice_url = Some(value);
                }
            }
            "duration_seconds" | "duration" => {
                let value = read_text_field(&mut field, 16).await?;
                duration = value.parse().ok();
            }
            "voice_file" | "audio" => {
                let content_type = field
                    .content_type()
                    .map(|v| v.to_string())
                    .unwrap_or_default();

                // Accept audio files
                if !content_type.starts_with("audio/") && !content_type.contains("webm") && !content_type.contains("mp4") {
                    return Err(AppError::bad_request("File must be an audio file"));
                }

                let bytes = read_binary_field(&mut field, 5 * 1024 * 1024).await?; // 5MB max

                // Save the file
                let upload_dir = &state.config.upload_dir;
                let voice_dir = Path::new(upload_dir).join("voice");
                fs::create_dir_all(&voice_dir)
                    .await
                    .map_err(|_| AppError::internal("Failed to create voice directory"))?;

                let ext = if content_type.contains("webm") { "webm" }
                    else if content_type.contains("mp4") || content_type.contains("m4a") { "m4a" }
                    else { "mp3" };
                let filename = format!(
                    "voice_{}_{}.{}",
                    user_id,
                    Utc::now().timestamp(),
                    ext
                );
                let path = voice_dir.join(&filename);

                fs::write(&path, bytes)
                    .await
                    .map_err(|_| AppError::internal("Failed to save voice file"))?;

                voice_url = Some(format!("/uploads/voice/{}", filename));
            }
            _ => {}
        }
    }

    let url = voice_url.ok_or_else(|| AppError::bad_request("Voice file or URL is required"))?;
    let dur = duration.unwrap_or(15); // Default 15 seconds if not provided

    // Validate duration (max 30 seconds)
    if dur <= 0 || dur > 30 {
        return Err(AppError::bad_request("Voice intro must be between 1-30 seconds"));
    }

    // Update user's voice intro
    sqlx::query(
        "UPDATE users SET voice_intro_url = $1, voice_intro_duration = $2, updated_at = NOW() WHERE id = $3"
    )
    .bind(&url)
    .bind(dur)
    .bind(user_id)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "success": true,
        "voice_intro_url": url,
        "duration_seconds": dur,
        "message": "Voice intro uploaded successfully"
    })))
}

/// JSON-based voice intro upload (for URL-based uploads)
pub async fn upload_voice_intro_json(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<VoiceIntroRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let url = payload.voice_url.ok_or_else(|| AppError::bad_request("voice_url is required"))?;
    let duration = payload.duration_seconds;

    if duration <= 0 || duration > 30 {
        return Err(AppError::bad_request("Voice intro must be between 1-30 seconds"));
    }

    sqlx::query(
        "UPDATE users SET voice_intro_url = $1, voice_intro_duration = $2, updated_at = NOW() WHERE id = $3"
    )
    .bind(&url)
    .bind(duration)
    .bind(user_id)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "success": true,
        "voice_intro_url": url,
        "duration_seconds": duration,
        "message": "Voice intro uploaded successfully"
    })))
}

/// Track voice intro playback for ML training
#[derive(Deserialize)]
pub struct TrackVoicePlayRequest {
    pub target_user_id: i32,
    pub play_duration_seconds: Option<i32>,
}

pub async fn track_voice_play(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<TrackVoicePlayRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Log the voice play event
    let _ = sqlx::query(
        r#"
        INSERT INTO interaction_events (user_id, target_user_id, event_type, surface, reward, created_at)
        VALUES ($1, $2, 'voice_play', 'discover', $3, NOW())
        "#
    )
    .bind(user_id)
    .bind(payload.target_user_id)
    .bind(payload.play_duration_seconds.map(|d| d as f64 / 30.0).unwrap_or(0.5))
    .execute(&state.db)
    .await;

    Ok(Json(json!({
        "success": true,
        "message": "Voice play tracked"
    })))
}

pub async fn profile_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let row = sqlx::query_as::<_, ProfileStatusRow>(
        r#"
        SELECT name, dob, gender, bio, profile_photo_url, profile_photos, is_profile_complete
        FROM users
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .fetch_optional(state.read_pool())
    .await?;

    let user = row.ok_or_else(|| AppError::not_found("User not found"))?;
    let is_complete = user.is_profile_complete.unwrap_or(false);
    let completion = if is_complete {
        100
    } else {
        let checks = [
            user.name.as_ref().map(|v| !v.is_empty()).unwrap_or(false),
            user.dob.is_some(),
            user.gender.as_ref().map(|v| !v.is_empty()).unwrap_or(false),
            user.bio.as_ref().map(|v| !v.is_empty()).unwrap_or(false),
            user.profile_photo_url
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
                || user.profile_photos.as_ref().map(is_json_array_nonempty).unwrap_or(false),
        ];
        let filled = checks.iter().filter(|c| **c).count();
        let total = checks.len().max(1);
        let percent = ((filled as f32 / total as f32) * 100.0).round() as i32;
        percent.min(100)
    };

    Ok(Json(json!({
        "profile_completion": completion,
        "is_profile_complete": is_complete,
    })))
}

pub async fn profile_me(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let read_db = state.read_pool();

    let user = fetch_user_by_id(read_db, user_id).await?;
    let user = user.ok_or_else(|| AppError::not_found("User not found"))?;

    let (preferences, location, subscriptions, spots) = tokio::try_join!(
        fetch_user_preferences(read_db, user_id),
        fetch_user_location(read_db, user_id),
        fetch_user_subscriptions(read_db, user_id),
        fetch_user_spots(read_db, user_id, 10),
    )?;

    let profile = json!({
        "id": user.id,
        "phone_number": user.phone_number,
        "email": user.email,
        "name": user.name,
        "display_name": user.display_name,
        "show_verified_name": user.show_verified_name.unwrap_or(true),
        "show_display_name_in_search": user.show_display_name_in_search.unwrap_or(false),
        "dob": user.dob.map(format_date),
        "age": user.dob.map(calculate_age),
        "gender": user.gender,
        "bio": user.bio,
        "location": user.location_text,
        "interests": json_array_or_empty(user.interests.as_ref()),
        "languages": json_array_or_empty(user.languages.as_ref()),
        "looking_for": user.looking_for,
        "profession_category": user.profession_category,
        "profession_title": user.profession_title,
        "height_cm": user.height_cm,
        "photos": get_user_photos(&user),
        "is_profile_complete": user.is_profile_complete,
        "profile_completion": compute_profile_completion(&user),
        "attractiveness_score": user.attractiveness_score,
        "is_verified": user.is_verified,
        "is_student_verified": user.is_student_verified,
        "preferences": preferences.map(|pref| json!({
            "min_age": pref.min_age,
            "max_age": pref.max_age,
            "preferred_genders": pref.preferred_genders,
            "max_distance_km": pref.max_distance,
            "only_verified": pref.only_verified,
            "only_students": pref.only_students,
            "preferred_locations": pref.preferred_locations,
        })),
        "location_data": location.map(|loc| json!({
            "latitude": loc.latitude,
            "longitude": loc.longitude,
            "city": loc.city,
            "state": loc.state,
            "country": loc.country,
            "neighborhood": loc.neighborhood,
            "is_fuzzy": loc.is_fuzzy,
            "show_exact_distance": loc.show_exact_distance,
            "last_updated": loc.last_updated.map(format_datetime),
        })),
        "subscriptions": subscriptions.into_iter().map(|sub| json!({
            "id": sub.id,
            "subscription_type": sub.subscription_type,
            "start_date": sub.start_date.map(format_datetime),
            "end_date": sub.end_date.map(format_datetime),
            "status": sub.status,
        })).collect::<Vec<Value>>(),
        "spots": spots.into_iter().map(|spot| json!({
            "id": spot.id,
            "title": spot.title,
            "poster_url": spot.poster_url,
            "renditions": spot.renditions,
            "expires_at": spot.expires_at.map(format_datetime),
            "created_at": spot.created_at.map(format_datetime),
            "is_global": spot.is_global,
            "city": spot.city,
            "tags": spot.tags,
        })).collect::<Vec<Value>>(),
    });

    Ok(Json(profile))
}

// ============================================================================
// Preferences
// ============================================================================

pub async fn update_preferences(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdatePreferencesRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Validate age range
    if let (Some(min), Some(max)) = (payload.min_age, payload.max_age) {
        if min < 18 {
            return Err(AppError::bad_request("Minimum age must be at least 18"));
        }
        if max < min {
            return Err(AppError::bad_request("Maximum age must be greater than minimum age"));
        }
    }

    let preferred_genders = payload.preferred_genders.map(|g| json!(g));
    let preferred_locations = payload.preferred_locations.map(|l| json!(l));

    let result = sqlx::query(
        r#"
        INSERT INTO user_preferences (user_id, min_age, max_age, preferred_genders, max_distance,
                                       only_verified, only_students, intent, preferred_locations, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW())
        ON CONFLICT (user_id) DO UPDATE SET
            min_age = COALESCE($2, user_preferences.min_age),
            max_age = COALESCE($3, user_preferences.max_age),
            preferred_genders = COALESCE($4, user_preferences.preferred_genders),
            max_distance = COALESCE($5, user_preferences.max_distance),
            only_verified = COALESCE($6, user_preferences.only_verified),
            only_students = COALESCE($7, user_preferences.only_students),
            intent = COALESCE($8, user_preferences.intent),
            preferred_locations = COALESCE($9, user_preferences.preferred_locations),
            updated_at = NOW()
        "#,
    )
    .bind(user_id)
    .bind(payload.min_age)
    .bind(payload.max_age)
    .bind(preferred_genders)
    .bind(payload.max_distance_km)
    .bind(payload.only_verified)
    .bind(payload.only_students)
    .bind(payload.intent)
    .bind(preferred_locations)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::internal("Failed to update preferences"));
    }

    Ok(Json(json!({ "message": "Preferences updated successfully" })))
}

// ============================================================================
// Discovery & Matching
// ============================================================================

pub async fn discover(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    state.metrics.inc_discover_requests();

    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(state.config.discover_limit);

    // Check Redis cache first
    let cache_key = user_id.to_string();
    if let Some(redis) = state.redis_service() {
        if let Ok(Some(cached)) = redis
            .cache_get::<Value>(crate::redis_service::keys::DISCOVERY_CACHE, &cache_key)
            .await
        {
            tracing::debug!(user_id, "Discover cache hit");
            return Ok(Json(cached));
        }
    }

    // Get user and preferences (LRU cache → DB fallback)
    let read_db = state.read_pool();
    let _user = fetch_user_by_id(read_db, user_id)
        .await?
        .ok_or_else(|| AppError::not_found("User not found"))?;

    // Preferences: check in-memory LRU first (5 min TTL)
    let (min_age, max_age, only_verified, max_distance) = {
        let cache = state.preferences_cache.read().await;
        if let Some(cp) = cache.peek(&user_id) {
            if crate::state::cache_fresh(cp.cached_at, crate::state::PREFS_CACHE_TTL) {
                (cp.min_age.unwrap_or(18), cp.max_age.unwrap_or(100),
                 cp.only_verified.unwrap_or(false), cp.max_distance.unwrap_or(state.config.default_max_distance_km))
            } else { (0, 0, false, 0) } // stale → will refetch below
        } else { (0, 0, false, 0) }
    };
    let (min_age, max_age, only_verified, max_distance) = if min_age == 0 {
        let prefs = fetch_user_preferences(read_db, user_id).await?;
        let cp = crate::state::CachedPreferences {
            min_age: prefs.as_ref().and_then(|p| p.min_age),
            max_age: prefs.as_ref().and_then(|p| p.max_age),
            max_distance: prefs.as_ref().and_then(|p| p.max_distance),
            only_verified: prefs.as_ref().and_then(|p| p.only_verified),
            only_students: prefs.as_ref().and_then(|p| p.only_students),
            cached_at: std::time::Instant::now(),
        };
        let result = (cp.min_age.unwrap_or(18), cp.max_age.unwrap_or(100),
                      cp.only_verified.unwrap_or(false), cp.max_distance.unwrap_or(state.config.default_max_distance_km));
        state.preferences_cache.write().await.put(user_id, cp);
        result
    } else { (min_age, max_age, only_verified, max_distance) };

    // Location: check in-memory LRU first (5 min TTL)
    let user_loc = {
        let cache = state.location_cache.read().await;
        if let Some(cl) = cache.peek(&user_id) {
            if crate::state::cache_fresh(cl.cached_at, crate::state::LOCATION_CACHE_TTL) {
                Some(cl.clone())
            } else { None }
        } else { None }
    };
    let user_loc = if let Some(loc) = user_loc {
        Some(loc)
    } else {
        let loc_row = fetch_user_location(read_db, user_id).await?;
        if let Some(ref lr) = loc_row {
            let cl = crate::state::CachedLocation {
                latitude: lr.latitude,
                longitude: lr.longitude,
                city: lr.city.clone(),
                cached_at: std::time::Instant::now(),
            };
            state.location_cache.write().await.put(user_id, cl);
        }
        loc_row.map(|lr| crate::state::CachedLocation {
            latitude: lr.latitude, longitude: lr.longitude,
            city: lr.city, cached_at: std::time::Instant::now(),
        })
    };

    // Get users who haven't been liked/passed by this user
    let candidates = sqlx::query_as::<_, DiscoverUserRow>(
        r#"
        SELECT u.id, u.name, u.display_name, u.show_verified_name,
               u.dob, u.gender, u.bio, u.profile_photo_url, u.profile_photos,
               u.profile_photo_1, u.profile_photo_2, u.profile_photo_3, u.is_verified,
               u.attractiveness_score, u.looking_for, u.profession_title, u.height_cm,
               l.city, l.latitude, l.longitude
        FROM users u
        LEFT JOIN user_locations l ON l.user_id = u.id
        WHERE u.id != $1
          AND u.is_active = TRUE
          AND u.is_profile_complete = TRUE
          AND ($2 = FALSE OR u.is_verified = TRUE)
          AND u.dob IS NOT NULL
          AND EXTRACT(YEAR FROM AGE(u.dob)) BETWEEN $3 AND $4
          AND NOT EXISTS (
              SELECT 1 FROM matches m
              WHERE (m.user1_id = $1 AND m.user2_id = u.id AND m.user1_liked IS NOT NULL)
                 OR (m.user2_id = $1 AND m.user1_id = u.id AND m.user2_liked IS NOT NULL)
          )
        ORDER BY u.attractiveness_score DESC NULLS LAST, RANDOM()
        LIMIT $5
        "#,
    )
    .bind(user_id)
    .bind(only_verified)
    .bind(min_age)
    .bind(max_age)
    .bind(limit)
    .fetch_all(read_db)
    .await?;

    // RL-based ranking: score candidates and re-sort (graceful degradation)
    let candidate_ids: Vec<i32> = candidates.iter().map(|c| c.id).collect();
    let score_map: std::collections::HashMap<i32, f64> = match tokio::time::timeout(
        std::time::Duration::from_secs(2),
        async {
            let mut ml = state.ml.write().await;
            ml.rank_candidates(&state.db, user_id, &candidate_ids).await
        },
    )
    .await
    {
        Ok(scores) => scores.into_iter().collect(),
        Err(_) => {
            state.metrics.inc_ml_fallback();
            tracing::warn!(user_id, "ML ranking timed out, falling back to attractiveness score");
            std::collections::HashMap::new()
        }
    };

    // Parallel fetch: super-likers + university info (don't wait sequentially)
    let candidate_id_list: Vec<i32> = candidates.iter().map(|c| c.id).collect();
    let (super_likers_result, uni_result) = tokio::join!(
        async {
            sqlx::query_scalar::<_, i64>(
                "SELECT from_user_id FROM swipes WHERE to_user_id = $1 AND action = 'superlike'"
            )
            .bind(user_id as i64)
            .fetch_all(read_db)
            .await
            .unwrap_or_default()
        },
        batch_lookup_university(read_db, &candidate_id_list)
    );
    let super_likers: std::collections::HashSet<i32> = super_likers_result.into_iter().map(|id| id as i32).collect();
    let uni_map = uni_result?;

    let mut profiles: Vec<DiscoverProfile> = candidates
        .into_iter()
        .map(|c| {
            let distance_km = if let (Some(ul), Some(lat), Some(lon)) = (&user_loc, c.latitude, c.longitude) {
                ul.latitude.zip(ul.longitude).map(|(ulat, ulon)| haversine_km(ulat, ulon, lat, lon))
            } else {
                None
            };

            let photos = get_photos_from_row(&c);
            let ml_score = score_map.get(&c.id).copied();
            let uni_info = uni_map.get(&c.id);
            let public_name = public_name_for_viewer(
                c.name.as_deref(), c.display_name.as_deref(), c.show_verified_name,
            );
            DiscoverProfile {
                id: c.id,
                name: public_name,
                display_name: c.display_name,
                age: c.dob.map(calculate_age),
                gender: c.gender,
                bio: c.bio,
                photos,
                is_verified: c.is_verified.unwrap_or(false),
                looking_for: c.looking_for,
                profession_title: c.profession_title,
                height_cm: c.height_cm,
                distance_km,
                distance_text: distance_km.map(format_distance),
                city: c.city,
                compatibility_score: ml_score.or(c.attractiveness_score),
                university: uni_info.map(|(name, _)| name.clone()),
                university_tier: uni_info.map(|(_, tier)| format_tier(tier)),
                interaction_status: None,
                super_liked_you: None, // populated below
            }
        })
        .filter(|p| {
            // Filter by distance if user has location
            if let Some(d) = p.distance_km {
                d <= max_distance as f64
            } else {
                true
            }
        })
        .collect();

    // Tag profiles that have super-liked this user
    for profile in &mut profiles {
        if super_likers.contains(&profile.id) {
            profile.super_liked_you = Some(true);
        }
    }

    // Sort by ML score (descending) — super-likers already boosted by ML
    profiles.sort_by(|a, b| {
        b.compatibility_score
            .unwrap_or(0.0)
            .partial_cmp(&a.compatibility_score.unwrap_or(0.0))
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // -------------------------------------------------------------------
    // Shadow scoring v1 — compute + log only, do NOT reorder production.
    // Contract: log top-50 by current_rank ∪ top-50 by shadow_rank.
    // -------------------------------------------------------------------
    let request_id = Uuid::new_v4();
    let candidate_pool_size = profiles.len() as i32;
    let viewer_primary_city = user_loc.as_ref().and_then(|l| l.city.clone());

    let viewer_behavior: Option<(Option<i16>, Option<f64>, Option<String>, Option<i32>)> =
        sqlx::query_as("SELECT peak_hour_utc, sessions_per_day_7d, primary_city, city_change_count_30d FROM user_behavior_profile WHERE user_id = $1")
        .bind(user_id).fetch_optional(read_db).await.ok().flatten();
    let viewer_is_traveler = viewer_behavior.as_ref().and_then(|(_, _, _, ccc)| *ccc).unwrap_or(0) >= 2;

    let cand_ids: Vec<i32> = profiles.iter().map(|p| p.id).collect();
    let cand_behaviors: std::collections::HashMap<i32, (Option<i16>, Option<f64>, Option<String>, Option<i32>)> = if !cand_ids.is_empty() {
        let placeholders: Vec<String> = (1..=cand_ids.len()).map(|i| format!("${}", i)).collect();
        let q = format!(
            "SELECT user_id, peak_hour_utc, sessions_per_day_7d, primary_city, city_change_count_30d FROM user_behavior_profile WHERE user_id IN ({})",
            placeholders.join(",")
        );
        let mut query = sqlx::query_as::<_, (i32, Option<i16>, Option<f64>, Option<String>, Option<i32>)>(&q);
        for id in &cand_ids { query = query.bind(id); }
        query.fetch_all(read_db).await.unwrap_or_default()
            .into_iter().map(|(id, h, s, c, t)| (id, (h, s, c, t))).collect()
    } else { std::collections::HashMap::new() };

    let fof_counts: std::collections::HashMap<i32, i32> = if !cand_ids.is_empty() {
        let placeholders: Vec<String> = (1..=cand_ids.len()).map(|i| format!("${}", i + 1)).collect();
        let q = format!(
            r#"SELECT g2.to_id::int AS candidate_id, COUNT(DISTINCT g1.to_id)::int AS fof
               FROM graph_edge_links_fwd g1
               JOIN graph_edge_links_fwd g2 ON g2.from_id = g1.to_id AND g2.edge_type = 'matched_with'
               WHERE g1.from_id = $1::text AND g1.edge_type = 'matched_with'
                 AND g2.to_id::int IN ({}) AND g2.to_id != $1::text
               GROUP BY g2.to_id"#,
            placeholders.join(",")
        );
        let mut query = sqlx::query_as::<_, (i32, i32)>(&q).bind(user_id.to_string());
        for id in &cand_ids { query = query.bind(id); }
        query.fetch_all(read_db).await.unwrap_or_default().into_iter().collect()
    } else { std::collections::HashMap::new() };

    let stale_location_viewer = user_loc.is_none();
    let viewer_peak = viewer_behavior.as_ref().and_then(|(h, _, _, _)| *h);
    let viewer_spd = viewer_behavior.as_ref().and_then(|(_, s, _, _)| *s).unwrap_or(0.0);
    let viewer_profile_missing = viewer_behavior.is_none();

    // Music signal: batch-fetch shared genre counts (same pattern as GraphQL resolver).
    let viewer_genre_count: i32 = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)::bigint FROM user_genre_profile WHERE user_id = $1"
    ).bind(user_id).fetch_one(read_db).await.unwrap_or(0) as i32;

    let shared_genre_counts: std::collections::HashMap<i32, i32> = if viewer_genre_count > 0 && !cand_ids.is_empty() {
        let cand_i64s: Vec<i64> = cand_ids.iter().map(|&i| i as i64).collect();
        sqlx::query_as::<_, (i64, i64)>(
            r#"SELECT b.user_id, COUNT(*)::bigint
               FROM user_genre_profile a
               JOIN user_genre_profile b ON a.genre = b.genre
               WHERE a.user_id = $1 AND b.user_id = ANY($2)
               GROUP BY b.user_id"#,
        )
        .bind(user_id as i64)
        .bind(&cand_i64s)
        .fetch_all(read_db)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|(cid, cnt)| (cid as i32, cnt as i32))
        .collect()
    } else { std::collections::HashMap::new() };

    let music_missing_viewer = viewer_genre_count <= 0;

    use crate::services::shadow_scoring::{self as ss, GraphFeatures, BehaviorFeatures, LocationFeatures, MusicFeatures};
    let mut rows: Vec<(i32, f64, f64, ss::ShadowComponents, GraphFeatures, BehaviorFeatures, LocationFeatures, MusicFeatures, f64, f64)> = Vec::with_capacity(profiles.len());
    for p in &profiles {
        let base = p.compatibility_score.unwrap_or(0.0);
        let base_norm = if base > 1.5 { base / 100.0 } else { base };

        let gfeat = GraphFeatures {
            fof_count: *fof_counts.get(&p.id).unwrap_or(&0),
            mutual_like_neighbors: 0,
            missing: false,
        };

        let cand = cand_behaviors.get(&p.id);
        let cand_missing = cand.is_none();
        let cand_peak = cand.and_then(|(h, _, _, _)| *h);
        let cand_spd = cand.and_then(|(_, s, _, _)| *s).unwrap_or(0.0);
        let cand_city = cand.and_then(|(_, _, c, _)| c.clone());
        let cand_traveler = cand.and_then(|(_, _, _, t)| *t).unwrap_or(0) >= 2;

        let peak_gap = match (viewer_peak, cand_peak) {
            (Some(a), Some(b)) => ss::circular_hour_gap(a, b),
            _ => 12.0,
        };
        let activity_gap = (viewer_spd - cand_spd).abs();
        let bfeat = BehaviorFeatures {
            peak_hour_gap: peak_gap,
            activity_level_gap: activity_gap,
            missing: viewer_profile_missing || cand_missing,
        };

        let same_city = match (&viewer_primary_city, &cand_city) {
            (Some(v), Some(c)) => v.eq_ignore_ascii_case(c),
            _ => false,
        };
        let lfeat = LocationFeatures {
            same_city,
            viewer_is_traveler,
            candidate_is_traveler: cand_traveler,
            stale: stale_location_viewer,
        };

        let shared = *shared_genre_counts.get(&p.id).unwrap_or(&0);
        let mfeat = MusicFeatures {
            shared_genre_count: shared,
            viewer_genre_count,
            missing: music_missing_viewer,
        };

        let components = ss::compute(base_norm, gfeat, bfeat, lfeat, mfeat);
        rows.push((p.id, base_norm, base, components, gfeat, bfeat, lfeat, mfeat, peak_gap, activity_gap));
    }

    let mut current_rank_map: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();
    for (idx, p) in profiles.iter().enumerate() {
        current_rank_map.insert(p.id, idx as i32);
    }

    let mut shadow_order: Vec<(i32, f64)> = rows.iter().map(|r| (r.0, r.3.shadow_score)).collect();
    shadow_order.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    let mut shadow_rank_map: std::collections::HashMap<i32, i32> = std::collections::HashMap::new();
    for (idx, (cid, _)) in shadow_order.iter().enumerate() {
        shadow_rank_map.insert(*cid, idx as i32);
    }

    let to_log: std::collections::HashSet<i32> = current_rank_map.iter()
        .filter(|(_, r)| **r < 50).map(|(id, _)| *id)
        .chain(shadow_rank_map.iter().filter(|(_, r)| **r < 50).map(|(id, _)| *id))
        .collect();
    let shown_ids: std::collections::HashSet<i32> = profiles.iter().take(limit as usize).map(|p| p.id).collect();

    {
        let db = state.db.clone();
        let req_id = request_id;
        let rows_to_log: Vec<_> = rows.iter().filter(|r| to_log.contains(&r.0)).cloned().collect();
        let current_ranks = current_rank_map.clone();
        let shadow_ranks = shadow_rank_map.clone();
        tokio::spawn(async move {
            for (cid, base_norm, base_raw, comps, g, b, l, m, peak_gap, activity_gap) in rows_to_log {
                let was_shown = shown_ids.contains(&cid);
                let c_rank = current_ranks.get(&cid).copied();
                let s_rank = shadow_ranks.get(&cid).copied();
                let _ = sqlx::query(
                    r#"INSERT INTO discover_feature_log
                       (request_id, viewer_user_id, candidate_user_id, was_shown, current_rank, shadow_rank,
                        current_score, shadow_score, base_score, category_score, attractiveness_score,
                        graph_score, behavior_score, location_score, music_score,
                        fof_count, mutual_like_neighbors, same_city, viewer_is_traveler, candidate_is_traveler,
                        peak_hour_gap, activity_level_gap, music_shared_genres,
                        behavior_profile_missing, graph_features_missing, stale_location, music_features_missing,
                        candidate_pool_size, scoring_version, shadow_version)
                       VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,$25,$26,$27,$28,'v1','shadow_v1')"#
                )
                .bind(req_id).bind(user_id).bind(cid).bind(was_shown).bind(c_rank).bind(s_rank)
                .bind(base_norm).bind(comps.shadow_score).bind(base_norm)
                .bind(None::<f64>).bind(Some(base_raw))
                .bind(comps.graph_score).bind(comps.behavior_score).bind(comps.location_score).bind(comps.music_score)
                .bind(g.fof_count).bind(g.mutual_like_neighbors)
                .bind(l.same_city).bind(l.viewer_is_traveler).bind(l.candidate_is_traveler)
                .bind(peak_gap).bind(activity_gap).bind(m.shared_genre_count)
                .bind(b.missing).bind(g.missing).bind(l.stale).bind(m.missing)
                .bind(candidate_pool_size)
                .execute(&db).await;
            }
        });
    }

    // Log impression events for ML (fire-and-forget, don't block response)
    let slate_id = Uuid::new_v4().to_string();
    let db_clone = state.db.clone();
    let slate_id_clone = slate_id.clone();
    let profile_ids: Vec<(usize, i32)> = profiles.iter().enumerate().map(|(rank, p)| (rank, p.id)).collect();
    tokio::spawn(async move {
        for (rank, profile_id) in profile_ids {
            let _ = log_interaction_event(
                &db_clone,
                user_id,
                profile_id,
                "impression",
                Some(&slate_id_clone),
                Some(rank as i32),
                Some("discover"),
            )
            .await;
        }
    });

    let response = json!({
        "profiles": profiles,
        "slate_id": slate_id,
        "count": profiles.len(),
        "prefetch_at": (profiles.len() as f64 * 0.7).ceil() as usize,
        "has_more": profiles.len() as i32 >= limit,
    });

    // Cache the result in Redis (120s TTL, fail gracefully)
    if let Some(redis) = state.redis_service() {
        if let Err(e) = redis
            .cache_set(crate::redis_service::keys::DISCOVERY_CACHE, &cache_key, &response, 120)
            .await
        {
            tracing::warn!(user_id, error = %e, "Failed to cache discover results");
        }
    }

    Ok(Json(response))
}

pub async fn like_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LikeRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let target_id = payload.target_user_id;
    state.metrics.inc_swipe_writes();

    if user_id == target_id {
        return Err(AppError::bad_request("Cannot like yourself"));
    }

    // Check if target exists
    let target_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND is_active = TRUE)",
    )
    .bind(target_id)
    .fetch_one(&state.db)
    .await?;

    if !target_exists {
        return Err(AppError::not_found("User not found"));
    }

    // Delegate to swipe_service (shared with GraphQL)
    let outcome = crate::services::swipe_service::execute_like(
        &state.db, user_id, target_id, "discover"
    ).await?;
    let match_id = outcome.match_id;
    let is_mutual = outcome.is_mutual;

    // Store message request if provided
    let message_text = payload.message.as_deref()
        .map(|m| m.trim())
        .filter(|m| !m.is_empty())
        .map(|m| &m[..m.len().min(300)]);

    if let Some(msg) = message_text {
        let like_msg_id = sqlx::query_scalar::<_, i64>(
            r#"INSERT INTO messages (match_id, sender_id, receiver_id, content, message_type, created_at)
               VALUES ($1, $2, $3, $4, 'like_message', NOW())
               ON CONFLICT DO NOTHING
               RETURNING id"#,
        )
        .bind(&match_id)
        .bind(user_id)
        .bind(target_id)
        .bind(msg)
        .fetch_optional(&state.db)
        .await
        .ok()
        .flatten();

        if let Some(mid) = like_msg_id {
            auto_queue_for_labeling(state.db.clone(), state.config.llm_enabled, "message", mid, 5);
        }
    }

    // Log like_with_message if message was attached (service already logged 'like')
    if message_text.is_some() {
        let _ = log_interaction_event(&state.db, user_id, target_id, "like_with_message", None, None, Some("discover")).await;
    }

    // Feed RL agent with like signal (non-blocking, never fails the request)
    let ml = state.ml.clone();
    let db = state.db.clone();
    tokio::spawn(async move {
        let mut ml = ml.write().await;
        ml.record_swipe(&db, user_id, target_id, true).await;
        if is_mutual {
            ml.record_swipe(&db, target_id, user_id, true).await;
        }
    });

    Ok(Json(json!({
        "message": if is_mutual { "It's a match!" } else { "Like sent" },
        "match_id": match_id,
        "is_mutual": is_mutual,
        "has_message": message_text.is_some(),
    })))
}

/// Super Like — premium action that notifies the target and stands out.
/// Deducts from user's super_like consumable balance (or checks subscription tier).
pub async fn super_like_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LikeRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let target_id = payload.target_user_id;
    state.metrics.inc_swipe_writes();

    if user_id == target_id {
        return Err(AppError::bad_request("Cannot super like yourself"));
    }

    // Check if target exists
    let target_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM users WHERE id = $1 AND is_active = TRUE)",
    )
    .bind(target_id)
    .fetch_one(&state.db)
    .await?;

    if !target_exists {
        return Err(AppError::not_found("User not found"));
    }

    // --- Check & deduct super like balance ---
    // 1. Check if user has unlimited super likes (Platinum subscription)
    let has_unlimited = sqlx::query_scalar::<_, bool>(r#"
        SELECT EXISTS(
            SELECT 1 FROM user_subscriptions us
            JOIN products p ON us.product_id = p.id
            WHERE us.user_id = $1 AND us.status = 'active'
            AND p.features::text LIKE '%unlimited_super_likes%'
        )
    "#)
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(false);

    if !has_unlimited {
        // 2. Check consumable balance
        let balance = sqlx::query_scalar::<_, i32>(
            "SELECT COALESCE(balance, 0) FROM user_consumables WHERE user_id = $1 AND consumable_type = 'super_like'"
        )
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?
        .unwrap_or(0);

        // 3. Check daily allocation from Gold subscription (5 daily)
        let has_daily = sqlx::query_scalar::<_, bool>(r#"
            SELECT EXISTS(
                SELECT 1 FROM user_subscriptions us
                JOIN products p ON us.product_id = p.id
                WHERE us.user_id = $1 AND us.status = 'active'
                AND p.features::text LIKE '%5_super_likes_daily%'
            )
        "#)
        .bind(user_id)
        .fetch_one(&state.db)
        .await
        .unwrap_or(false);

        if has_daily {
            // Count super likes used today
            let used_today = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*) FROM swipes WHERE from_user_id = $1 AND action = 'superlike' AND created_at >= CURRENT_DATE"
            )
            .bind(user_id as i64)
            .fetch_one(&state.db)
            .await
            .unwrap_or(0);

            if used_today >= 5 && balance <= 0 {
                return Err(AppError::bad_request("Daily super like limit reached (5/day). Purchase more or upgrade to Platinum."));
            }
            // If daily allocation available, no deduction needed from consumables
            if used_today < 5 {
                // Using daily allocation — no balance deduction
            } else {
                // Daily used up, deduct from purchased balance
                sqlx::query("UPDATE user_consumables SET balance = balance - 1, total_used = total_used + 1, updated_at = NOW() WHERE user_id = $1 AND consumable_type = 'super_like'")
                    .bind(user_id)
                    .execute(&state.db)
                    .await?;
            }
        } else if balance > 0 {
            // Free/no subscription — deduct from purchased balance
            sqlx::query("UPDATE user_consumables SET balance = balance - 1, total_used = total_used + 1, updated_at = NOW() WHERE user_id = $1 AND consumable_type = 'super_like'")
                .bind(user_id)
                .execute(&state.db)
                .await?;
        } else {
            return Err(AppError::bad_request("No super likes remaining. Purchase a super like pack or subscribe to Gold/Platinum."));
        }
    }

    // --- Record the super like swipe ---
    sqlx::query(
        "INSERT INTO swipes (from_user_id, to_user_id, action, source) VALUES ($1, $2, 'superlike', 'discover') ON CONFLICT (from_user_id, to_user_id) DO UPDATE SET action = 'superlike', created_at = NOW()"
    )
    .bind(user_id as i64)
    .bind(target_id as i64)
    .execute(&state.db)
    .await?;

    // --- Create/update match record (same as like_user) ---
    let (user1_id, user2_id, is_user1) = if user_id < target_id {
        (user_id, target_id, true)
    } else {
        (target_id, user_id, false)
    };

    let existing = sqlx::query_as::<_, MatchCheckRow>(
        "SELECT id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match FROM matches WHERE user1_id = $1 AND user2_id = $2",
    )
    .bind(user1_id)
    .bind(user2_id)
    .fetch_optional(&state.db)
    .await?;

    let (match_id, is_mutual) = match existing {
        Some(m) => {
            let other_liked = if is_user1 { m.user2_liked } else { m.user1_liked };
            let is_mutual = other_liked.unwrap_or(false);

            let query = if is_user1 {
                "UPDATE matches SET user1_liked = TRUE, is_mutual_match = $1, updated_at = NOW() WHERE id = $2"
            } else {
                "UPDATE matches SET user2_liked = TRUE, is_mutual_match = $1, updated_at = NOW() WHERE id = $2"
            };

            sqlx::query(query)
                .bind(is_mutual)
                .bind(&m.id)
                .execute(&state.db)
                .await?;

            (m.id, is_mutual)
        }
        None => {
            let new_id = Uuid::new_v4().to_string();
            let (u1_liked, u2_liked) = if is_user1 { (true, false) } else { (false, true) };

            sqlx::query(
                r#"INSERT INTO matches (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, status, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, FALSE, 'active', NOW(), NOW())"#,
            )
            .bind(&new_id)
            .bind(user1_id)
            .bind(user2_id)
            .bind(u1_liked)
            .bind(u2_liked)
            .execute(&state.db)
            .await?;

            (new_id, false)
        }
    };

    // Log interaction
    let _ = log_interaction_event(&state.db, user_id, target_id, "superlike", None, None, Some("discover")).await;

    // Publish event (fires notification to target)
    state.event_bus.publish("swipe_handler", crate::modules::events::DomainEvent::SwipeSuperLike {
        user_id,
        target_user_id: target_id,
    });

    // Feed RL agent with 3× weighted super-like signal
    let ml = state.ml.clone();
    let db = state.db.clone();
    tokio::spawn(async move {
        let mut ml = ml.write().await;
        ml.record_swipe_weighted(&db, user_id, target_id, true, true).await;
    });

    Ok(Json(json!({
        "message": if is_mutual { "It's a match! (Super Like)" } else { "Super Like sent!" },
        "match_id": match_id,
        "is_mutual": is_mutual,
        "is_super_like": true,
    })))
}

pub async fn pass_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LikeRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let target_id = payload.target_user_id;
    state.metrics.inc_swipe_writes();

    // Log pass event (for ML training - negative signal)
    let _ = log_interaction_event(&state.db, user_id, target_id, "pass", None, None, Some("discover")).await;

    // Feed RL agent with pass signal (non-blocking)
    let ml = state.ml.clone();
    let db = state.db.clone();
    tokio::spawn(async move {
        let mut ml = ml.write().await;
        ml.record_swipe(&db, user_id, target_id, false).await;
    });

    // Delegate to swipe_service (shared with GraphQL)
    crate::services::swipe_service::execute_pass(&state.db, user_id, target_id, "discover").await?;

    Ok(Json(json!({ "message": "Passed" })))
}

pub async fn get_match(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(match_id): AxumPath<String>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Get match and verify user is part of it
    let m = sqlx::query_as::<_, MatchRow>(
        r#"
        SELECT id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match,
               ai_compatibility_score, visual_compatibility_score, match_reason,
               messages_count, voice_messages_count, last_message_at, can_send_text,
               status, blocked_by_user_id, created_at, updated_at
        FROM matches
        WHERE id = $1 AND (user1_id = $2 OR user2_id = $2)
        "#,
    )
    .bind(&match_id)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("Match not found"))?;

    // Get the other user's profile
    let other_id = if m.user1_id == user_id { m.user2_id } else { m.user1_id };
    let other_user = fetch_user_by_id(&state.db, other_id)
        .await?
        .ok_or_else(|| AppError::not_found("Matched user not found"))?;

    let other_location = fetch_user_location(&state.db, other_id).await?;
    let my_location = fetch_user_location(&state.db, user_id).await?;

    let distance_km = if let (Some(ml), Some(ol)) = (&my_location, &other_location) {
        ml.latitude
            .zip(ml.longitude)
            .zip(ol.latitude.zip(ol.longitude))
            .map(|((lat1, lon1), (lat2, lon2))| haversine_km(lat1, lon1, lat2, lon2))
    } else {
        None
    };

    // Get photos before moving other fields
    let photos = get_user_photos(&other_user);

    // Lookup university info for matched user
    let uni_map = batch_lookup_university(&state.db, &[other_id]).await?;
    let uni_info = uni_map.get(&other_id);

    let public_name = public_name_for_viewer(
        other_user.name.as_deref(),
        other_user.display_name.as_deref(),
        other_user.show_verified_name,
    );
    let profile = DiscoverProfile {
        id: other_user.id,
        name: public_name,
        display_name: other_user.display_name.clone(),
        age: other_user.dob.map(calculate_age),
        gender: other_user.gender,
        bio: other_user.bio,
        photos,
        is_verified: other_user.is_verified.unwrap_or(false),
        looking_for: other_user.looking_for,
        profession_title: other_user.profession_title,
        height_cm: other_user.height_cm,
        distance_km,
        distance_text: distance_km.map(format_distance),
        city: other_location.and_then(|l| l.city),
        compatibility_score: m.ai_compatibility_score,
        university: uni_info.map(|(name, _)| name.clone()),
        university_tier: uni_info.map(|(_, tier)| format_tier(tier)),
        interaction_status: Some("matched".to_string()),
        super_liked_you: None,
    };

    let detail = MatchDetail {
        match_id: m.id,
        is_mutual: m.is_mutual_match.unwrap_or(false),
        matched_at: m.created_at.map(format_datetime),
        can_send_text: m.can_send_text.unwrap_or(false),
        messages_count: m.messages_count.unwrap_or(0),
        voice_messages_count: m.voice_messages_count.unwrap_or(0),
        other_user: profile,
    };

    Ok(Json(json!(detail)))
}

pub async fn get_matches(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let limit: i64 = params
        .get("limit")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(30)
        .min(100)
        .max(1);

    let before: Option<NaiveDateTime> = params
        .get("before")
        .and_then(|v| NaiveDateTime::parse_from_str(v, "%Y-%m-%dT%H:%M:%S%.f").ok()
            .or_else(|| NaiveDateTime::parse_from_str(v, "%Y-%m-%dT%H:%M:%S").ok()));

    let read_db = state.read_pool();
    let matches = sqlx::query_as::<_, MatchRow>(
        r#"
        SELECT id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match,
               ai_compatibility_score, visual_compatibility_score, match_reason,
               messages_count, voice_messages_count, last_message_at, can_send_text,
               status, blocked_by_user_id, created_at, updated_at
        FROM matches
        WHERE (user1_id = $1 OR user2_id = $1)
          AND is_mutual_match = TRUE
          AND status = 'active'
          AND ($2::timestamp IS NULL OR last_message_at < $2)
        ORDER BY last_message_at DESC NULLS LAST, created_at DESC
        LIMIT $3
        "#,
    )
    .bind(user_id)
    .bind(before)
    .bind(limit + 1)
    .fetch_all(read_db)
    .await?;

    let has_more = matches.len() as i64 > limit;
    let matches_page: Vec<_> = matches.into_iter().take(limit as usize).collect();

    let mut results = Vec::new();
    for m in &matches_page {
        let other_id = if m.user1_id == user_id { m.user2_id } else { m.user1_id };
        if let Some(other_user) = fetch_user_by_id(read_db, other_id).await? {
            results.push(json!({
                "match_id": m.id,
                "is_mutual": true,
                "matched_at": m.created_at.map(format_datetime),
                "can_send_text": m.can_send_text.unwrap_or(false),
                "messages_count": m.messages_count.unwrap_or(0),
                "voice_messages_count": m.voice_messages_count.unwrap_or(0),
                "last_message_at": m.last_message_at.map(format_datetime),
                "other_user": {
                    "id": other_user.id,
                    "name": other_user.name,
                    "photos": get_user_photos(&other_user),
                    "is_verified": other_user.is_verified.unwrap_or(false),
                }
            }));
        }
    }

    let next_cursor = if has_more {
        matches_page.last().and_then(|m| m.last_message_at.map(|dt| dt.format("%Y-%m-%dT%H:%M:%S%.f").to_string()))
    } else {
        None
    };

    Ok(Json(json!({
        "matches": results,
        "next_cursor": next_cursor,
        "has_more": has_more
    })))
}

// ============================================================================
// Location
// ============================================================================

pub async fn update_location(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LocationUpdateRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Validate coordinates
    if payload.latitude < -90.0 || payload.latitude > 90.0 {
        return Err(AppError::bad_request("Invalid latitude"));
    }
    if payload.longitude < -180.0 || payload.longitude > 180.0 {
        return Err(AppError::bad_request("Invalid longitude"));
    }

    let result = sqlx::query(
        r#"
        INSERT INTO user_locations (user_id, latitude, longitude, accuracy, city, state, country, last_updated, update_source)
        VALUES ($1, $2, $3, $4, $5, $6, $7, NOW(), 'manual')
        ON CONFLICT (user_id) DO UPDATE SET
            latitude = $2,
            longitude = $3,
            accuracy = COALESCE($4, user_locations.accuracy),
            city = COALESCE($5, user_locations.city),
            state = COALESCE($6, user_locations.state),
            country = COALESCE($7, user_locations.country),
            last_updated = NOW(),
            update_source = 'manual'
        "#,
    )
    .bind(user_id)
    .bind(payload.latitude)
    .bind(payload.longitude)
    .bind(payload.accuracy)
    .bind(&payload.city)
    .bind(&payload.state)
    .bind(&payload.country)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        return Err(AppError::internal("Failed to update location"));
    }

    Ok(Json(json!({ "message": "Location updated successfully" })))
}

pub async fn get_my_location(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let location = fetch_user_location(&state.db, user_id).await?;

    match location {
        Some(loc) => Ok(Json(json!({
            "latitude": loc.latitude,
            "longitude": loc.longitude,
            "city": loc.city,
            "state": loc.state,
            "country": loc.country,
            "neighborhood": loc.neighborhood,
            "last_updated": loc.last_updated
        }))),
        None => Ok(Json(json!({
            "message": "Location not set"
        }))),
    }
}

// Location search for autocomplete
pub async fn search_locations(
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let query = params
        .get("q")
        .map(|s| s.trim().to_lowercase())
        .filter(|s| s.len() >= 2)
        .ok_or_else(|| AppError::bad_request("Query parameter 'q' is required (min 2 chars)"))?;

    // Common Indian cities for the Telugu dating app
    let indian_cities = vec![
        "Hyderabad, Telangana",
        "Secunderabad, Telangana",
        "Warangal, Telangana",
        "Nizamabad, Telangana",
        "Karimnagar, Telangana",
        "Khammam, Telangana",
        "Ramagundam, Telangana",
        "Vijayawada, Andhra Pradesh",
        "Visakhapatnam, Andhra Pradesh",
        "Guntur, Andhra Pradesh",
        "Nellore, Andhra Pradesh",
        "Tirupati, Andhra Pradesh",
        "Kakinada, Andhra Pradesh",
        "Rajahmundry, Andhra Pradesh",
        "Kurnool, Andhra Pradesh",
        "Anantapur, Andhra Pradesh",
        "Kadapa, Andhra Pradesh",
        "Bangalore, Karnataka",
        "Chennai, Tamil Nadu",
        "Mumbai, Maharashtra",
        "Pune, Maharashtra",
        "Delhi, Delhi",
        "Gurgaon, Haryana",
        "Noida, Uttar Pradesh",
        "Kolkata, West Bengal",
        "Ahmedabad, Gujarat",
        "Jaipur, Rajasthan",
        // US cities for testing
        "San Francisco, California",
        "San Jose, California",
        "Los Angeles, California",
        "New York, New York",
        "Seattle, Washington",
        "Austin, Texas",
        "Dallas, Texas",
        "Houston, Texas",
        "Chicago, Illinois",
        "Boston, Massachusetts",
        "Atlanta, Georgia",
        "Miami, Florida",
        "Denver, Colorado",
        "Phoenix, Arizona",
        "Portland, Oregon",
    ];

    // Filter cities matching the query
    let results: Vec<&str> = indian_cities
        .iter()
        .filter(|city| city.to_lowercase().contains(&query))
        .take(6)
        .copied()
        .collect();

    Ok(Json(json!({
        "results": results
    })))
}

pub async fn get_nearby(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(20);

    // Get user's location and active pass
    let user_loc = fetch_user_location(&state.db, user_id)
        .await?
        .ok_or_else(|| AppError::bad_request("Please update your location first"))?;

    let (user_lat, user_lon) = user_loc
        .latitude
        .zip(user_loc.longitude)
        .ok_or_else(|| AppError::bad_request("Location coordinates not set"))?;

    let active_pass = get_active_pass(&state.db, user_id).await?;
    let pass_type = active_pass
        .as_ref()
        .and_then(|p| p.subscription_type.as_ref())
        .map(|s| PassType::from_str(s))
        .unwrap_or(PassType::Free);

    let max_distance = state.config.default_max_distance_km as f64 + pass_type.enhanced_radius_miles() * 1.60934;

    // Find nearby users with location
    let nearby = sqlx::query_as::<_, DiscoverUserRow>(
        r#"
        SELECT u.id, u.name, u.display_name, u.show_verified_name,
               u.dob, u.gender, u.bio, u.profile_photo_url, u.profile_photos,
               u.profile_photo_1, u.profile_photo_2, u.profile_photo_3, u.is_verified,
               u.attractiveness_score, u.looking_for, u.profession_title, u.height_cm,
               l.city, l.latitude, l.longitude
        FROM users u
        JOIN user_locations l ON l.user_id = u.id
        WHERE u.id != $1
          AND u.is_active = TRUE
          AND u.is_profile_complete = TRUE
          AND l.latitude IS NOT NULL
          AND l.longitude IS NOT NULL
        ORDER BY (
            6371 * acos(
                cos(radians($2)) * cos(radians(l.latitude)) *
                cos(radians(l.longitude) - radians($3)) +
                sin(radians($2)) * sin(radians(l.latitude))
            )
        ) ASC
        LIMIT $4
        "#,
    )
    .bind(user_id)
    .bind(user_lat)
    .bind(user_lon)
    .bind(limit)
    .fetch_all(state.read_pool())
    .await?;

    let can_see_exact = pass_type.can_see_exact_distance();
    let can_see_city = pass_type.can_see_city_names();

    let results: Vec<NearbyMatch> = nearby
        .into_iter()
        .filter_map(|n| {
            let (lat, lon) = n.latitude.zip(n.longitude)?;
            let distance_km = haversine_km(user_lat, user_lon, lat, lon);
            if distance_km > max_distance {
                return None;
            }

            let photos = get_photos_from_row(&n);
            let public_name = public_name_for_viewer(
                n.name.as_deref(), n.display_name.as_deref(), n.show_verified_name,
            );
            Some(NearbyMatch {
                user_id: n.id,
                name: public_name,
                photos,
                distance_km: if can_see_exact { distance_km } else { fuzzy_distance(distance_km) },
                distance_text: if can_see_exact {
                    format_distance(distance_km)
                } else {
                    format_fuzzy_distance(distance_km)
                },
                city: if can_see_city { n.city } else { None },
                is_verified: n.is_verified.unwrap_or(false),
            })
        })
        .collect();

    Ok(Json(json!({
        "nearby": results,
        "pass_type": pass_type.as_str(),
        "enhanced_radius_km": pass_type.enhanced_radius_miles() * 1.60934,
    })))
}

/// POST /location/search-history — saves location search for proprietary hotspot data
pub async fn save_search_history(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let name = payload["name"].as_str().unwrap_or("").to_string();
    if name.is_empty() {
        return Err(AppError::bad_request("Missing 'name' field"));
    }
    let latitude = payload["latitude"].as_f64();
    let longitude = payload["longitude"].as_f64();

    sqlx::query(
        "INSERT INTO location_search_history (user_id, name, latitude, longitude) VALUES ($1, $2, $3, $4)"
    )
    .bind(user_id)
    .bind(&name)
    .bind(latitude)
    .bind(longitude)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({ "ok": true })))
}

pub async fn purchase_pass(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PurchasePassRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let pass_type = PassType::from_str(&payload.pass_type);
    if pass_type == PassType::Free {
        return Err(AppError::bad_request("Cannot purchase free pass"));
    }

    // Idempotency check: If idempotency_key provided, check if already processed
    if let Some(ref idempotency_key) = payload.idempotency_key {
        let existing: Option<(i32, String, String)> = sqlx::query_as(
            r#"
            SELECT id, payment_id, status
            FROM user_subscriptions
            WHERE user_id = $1 AND idempotency_key = $2
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(idempotency_key)
        .fetch_optional(&state.db)
        .await?;

        if let Some((_, payment_id, status)) = existing {
            // Return the existing subscription info (idempotent response)
            return Ok(Json(json!({
                "message": "Pass already purchased (idempotent)",
                "pass_type": pass_type.as_str(),
                "payment_id": payment_id,
                "status": status,
                "idempotent": true,
            })));
        }
    }

    // Check for existing active subscription of the same type
    let existing_active: Option<(i32,)> = sqlx::query_as(
        r#"
        SELECT id FROM user_subscriptions
        WHERE user_id = $1
          AND pass_type = $2
          AND is_active = TRUE
          AND (end_date IS NULL OR end_date > NOW())
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .bind(pass_type.as_str())
    .fetch_optional(&state.db)
    .await?;

    if existing_active.is_some() {
        return Err(AppError::bad_request(
            "You already have an active subscription of this type"
        ));
    }

    let price_cents = pass_type.price_cents(&state.config);

    // Check for student discount
    let student_discount = get_student_discount(&state.db, user_id, &state.config).await?;
    let discount_amount = (price_cents as f64 * student_discount.discount_percent as f64 / 100.0) as i64;
    let final_price = price_cents - discount_amount;

    // Calculate end date
    let start_date = Utc::now().naive_utc();
    let end_date = pass_type.duration_hours().map(|hours| {
        start_date + chrono::Duration::hours(hours)
    });

    // Create subscription record (mock payment - in production integrate with Stripe)
    let payment_id = format!("mock_{}", Uuid::new_v4());

    sqlx::query(
        r#"
        INSERT INTO user_subscriptions (
            user_id, subscription_type, pass_type, status, original_price, amount_paid,
            discount_applied, discount_type, start_date, end_date, payment_id, is_active,
            enhanced_radius, can_see_city_names, unlimited_swipes, priority_visibility,
            idempotency_key, created_at, updated_at
        ) VALUES (
            $1, $2, $2, 'active', $3, $4, $5, $6, $7, $8, $9, TRUE,
            $10, $11, TRUE, TRUE, $12, NOW(), NOW()
        )
        "#,
    )
    .bind(user_id)
    .bind(pass_type.as_str())
    .bind(rust_decimal::Decimal::new(price_cents, 2))
    .bind(rust_decimal::Decimal::new(final_price, 2))
    .bind(rust_decimal::Decimal::new(discount_amount, 2))
    .bind(if student_discount.is_verified { Some("student") } else { None::<&str> })
    .bind(start_date)
    .bind(end_date)
    .bind(&payment_id)
    .bind(pass_type.enhanced_radius_miles())
    .bind(pass_type.can_see_city_names())
    .bind(&payload.idempotency_key)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "message": "Pass purchased successfully",
        "pass_type": pass_type.as_str(),
        "original_price_cents": price_cents,
        "discount_cents": discount_amount,
        "final_price_cents": final_price,
        "start_date": format_datetime(start_date),
        "end_date": end_date.map(format_datetime),
        "payment_id": payment_id,
    })))
}

// ============================================================================
// RevenueCat Webhook & Subscription Sync
// ============================================================================

/// RevenueCat webhook event types
#[derive(Debug, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum RevenueCatEventType {
    InitialPurchase,
    Renewal,
    Cancellation,
    Uncancellation,
    NonRenewingPurchase,
    SubscriptionPaused,
    Expiration,
    BillingIssue,
    ProductChange,
    #[serde(other)]
    Unknown,
}

#[derive(Debug, Deserialize)]
pub struct RevenueCatWebhookEvent {
    #[serde(rename = "type")]
    pub event_type: RevenueCatEventType,
    pub app_user_id: String,
    pub product_id: String,
    pub purchased_at_ms: Option<i64>,
    pub expiration_at_ms: Option<i64>,
    pub store: Option<String>,
    pub environment: Option<String>,
    pub original_transaction_id: Option<String>,
    pub price_in_purchased_currency: Option<f64>,
    pub currency: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct RevenueCatWebhookPayload {
    pub event: RevenueCatWebhookEvent,
    pub api_version: Option<String>,
}

/// Handle RevenueCat webhook events
pub async fn revenuecat_webhook(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RevenueCatWebhookPayload>,
) -> Result<Json<Value>, AppError> {
    // Verify webhook authorization (RevenueCat sends a shared secret in header)
    let auth_header = headers
        .get("Authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("");

    // In production, verify this matches your RevenueCat webhook secret.
    // Constant-time compare so a rejected request's timing doesn't leak how
    // many leading bytes of the secret were guessed correctly.
    let expected_secret = state.config.revenuecat_webhook_secret.as_deref().unwrap_or("");
    if !expected_secret.is_empty() {
        let expected_header = format!("Bearer {}", expected_secret);
        let matches: bool = subtle::ConstantTimeEq::ct_eq(
            auth_header.as_bytes(),
            expected_header.as_bytes(),
        )
        .into();
        if !matches {
            return Err(AppError::unauthorized("Invalid webhook authorization"));
        }
    }

    let event = &payload.event;
    tracing::info!(
        "RevenueCat webhook: {:?} for user {} product {}",
        event.event_type,
        event.app_user_id,
        event.product_id
    );

    // Parse user_id from app_user_id (we set this as the user's numeric ID)
    let user_id: i64 = event.app_user_id.parse().map_err(|_| {
        AppError::bad_request("Invalid app_user_id format")
    })?;

    // Map product_id to pass_type
    let pass_type = product_id_to_pass_type(&event.product_id);

    match event.event_type {
        RevenueCatEventType::InitialPurchase | RevenueCatEventType::NonRenewingPurchase => {
            // New subscription - create record
            create_subscription_from_webhook(&state.db, user_id, event, &pass_type).await?;
        }
        RevenueCatEventType::Renewal => {
            // Subscription renewed - extend or create
            extend_subscription_from_webhook(&state.db, user_id, event, &pass_type).await?;
        }
        RevenueCatEventType::Expiration | RevenueCatEventType::Cancellation => {
            // Mark subscription as expired/cancelled
            deactivate_subscription(&state.db, user_id, &event.product_id).await?;
        }
        RevenueCatEventType::Uncancellation => {
            // User uncancelled - reactivate
            reactivate_subscription(&state.db, user_id, &event.product_id).await?;
        }
        RevenueCatEventType::BillingIssue | RevenueCatEventType::SubscriptionPaused => {
            // Mark subscription as having issues
            mark_subscription_billing_issue(&state.db, user_id, &event.product_id).await?;
        }
        _ => {
            tracing::info!("Unhandled RevenueCat event type: {:?}", event.event_type);
        }
    }

    Ok(Json(json!({ "status": "ok" })))
}

/// Sync subscription from frontend (called after successful purchase)
#[derive(Debug, Deserialize)]
pub struct SyncSubscriptionRequest {
    pub product_id: String,
    pub purchase_date: Option<String>,
    pub expiration_date: Option<String>,
    pub is_active: bool,
    pub store: Option<String>,
    pub original_transaction_id: Option<String>,
}

pub async fn sync_subscription(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SyncSubscriptionRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let pass_type = product_id_to_pass_type(&payload.product_id);

    // Parse dates
    let start_date = payload.purchase_date
        .as_ref()
        .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
        .map(|dt| dt.naive_utc())
        .unwrap_or_else(|| Utc::now().naive_utc());

    let end_date = payload.expiration_date
        .as_ref()
        .and_then(|d| chrono::DateTime::parse_from_rfc3339(d).ok())
        .map(|dt| dt.naive_utc());

    // Upsert subscription
    sqlx::query(
        r#"
        INSERT INTO user_subscriptions (
            user_id, subscription_type, pass_type, status, start_date, end_date,
            payment_id, is_active, store, created_at, updated_at
        ) VALUES (
            $1, $2, $2, 'active', $3, $4, $5, $6, $7, NOW(), NOW()
        )
        ON CONFLICT (user_id, pass_type) WHERE is_active = TRUE
        DO UPDATE SET
            start_date = EXCLUDED.start_date,
            end_date = EXCLUDED.end_date,
            is_active = EXCLUDED.is_active,
            store = EXCLUDED.store,
            updated_at = NOW()
        "#,
    )
    .bind(user_id)
    .bind(pass_type.as_str())
    .bind(start_date)
    .bind(end_date)
    .bind(&payload.original_transaction_id)
    .bind(payload.is_active)
    .bind(&payload.store)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "message": "Subscription synced",
        "pass_type": pass_type.as_str(),
        "is_active": payload.is_active,
    })))
}

// Helper functions for subscription management
fn product_id_to_pass_type(product_id: &str) -> PassType {
    match product_id {
        "nava_boost_1hr" => PassType::Hourly,
        "nava_daily_pass" => PassType::Daily,
        "nava_weekly_sub" => PassType::Weekly,
        "nava_monthly_sub" => PassType::Monthly,
        "nava_ultra_3mo" => PassType::Ultra,
        _ => PassType::Free,
    }
}

async fn create_subscription_from_webhook(
    db: &PgPool,
    user_id: i64,
    event: &RevenueCatWebhookEvent,
    pass_type: &PassType,
) -> Result<(), AppError> {
    let start_date = event.purchased_at_ms
        .map(|ms| chrono::DateTime::from_timestamp_millis(ms).map(|dt| dt.naive_utc()))
        .flatten()
        .unwrap_or_else(|| Utc::now().naive_utc());

    let end_date = event.expiration_at_ms
        .map(|ms| chrono::DateTime::from_timestamp_millis(ms).map(|dt| dt.naive_utc()))
        .flatten();

    let price_cents = event.price_in_purchased_currency
        .map(|p| (p * 100.0) as i64)
        .unwrap_or(0);

    sqlx::query(
        r#"
        INSERT INTO user_subscriptions (
            user_id, subscription_type, pass_type, status, original_price, amount_paid,
            start_date, end_date, payment_id, is_active, store,
            enhanced_radius, can_see_city_names, unlimited_swipes, priority_visibility,
            created_at, updated_at
        ) VALUES (
            $1, $2, $2, 'active', $3, $3, $4, $5, $6, TRUE, $7,
            $8, $9, TRUE, TRUE, NOW(), NOW()
        )
        "#,
    )
    .bind(user_id)
    .bind(pass_type.as_str())
    .bind(rust_decimal::Decimal::new(price_cents, 2))
    .bind(start_date)
    .bind(end_date)
    .bind(&event.original_transaction_id)
    .bind(&event.store)
    .bind(pass_type.enhanced_radius_miles())
    .bind(pass_type.can_see_city_names())
    .execute(db)
    .await?;

    Ok(())
}

async fn extend_subscription_from_webhook(
    db: &PgPool,
    user_id: i64,
    event: &RevenueCatWebhookEvent,
    pass_type: &PassType,
) -> Result<(), AppError> {
    let end_date = event.expiration_at_ms
        .map(|ms| chrono::DateTime::from_timestamp_millis(ms).map(|dt| dt.naive_utc()))
        .flatten();

    // Update existing subscription or create new one
    let result = sqlx::query(
        r#"
        UPDATE user_subscriptions
        SET end_date = $1, is_active = TRUE, status = 'active', updated_at = NOW()
        WHERE user_id = $2 AND pass_type = $3 AND is_active = TRUE
        "#,
    )
    .bind(end_date)
    .bind(user_id)
    .bind(pass_type.as_str())
    .execute(db)
    .await?;

    if result.rows_affected() == 0 {
        // No existing subscription, create new one
        create_subscription_from_webhook(db, user_id, event, pass_type).await?;
    }

    Ok(())
}

async fn deactivate_subscription(
    db: &PgPool,
    user_id: i64,
    product_id: &str,
) -> Result<(), AppError> {
    let pass_type = product_id_to_pass_type(product_id);

    sqlx::query(
        r#"
        UPDATE user_subscriptions
        SET is_active = FALSE, status = 'expired', updated_at = NOW()
        WHERE user_id = $1 AND pass_type = $2
        "#,
    )
    .bind(user_id)
    .bind(pass_type.as_str())
    .execute(db)
    .await?;

    Ok(())
}

async fn reactivate_subscription(
    db: &PgPool,
    user_id: i64,
    product_id: &str,
) -> Result<(), AppError> {
    let pass_type = product_id_to_pass_type(product_id);

    sqlx::query(
        r#"
        UPDATE user_subscriptions
        SET is_active = TRUE, status = 'active', updated_at = NOW()
        WHERE user_id = $1 AND pass_type = $2
        "#,
    )
    .bind(user_id)
    .bind(pass_type.as_str())
    .execute(db)
    .await?;

    Ok(())
}

async fn mark_subscription_billing_issue(
    db: &PgPool,
    user_id: i64,
    product_id: &str,
) -> Result<(), AppError> {
    let pass_type = product_id_to_pass_type(product_id);

    sqlx::query(
        r#"
        UPDATE user_subscriptions
        SET status = 'billing_issue', updated_at = NOW()
        WHERE user_id = $1 AND pass_type = $2 AND is_active = TRUE
        "#,
    )
    .bind(user_id)
    .bind(pass_type.as_str())
    .execute(db)
    .await?;

    Ok(())
}

// ============================================================================
// Student Verification
// ============================================================================

/// Send OTP to student email for verification
pub async fn verify_student(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<StudentVerifyRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Validate email format
    if !payload.email.contains('@') || !payload.email.contains('.') {
        return Err(AppError::bad_request("Invalid email format"));
    }

    // Check university domain and determine tier
    let domain = payload.email.split('@').last().unwrap_or("");
    let (university_name, tier) = determine_university_tier(domain, payload.university_name.as_deref());

    if tier == StudentTier::None {
        return Err(AppError::bad_request("Email domain not recognized as a valid university"));
    }

    // Generate 6-digit OTP
    let otp_code = format!("{:06}", rand::thread_rng().gen_range(100000..999999));

    // OTP expires in 10 minutes
    let _otp_expires_at = Utc::now().naive_utc() + chrono::Duration::minutes(10);
    let verification_expires_at = Utc::now().naive_utc() + chrono::Duration::days(365);

    // First, delete any existing record for this user
    sqlx::query("DELETE FROM student_verifications WHERE user_id = $1")
        .bind(user_id)
        .execute(&state.db)
        .await?;

    // Insert pending verification record with OTP
    sqlx::query(
        r#"
        INSERT INTO student_verifications (
            user_id, university_name, email, status, verification_method,
            discount_tier, submitted_at, expires_at, verification_code
        ) VALUES ($1, $2, $3, 'pending', 'email', $4, NOW(), $5, $6)
        "#,
    )
    .bind(user_id)
    .bind(&university_name)
    .bind(&payload.email)
    .bind(tier.as_str())
    .bind(verification_expires_at)
    .bind(&otp_code)
    .execute(&state.db)
    .await?;

    // Send OTP email
    let email_sent = send_otp_email(&payload.email, &otp_code, &university_name, &state.config).await;

    if let Err(e) = email_sent {
        tracing::warn!("Failed to send OTP email: {:?}", e);
        // Still return success but indicate email might not have been sent
        // In production, you might want to handle this differently
    }

    Ok(Json(json!({
        "message": "OTP sent to your email",
        "email": payload.email,
        "university_name": university_name,
        "discount_tier": tier.as_str(),
        "otp_expires_in_seconds": 600,
        // Include OTP in dev mode for testing (remove in production)
        "dev_otp": if state.config.is_dev_mode() { Some(&otp_code) } else { None },
    })))
}

/// Verify OTP and complete student verification
pub async fn verify_student_otp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<StudentVerifyOtpRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Find pending verification for this user and email
    let verification = sqlx::query_as::<_, (i32, String, String, String)>(
        r#"
        SELECT id, verification_code, discount_tier, university_name
        FROM student_verifications
        WHERE user_id = $1 AND email = $2 AND status = 'pending'
          AND expires_at > NOW()
        "#,
    )
    .bind(user_id)
    .bind(&payload.email)
    .fetch_optional(&state.db)
    .await?;

    let (verification_id, stored_otp, discount_tier, university_name) = verification
        .ok_or_else(|| AppError::bad_request("OTP expired or not found — please request a new one"))?;

    // Verify OTP
    if payload.otp != stored_otp {
        return Err(AppError::bad_request("Invalid OTP code"));
    }

    // Find university by domain and link it
    let email_domain = payload.email.split('@').last().unwrap_or("");
    let university_info = sqlx::query_as::<_, (i64, String)>(
        r#"
        SELECT id, country_code FROM universities
        WHERE $1 LIKE '%' || domain
        ORDER BY LENGTH(domain) DESC
        LIMIT 1
        "#
    )
    .bind(email_domain)
    .fetch_optional(&state.db)
    .await?;

    let (university_id, country_code) = university_info.unwrap_or((0, String::new()));

    // Mark as approved with university link
    sqlx::query(
        r#"
        UPDATE student_verifications
        SET status = 'approved', verified_at = NOW(),
            university_id = NULLIF($2, 0),
            university_country_code = NULLIF($3, '')
        WHERE id = $1
        "#,
    )
    .bind(verification_id)
    .bind(university_id)
    .bind(&country_code)
    .execute(&state.db)
    .await?;

    // Update user's student status
    sqlx::query("UPDATE users SET is_student_verified = TRUE, updated_at = NOW() WHERE id = $1")
        .bind(user_id)
        .execute(&state.db)
        .await?;

    // Get discount percent
    let tier = match discount_tier.as_str() {
        "top_private" => StudentTier::TopPrivate,
        "top_public" => StudentTier::TopPublic,
        _ => StudentTier::Regular,
    };

    Ok(Json(json!({
        "message": "Student verification successful!",
        "university_name": university_name,
        "university_id": if university_id > 0 { Some(university_id) } else { None },
        "country_code": if !country_code.is_empty() { Some(&country_code) } else { None },
        "discount_tier": discount_tier,
        "discount_percent": tier.discount_percent(&state.config),
        "verified": true,
    })))
}

/// Send OTP email using SMTP
async fn send_otp_email(
    to_email: &str,
    otp: &str,
    university_name: &str,
    config: &crate::config::Config,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    use lettre::{
        message::header::ContentType,
        transport::smtp::authentication::Credentials,
        AsyncSmtpTransport, AsyncTransport, Message, Tokio1Executor,
    };

    let email_body = format!(
        r#"
Hello!

Your NAVA student verification code is:

    {}

This code expires in 10 minutes.

University: {}

If you didn't request this code, please ignore this email.

- The NAVA Team
        "#,
        otp, university_name
    );

    let email = Message::builder()
        .from(config.smtp_from.parse()?)
        .to(to_email.parse()?)
        .subject("NAVA - Your Student Verification Code")
        .header(ContentType::TEXT_PLAIN)
        .body(email_body)?;

    let creds = Credentials::new(
        config.smtp_username.clone(),
        config.smtp_password.clone(),
    );

    let mailer: AsyncSmtpTransport<Tokio1Executor> =
        AsyncSmtpTransport::<Tokio1Executor>::relay(&config.smtp_host)?
            .credentials(creds)
            .build();

    mailer.send(email).await?;

    tracing::info!("OTP email sent to {}", to_email);
    Ok(())
}

pub async fn student_status(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<StudentStatusResponse>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let verification = sqlx::query_as::<_, StudentVerificationRow>(
        r#"
        SELECT id, user_id, university_name, university_type, email, student_id,
               status, verification_method, discount_tier, submitted_at, verified_at, expires_at
        FROM student_verifications
        WHERE user_id = $1 AND status = 'approved'
        ORDER BY verified_at DESC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    let response = match verification {
        Some(v) => {
            let tier = v.discount_tier.as_deref().map(StudentTier::from_str).unwrap_or(StudentTier::None);
            StudentStatusResponse {
                is_verified: true,
                university_name: v.university_name,
                discount_tier: Some(tier.as_str().to_string()),
                discount_percent: tier.discount_percent(&state.config),
                expires_at: v.expires_at.map(format_datetime),
            }
        }
        None => StudentStatusResponse {
            is_verified: false,
            university_name: None,
            discount_tier: None,
            discount_percent: 0,
            expires_at: None,
        },
    };

    Ok(Json(response))
}

// ============================================================================
// Extended Student Verification — Document upload, domain OTP, admin review
// ============================================================================

/// POST /student/verify/domain-otp
/// OTP to any institutional domain in our university whitelist (e.g. .ac.in, .edu.in)
/// Falls back to DB lookup when the domain isn't in the hardcoded tier list.
#[derive(Debug, Deserialize)]
pub struct DomainOtpRequest {
    pub email: String,
}

pub async fn verify_student_domain_otp(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<DomainOtpRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let email = payload.email.trim().to_lowercase();
    if !email.contains('@') {
        return Err(AppError::bad_request("Invalid email"));
    }
    let domain = email.split('@').last().unwrap_or("");

    // Look up university by domain in DB
    let uni = sqlx::query_as::<_, (i64, String, String, String)>(
        "SELECT id, name, tier, country_code FROM universities WHERE LOWER(domain) = $1 AND is_active = TRUE LIMIT 1"
    )
    .bind(domain)
    .fetch_optional(&state.db)
    .await?;

    let (uni_id, uni_name, tier_str, _country) = uni
        .ok_or_else(|| AppError::bad_request("Email domain not associated with any verified institution in our system"))?;

    let otp_code = format!("{:06}", rand::thread_rng().gen_range(100000..999999));
    let expires_at = Utc::now().naive_utc() + chrono::Duration::days(365);

    sqlx::query("DELETE FROM student_verifications WHERE user_id = $1")
        .bind(user_id).execute(&state.db).await?;

    sqlx::query(r#"
        INSERT INTO student_verifications
            (user_id, university_name, university_id, email, status, verification_method,
             discount_tier, submitted_at, expires_at, verification_code, assurance_level, method_detail)
        VALUES ($1, $2, $3, $4, 'pending', 'domain_otp', $5, NOW(), $6, $7, 'high', $8)
    "#)
    .bind(user_id)
    .bind(&uni_name)
    .bind(uni_id)
    .bind(&email)
    .bind(&tier_str)
    .bind(expires_at)
    .bind(&otp_code)
    .bind(format!("domain:{}", domain))
    .execute(&state.db)
    .await?;

    let email_sent = send_otp_email(&email, &otp_code, &uni_name, &state.config).await;
    if let Err(e) = email_sent {
        tracing::warn!("Failed to send domain OTP email: {:?}", e);
    }

    Ok(Json(json!({
        "message": "OTP sent to your institutional email",
        "email": email,
        "university_name": uni_name,
        "assurance_level": "high",
        "otp_expires_in_seconds": 600,
        "dev_otp": if state.config.is_dev_mode() { Some(&otp_code) } else { None },
    })))
}

/// POST /student/verify/document  (multipart)
/// Fields: doc_type (string), file (binary), selfie (binary, optional)
pub async fn verify_student_document(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let mut doc_type: Option<String> = None;
    let mut doc_bytes: Option<Vec<u8>> = None;
    let mut selfie_bytes: Option<Vec<u8>> = None;
    let mut university_name: Option<String> = None;

    while let Some(mut field) = multipart.next_field().await
        .map_err(|_| AppError::bad_request("Invalid multipart data"))? {
        match field.name().unwrap_or("") {
            "doc_type"        => { doc_type = Some(field.text().await.unwrap_or_default()); }
            "university_name" => { university_name = Some(field.text().await.unwrap_or_default()); }
            "file"            => { doc_bytes = Some(field.bytes().await.unwrap_or_default().to_vec()); }
            "selfie"          => { selfie_bytes = Some(field.bytes().await.unwrap_or_default().to_vec()); }
            _ => {}
        }
    }

    let doc_type = doc_type.ok_or_else(|| AppError::bad_request("doc_type is required"))?;
    let doc_bytes = doc_bytes.ok_or_else(|| AppError::bad_request("file is required"))?;

    // Validate doc_type
    let valid_types = ["student_id_photo", "enrollment_letter", "fee_receipt", "bonafide_certificate"];
    if !valid_types.contains(&doc_type.as_str()) {
        return Err(AppError::bad_request("Invalid doc_type"));
    }

    // Max 10MB per document
    if doc_bytes.len() > 10 * 1024 * 1024 {
        return Err(AppError::bad_request("Document exceeds 10MB limit"));
    }

    // Store document — save to local uploads dir (production: use S3/GCS)
    let uploads_dir = std::path::Path::new("uploads/verification");
    tokio::fs::create_dir_all(uploads_dir).await
        .map_err(|e| AppError::Internal(format!("Failed to create upload dir: {}", e)))?;

    let file_ext = "jpg"; // Default; could detect from magic bytes
    let file_name = format!("vdoc_{}_{}.{}", user_id, uuid::Uuid::new_v4(), file_ext);
    let file_path = uploads_dir.join(&file_name);
    tokio::fs::write(&file_path, &doc_bytes).await
        .map_err(|e| AppError::Internal(format!("Failed to store document: {}", e)))?;

    let storage_path = file_path.to_string_lossy().to_string();
    let retention_days = 90i64;
    let expires_at = Utc::now().naive_utc() + chrono::Duration::days(retention_days);

    // Create or update pending verification record
    let sv = sqlx::query_as::<_, (i32,)>(
        "SELECT id FROM student_verifications WHERE user_id = $1 AND status IN ('pending', 'approved') LIMIT 1"
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    let verification_id = if let Some((id,)) = sv {
        id
    } else {
        let uni_name = university_name.as_deref().unwrap_or("Unknown University");
        sqlx::query_scalar::<_, i32>(r#"
            INSERT INTO student_verifications
                (user_id, university_name, email, status, verification_method, submitted_at, assurance_level, method_detail)
            VALUES ($1, $2, '', 'pending', 'document', NOW(), 'medium', $3)
            RETURNING id
        "#)
        .bind(user_id)
        .bind(uni_name)
        .bind(format!("doc:{}", doc_type))
        .fetch_one(&state.db)
        .await?
    };

    // Insert document record
    let doc_id = sqlx::query_scalar::<_, i64>(r#"
        INSERT INTO verification_documents
            (user_id, verification_id, doc_type, storage_path, review_status, expires_at)
        VALUES ($1, $2, $3::verification_doc_type, $4, 'pending', $5)
        RETURNING id
    "#)
    .bind(user_id)
    .bind(verification_id)
    .bind(&doc_type)
    .bind(&storage_path)
    .bind(expires_at)
    .fetch_one(&state.db)
    .await?;

    // If selfie also uploaded, store it too
    if let Some(selfie) = selfie_bytes {
        if !selfie.is_empty() {
            let selfie_name = format!("vdoc_selfie_{}_{}.jpg", user_id, uuid::Uuid::new_v4());
            let selfie_path = uploads_dir.join(&selfie_name);
            let _ = tokio::fs::write(&selfie_path, &selfie).await;
            let _ = sqlx::query(r#"
                INSERT INTO verification_documents
                    (user_id, verification_id, doc_type, storage_path, review_status, expires_at)
                VALUES ($1, $2, 'id_selfie'::verification_doc_type, $3, 'pending', $4)
            "#)
            .bind(user_id)
            .bind(verification_id)
            .bind(selfie_path.to_string_lossy().as_ref())
            .bind(expires_at)
            .execute(&state.db)
            .await;
        }
    }

    Ok(Json(json!({
        "submitted": true,
        "document_id": doc_id,
        "verification_id": verification_id,
        "doc_type": doc_type,
        "assurance_level": "medium",
        "review_status": "pending",
        "message": "Document submitted for review. We'll verify within 24-48 hours.",
        "retention_days": retention_days,
    })))
}

// ============================================================================
// Alumni Verification
// ============================================================================

/// POST /alumni/verify-degree  (multipart)
/// Fields: file (binary), university_name (string, optional), graduation_year (string, optional)
/// Auto-approves if image is valid; otherwise queues for 24h manual review.
pub async fn verify_alumni_degree(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let mut file_bytes: Option<Vec<u8>> = None;
    let mut university_name: Option<String> = None;
    let mut graduation_year: Option<i32> = None;

    while let Some(mut field) = multipart.next_field().await
        .map_err(|_| AppError::bad_request("Invalid multipart data"))? {
        match field.name().unwrap_or("") {
            "file"            => { file_bytes = Some(field.bytes().await.unwrap_or_default().to_vec()); }
            "university_name" => { university_name = Some(field.text().await.unwrap_or_default()); }
            "graduation_year" => {
                if let Ok(y) = field.text().await.unwrap_or_default().parse::<i32>() {
                    graduation_year = Some(y);
                }
            }
            _ => {}
        }
    }

    let file_bytes = file_bytes.ok_or_else(|| AppError::bad_request("file is required"))?;
    if file_bytes.is_empty() { return Err(AppError::bad_request("file is empty")); }
    if file_bytes.len() > state.config.max_photo_bytes {
        return Err(AppError::bad_request(format!(
            "File exceeds {}MB limit",
            state.config.max_photo_bytes / 1024 / 1024
        )));
    }

    // Store document — derive extension from magic bytes so HEIC/PNG/PDF are all preserved
    let ext = detect_image_ext(&file_bytes);
    let uploads_dir = std::path::Path::new("uploads/verification/alumni");
    tokio::fs::create_dir_all(uploads_dir).await
        .map_err(|e| AppError::Internal(format!("Upload dir error: {}", e)))?;
    let file_name = format!("alumni_degree_{}_{}_{}.{}", user_id, uuid::Uuid::new_v4(), Utc::now().timestamp(), ext);
    let file_path = uploads_dir.join(&file_name);
    tokio::fs::write(&file_path, &file_bytes).await
        .map_err(|e| AppError::Internal(format!("Failed to store file: {}", e)))?;
    let doc_path = file_path.to_string_lossy().to_string();

    let uni_name = university_name.as_deref().unwrap_or("Unknown University");
    let expires_at = (Utc::now() + chrono::Duration::days(90)).naive_utc();

    // Delete any prior pending alumni verification
    sqlx::query("DELETE FROM student_verifications WHERE user_id = $1 AND is_alumni = TRUE AND status = 'pending'")
        .bind(user_id).execute(&state.db).await?;

    let sv_id = sqlx::query_scalar::<_, i32>(r#"
        INSERT INTO student_verifications
            (user_id, university_name, graduation_year, email, status, verification_method,
             is_alumni, alumni_doc_path, assurance_level, submitted_at, expires_at)
        VALUES ($1, $2, $3, '', 'pending', 'alumni_degree', TRUE, $4, 'medium', NOW(), $5)
        RETURNING id
    "#)
    .bind(user_id).bind(uni_name).bind(graduation_year).bind(&doc_path).bind(expires_at)
    .fetch_one(&state.db).await?;

    Ok(Json(json!({
        "submitted": true,
        "verification_id": sv_id,
        "method": "alumni_degree",
        "status": "pending",
        "message": "Degree submitted for review. We'll verify within 24 hours.",
        "auto_approved": false
    })))
}

/// POST /alumni/verify-linkedin
/// Body: { "linkedin_url": "https://linkedin.com/in/...", "university_name": "..." }
/// Always goes to manual review.
pub async fn verify_alumni_linkedin(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let linkedin_url = body.get("linkedin_url")
        .and_then(|v| v.as_str())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::bad_request("linkedin_url is required"))?;

    // Basic URL validation
    if !linkedin_url.starts_with("https://www.linkedin.com/in/") && !linkedin_url.starts_with("https://linkedin.com/in/") {
        return Err(AppError::bad_request("Please provide a valid LinkedIn profile URL (https://linkedin.com/in/...)"));
    }

    let university_name = body.get("university_name")
        .and_then(|v| v.as_str())
        .unwrap_or("Unknown University");

    let expires_at = (Utc::now() + chrono::Duration::days(90)).naive_utc();

    // Delete any prior pending alumni linkedin verification
    sqlx::query("DELETE FROM student_verifications WHERE user_id = $1 AND is_alumni = TRUE AND status = 'pending' AND verification_method = 'alumni_linkedin'")
        .bind(user_id).execute(&state.db).await?;

    let sv_id = sqlx::query_scalar::<_, i32>(r#"
        INSERT INTO student_verifications
            (user_id, university_name, email, status, verification_method,
             is_alumni, linkedin_url, assurance_level, submitted_at, expires_at)
        VALUES ($1, $2, '', 'pending', 'alumni_linkedin', TRUE, $3, 'low', NOW(), $4)
        RETURNING id
    "#)
    .bind(user_id).bind(university_name).bind(&linkedin_url).bind(expires_at)
    .fetch_one(&state.db).await?;

    Ok(Json(json!({
        "submitted": true,
        "verification_id": sv_id,
        "method": "alumni_linkedin",
        "status": "pending",
        "message": "LinkedIn profile submitted for manual review. This typically takes 24–48 hours.",
        "auto_approved": false
    })))
}

/// GET /admin/verification/queue — List pending document verifications (admin only)
pub async fn admin_verification_queue(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _admin_id = decode_access_token(&token, &state.config.secret_key)?;
    // TODO: enforce admin role check when role system is added

    #[derive(Debug, sqlx::FromRow, Serialize)]
    struct QueueItem {
        doc_id: i64,
        user_id: i32,
        user_name: Option<String>,
        doc_type: String,
        confidence_score: Option<f32>,
        extracted_institute: Option<String>,
        extracted_name: Option<String>,
        extracted_expiry: Option<NaiveDate>,
        review_status: String,
        submitted_at: Option<chrono::NaiveDateTime>,
        verification_id: Option<i32>,
        university_name: Option<String>,
    }

    let items = sqlx::query_as::<_, QueueItem>(r#"
        SELECT vd.id AS doc_id, vd.user_id, u.name AS user_name,
               vd.doc_type::text, vd.confidence_score, vd.extracted_institute,
               vd.extracted_name, vd.extracted_expiry,
               vd.review_status::text, vd.created_at AS submitted_at,
               sv.id AS verification_id, sv.university_name
        FROM verification_documents vd
        JOIN users u ON u.id = vd.user_id
        LEFT JOIN student_verifications sv ON sv.id = vd.verification_id
        WHERE vd.review_status = 'pending'
        ORDER BY vd.confidence_score ASC NULLS FIRST, vd.created_at ASC
        LIMIT 50
    "#)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(json!({ "queue": items, "total": items.len() })))
}

/// POST /admin/verification/{doc_id}/decision — Approve or reject a document
#[derive(Debug, Deserialize)]
pub struct VerificationDecision {
    pub action: String, // "approve" | "reject" | "needs_more_info"
    pub notes: Option<String>,
    pub assurance_level: Option<String>, // override: "high" | "medium"
}

pub async fn admin_verification_decision(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(doc_id): AxumPath<i64>,
    Json(payload): Json<VerificationDecision>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let admin_id = decode_access_token(&token, &state.config.secret_key)?;

    if !["approve", "reject", "needs_more_info"].contains(&payload.action.as_str()) {
        return Err(AppError::bad_request("action must be approve, reject, or needs_more_info"));
    }

    // Fetch the document and linked verification
    let doc = sqlx::query_as::<_, (i32, Option<i32>, String)>(
        "SELECT user_id, verification_id, doc_type::text FROM verification_documents WHERE id = $1"
    )
    .bind(doc_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("Document not found"))?;

    let (doc_user_id, verification_id, _doc_type) = doc;

    // Update document review status
    sqlx::query(r#"
        UPDATE verification_documents
        SET review_status = $1::doc_review_status, review_notes = $2,
            reviewed_by = $3, reviewed_at = NOW()
        WHERE id = $4
    "#)
    .bind(&payload.action)
    .bind(&payload.notes)
    .bind(admin_id)
    .bind(doc_id)
    .execute(&state.db)
    .await?;

    // If approved, update the student_verification record
    if payload.action == "approve" {
        let assurance = payload.assurance_level.as_deref().unwrap_or("medium");
        let expires_at = Utc::now().naive_utc() + chrono::Duration::days(365);

        if let Some(sv_id) = verification_id {
            sqlx::query(r#"
                UPDATE student_verifications
                SET status = 'approved', verified_at = NOW(),
                    expires_at = $1, assurance_level = $2
                WHERE id = $3
            "#)
            .bind(expires_at)
            .bind(assurance)
            .bind(sv_id)
            .execute(&state.db)
            .await?;
        }

        // Mark user as student verified
        sqlx::query("UPDATE users SET is_student_verified = TRUE, updated_at = NOW() WHERE id = $1")
            .bind(doc_user_id)
            .execute(&state.db)
            .await?;
    } else if payload.action == "reject" {
        if let Some(sv_id) = verification_id {
            sqlx::query("UPDATE student_verifications SET status = 'rejected' WHERE id = $1")
                .bind(sv_id)
                .execute(&state.db)
                .await?;
        }
    }

    Ok(Json(json!({
        "decided": true,
        "doc_id": doc_id,
        "action": payload.action,
        "user_id": doc_user_id,
    })))
}

// ============================================================================
// Calls
// ============================================================================

pub async fn create_call(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateCallRequest>,
) -> Result<Json<CreateCallResponse>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Verify user is part of the match
    let m = sqlx::query_as::<_, MatchCheckRow>(
        "SELECT id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match FROM matches WHERE id = $1",
    )
    .bind(&payload.match_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("Match not found"))?;

    if m.user1_id != user_id && m.user2_id != user_id {
        return Err(AppError::forbidden("Not authorized for this match"));
    }

    if !m.is_mutual_match.unwrap_or(false) {
        return Err(AppError::bad_request("Cannot call without mutual match"));
    }

    let callee_id = if m.user1_id == user_id { m.user2_id } else { m.user1_id };
    let call_id = Uuid::new_v4().to_string();
    let call_type = payload.call_type.as_deref().unwrap_or("voice").to_string();

    // Create call session
    let session = CallSession {
        call_id: call_id.clone(),
        match_id: payload.match_id.clone(),
        caller_id: user_id,
        callee_id,
        call_type,
        status: "ringing".to_string(),
        started_at: Utc::now().naive_utc(),
        ended_at: None,
    };

    // Persist to shared Redis storage first so a callee whose WebSocket lands on
    // a different pod can still find and join this call, then register locally.
    crate::realtime::store_call_session(&state, &session).await;
    {
        let mut sessions = state.call_sessions.write().await;
        sessions.create(session);
    }

    // Generate call token
    let call_token = create_call_token(
        user_id,
        &payload.match_id,
        &call_id,
        &state.config.secret_key,
        state.config.call_token_expire_minutes,
    )?;

    Ok(Json(CreateCallResponse {
        call_id,
        call_token,
        expires_in: state.config.call_token_expire_minutes * 60,
    }))
}

// ============================================================================
// Banner image upload helper (shared by events, playgrounds, spots)
// ============================================================================

/// Decode image bytes. Tries the `image` crate first (JPEG/PNG/GIF/WebP/TIFF/
/// BMP/ICO/TGA/QOI/PNM), falls back to ffmpeg for HEIC/HEIF/AVIF/anything else
/// the image crate can't natively decode.
///
/// The ffmpeg fallback handles any format ffmpeg knows, including:
/// - HEIC/HEIF (iPhone default camera format)
/// - AVIF
/// - JPEG 2000, JPEG XL
/// - RAW/DNG (via ffmpeg's libraw)
async fn decode_any_image(bytes: &[u8]) -> Result<image::DynamicImage, AppError> {
    // Fast path: pure-rust decoders for common formats.
    if let Ok(img) = image::load_from_memory(bytes) {
        return Ok(img);
    }

    // Fallback: shell out to ffmpeg. Writes input+output to temp files in /tmp
    // because ffmpeg's stdin parsing can fail for some formats (HEIC in particular).
    let tmp_dir = std::env::temp_dir();
    let nonce = Uuid::new_v4();
    let input_path  = tmp_dir.join(format!("banner_in_{}.bin", nonce));
    let output_path = tmp_dir.join(format!("banner_out_{}.png", nonce));

    fs::write(&input_path, bytes).await
        .map_err(|_| AppError::internal("image decode: tmp write failed"))?;

    let status = tokio::process::Command::new("ffmpeg")
        .arg("-y")                       // overwrite output
        .arg("-i").arg(&input_path)
        .arg("-frames:v").arg("1")       // single frame (HEIC sequences, GIFs)
        .arg("-f").arg("image2")
        .arg("-vcodec").arg("png")       // lossless handoff to image crate
        .arg(&output_path)
        .stderr(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .status()
        .await;

    // Always clean up input; output handled below
    let _ = fs::remove_file(&input_path).await;

    let ok = status.map(|s| s.success()).unwrap_or(false);
    if !ok {
        let _ = fs::remove_file(&output_path).await;
        return Err(AppError::bad_request("unsupported or invalid image format"));
    }

    let png_bytes = match fs::read(&output_path).await {
        Ok(b) => b,
        Err(_) => {
            let _ = fs::remove_file(&output_path).await;
            return Err(AppError::internal("image decode: read converted failed"));
        }
    };
    let _ = fs::remove_file(&output_path).await;

    image::load_from_memory(&png_bytes)
        .map_err(|_| AppError::bad_request("image decode: converted image unreadable"))
}

/// Decode a base64 image, validate, encode as high-quality JPEG, persist
/// under /uploads/banners/. Returns URL path or None.
///
/// Quality targets (retina/@3x iPhone + iPad):
/// - Long-edge cap: 2560px (iPad 3x landscape banners, 12.9" iPad Pro width)
/// - JPEG quality: 92 (near-lossless; doubles our profile-photo encoder's 90)
/// - Lanczos3 resample (best-in-class for downscale sharpness)
/// - No resize at all if source is already at/under target (avoids double-JPEG
///   softening when iOS already sent an appropriately-sized banner)
async fn save_base64_banner(
    state: &AppState,
    user_id: i32,
    b64: &str,
    prefix: &str, // "event", "playground", "spot"
) -> Result<Option<String>, AppError> {
    use image::codecs::jpeg::JpegEncoder;
    use image::ColorType;

    const MAX_UPLOAD_BYTES: usize = 10 * 1024 * 1024;   // 10MB source cap
    const TARGET_LONG_EDGE: u32   = 2560;               // retina/iPad-safe
    const MIN_DIM: u32            = 64;
    const MAX_DIM: u32            = 8000;               // allow large originals
    const JPEG_QUALITY: u8        = 92;

    let cleaned = b64.trim();
    if cleaned.is_empty() { return Ok(None); }

    // Strip optional data URL prefix: data:image/jpeg;base64,xxxx
    let raw = cleaned
        .split_once(',')
        .map(|(_, tail)| tail)
        .unwrap_or(cleaned);

    let bytes = STANDARD.decode(raw)
        .map_err(|_| AppError::bad_request("banner: invalid base64"))?;
    if bytes.len() > MAX_UPLOAD_BYTES {
        return Err(AppError::bad_request("banner: max 10MB"));
    }

    let img = decode_any_image(&bytes).await?;
    let (w, h) = (img.width(), img.height());
    if w < MIN_DIM || h < MIN_DIM {
        return Err(AppError::bad_request(format!("banner: min {}x{}px", MIN_DIM, MIN_DIM)));
    }
    if w > MAX_DIM || h > MAX_DIM {
        return Err(AppError::bad_request(format!("banner: max {}x{}px", MAX_DIM, MAX_DIM)));
    }

    // Only downscale when the source actually exceeds target. Upscaling or
    // re-resizing an already-sized image just costs quality for zero benefit.
    let img = if w.max(h) > TARGET_LONG_EDGE {
        img.resize(TARGET_LONG_EDGE, TARGET_LONG_EDGE, image::imageops::FilterType::Lanczos3)
    } else { img };

    // High-quality JPEG encode (q=92). Separate from encode_jpeg() helper
    // which is tuned for smaller profile thumbnails at q=90.
    let rgb = img.to_rgb8();
    let mut jpeg_bytes = Vec::with_capacity(bytes.len());
    let mut encoder = JpegEncoder::new_with_quality(&mut jpeg_bytes, JPEG_QUALITY);
    encoder.encode(&rgb, rgb.width(), rgb.height(), ColorType::Rgb8.into())
        .map_err(|_| AppError::internal("banner: jpeg encode failed"))?;

    let banner_dir = Path::new(&state.config.upload_dir).join("banners");
    fs::create_dir_all(&banner_dir).await
        .map_err(|_| AppError::internal("banner: mkdir failed"))?;

    let filename = format!(
        "{}_{}_{}_{}.jpg",
        prefix, user_id, Utc::now().timestamp(), Uuid::new_v4()
    );
    let path = banner_dir.join(&filename);
    fs::write(&path, &jpeg_bytes).await
        .map_err(|_| AppError::internal("banner: write failed"))?;

    Ok(Some(format!("/uploads/banners/{}", filename)))
}

/// POST /uploads/banner
/// Streaming multipart upload for banner images. Accepts up to 150MB raw.
/// Returns {banner_url: "/uploads/banners/xxx.jpg"} which clients then pass
/// to the create endpoints (events, playgrounds, outdoor_spots).
///
/// This is the preferred path for any banner above ~5MB. Small crops/filters
/// can still use the base64 field on the create endpoints directly.
pub async fn upload_banner(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    use image::codecs::jpeg::JpegEncoder;
    use image::ColorType;

    const MAX_UPLOAD_BYTES: usize = 150 * 1024 * 1024;
    const TARGET_LONG_EDGE: u32   = 2560;
    const MIN_DIM: u32            = 64;
    const MAX_DIM: u32            = 16000;   // modern DSLR / panorama
    const JPEG_QUALITY: u8        = 92;

    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Stream the first image field. Collect bytes as they arrive (axum's
    // multipart is chunked; no 200MB blob in memory at once during network IO).
    let mut image_bytes: Option<Vec<u8>> = None;
    while let Some(mut field) = multipart.next_field().await
        .map_err(|_| AppError::bad_request("invalid multipart"))?
    {
        let name = field.name().unwrap_or("").to_string();
        if name != "banner" && name != "file" && name != "image" { continue; }

        // Some clients (notably iOS Share Extension with HEIC) send
        // application/octet-stream. Trust the decoder below rather than
        // rejecting on MIME alone.
        let ct = field.content_type().map(|s| s.to_string()).unwrap_or_default();
        if !ct.is_empty()
            && !ct.starts_with("image/")
            && ct != "application/octet-stream"
        {
            return Err(AppError::bad_request("banner must be an image"));
        }

        let mut buf: Vec<u8> = Vec::with_capacity(4 * 1024 * 1024);
        while let Some(chunk) = field.chunk().await
            .map_err(|_| AppError::bad_request("multipart read failed"))?
        {
            if buf.len() + chunk.len() > MAX_UPLOAD_BYTES {
                return Err(AppError::bad_request("banner: max 150MB"));
            }
            buf.extend_from_slice(&chunk);
        }
        image_bytes = Some(buf);
        break;
    }

    let bytes = image_bytes.ok_or_else(|| AppError::bad_request("missing 'banner' field"))?;
    if bytes.is_empty() {
        return Err(AppError::bad_request("banner: empty file"));
    }

    // Decode via format-agnostic helper (image crate + ffmpeg fallback for
    // HEIC/HEIF/AVIF/JPEG-XL/RAW and anything ffmpeg knows).
    let img = decode_any_image(&bytes).await?;
    let (w, h) = (img.width(), img.height());
    if w < MIN_DIM || h < MIN_DIM {
        return Err(AppError::bad_request(format!("banner: min {}x{}px", MIN_DIM, MIN_DIM)));
    }
    if w > MAX_DIM || h > MAX_DIM {
        return Err(AppError::bad_request(format!("banner: max {}x{}px", MAX_DIM, MAX_DIM)));
    }

    // Downscale only when necessary — preserve source quality for images already
    // at or under our target resolution.
    let img = if w.max(h) > TARGET_LONG_EDGE {
        img.resize(TARGET_LONG_EDGE, TARGET_LONG_EDGE, image::imageops::FilterType::Lanczos3)
    } else { img };

    let rgb = img.to_rgb8();
    let mut jpeg_bytes = Vec::with_capacity(2 * 1024 * 1024);
    let mut encoder = JpegEncoder::new_with_quality(&mut jpeg_bytes, JPEG_QUALITY);
    encoder.encode(&rgb, rgb.width(), rgb.height(), ColorType::Rgb8.into())
        .map_err(|_| AppError::internal("banner: jpeg encode failed"))?;

    let banner_dir = Path::new(&state.config.upload_dir).join("banners");
    fs::create_dir_all(&banner_dir).await
        .map_err(|_| AppError::internal("banner: mkdir failed"))?;

    let filename = format!(
        "upload_{}_{}_{}.jpg",
        user_id, Utc::now().timestamp(), Uuid::new_v4()
    );
    let path = banner_dir.join(&filename);
    fs::write(&path, &jpeg_bytes).await
        .map_err(|_| AppError::internal("banner: write failed"))?;

    let url = format!("/uploads/banners/{}", filename);
    Ok(Json(json!({
        "banner_url": url,
        "width": rgb.width(),
        "height": rgb.height(),
        "bytes": jpeg_bytes.len()
    })))
}

/// Extract banner_url from a create payload: accept either `banner` (base64) or
/// a pre-uploaded `banner_url`. Base64 wins if both provided.
async fn extract_banner_url(
    state: &AppState,
    user_id: i32,
    payload: &Value,
    prefix: &str,
) -> Result<Option<String>, AppError> {
    if let Some(b64) = payload.get("banner").and_then(|v| v.as_str()) {
        if !b64.trim().is_empty() {
            return save_base64_banner(state, user_id, b64, prefix).await;
        }
    }
    Ok(payload.get("banner_url").and_then(|v| v.as_str()).map(|s| s.to_string()))
}

// ============================================================================
// Spots (Short Videos)
// ============================================================================

pub async fn create_spot(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Check spot limit for free users
    let active_pass = get_active_pass(&state.db, user_id).await?;
    let is_premium = active_pass.is_some();

    if !is_premium {
        let spot_count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM spots WHERE user_id = $1 AND (expires_at IS NULL OR expires_at > NOW())",
        )
        .bind(user_id)
        .fetch_one(&state.db)
        .await?;

        if spot_count >= state.config.free_spots_limit as i64 {
            return Err(AppError::bad_request(format!(
                "Free users can only have {} active spots. Upgrade to premium for unlimited spots.",
                state.config.free_spots_limit
            )));
        }
    }

    let mut title: Option<String> = None;
    let mut city: Option<String> = None;
    let mut tags: Option<Vec<String>> = None;
    let mut is_global: bool = true;
    let mut video_data: Option<Vec<u8>> = None;
    let mut mime_type: Option<String> = None;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::bad_request("Invalid multipart data"))?
    {
        let field_name = field.name().unwrap_or("").to_string();
        match field_name.as_str() {
            "title" => {
                title = Some(read_text_field(&mut field, 100).await?);
            }
            "city" => {
                city = Some(read_text_field(&mut field, 100).await?);
            }
            "tags" => {
                let tags_str = read_text_field(&mut field, 500).await?;
                tags = Some(tags_str.split(',').map(|s| s.trim().to_string()).collect());
            }
            "is_global" => {
                let val = read_text_field(&mut field, 10).await?;
                is_global = val == "true" || val == "1";
            }
            "video" | "media" => {
                let ct = field
                    .content_type()
                    .map(|v| v.to_string())
                    .unwrap_or_default();
                if !ct.starts_with("video/") && !ct.starts_with("audio/") {
                    return Err(AppError::bad_request("File must be video or audio"));
                }
                mime_type = Some(ct);
                video_data = Some(read_binary_field(&mut field, state.config.max_video_bytes).await?);
            }
            _ => {}
        }
    }

    let video_bytes = video_data.ok_or_else(|| AppError::bad_request("Video/audio file is required"))?;
    let mime = mime_type.unwrap_or_else(|| "video/mp4".to_string());

    // Save file
    let upload_dir = &state.config.upload_dir;
    fs::create_dir_all(format!("{}/spots", upload_dir))
        .await
        .map_err(|_| AppError::internal("Failed to create spots directory"))?;

    let ext = if mime.contains("quicktime") || mime.contains("mov") {
        "mov"
    } else if mime.contains("mp4") || mime.contains("m4v") {
        "mp4"
    } else if mime.contains("webm") {
        "webm"
    } else if mime.contains("hevc") {
        "hevc"
    } else if mime.contains("x-m4v") {
        "m4v"
    } else if mime.contains("audio") {
        "m4a"
    } else {
        "mp4"
    };

    let filename = format!("spots/{}_{}_{}.{}", user_id, Utc::now().timestamp(), Uuid::new_v4(), ext);
    let path = format!("{}/{}", upload_dir, filename);
    fs::write(&path, &video_bytes)
        .await
        .map_err(|_| AppError::internal("Failed to save video"))?;

    // Calculate expiry
    let expires_at = if is_premium {
        None
    } else {
        Some(Utc::now().naive_utc() + chrono::Duration::days(state.config.spot_expiry_days as i64))
    };

    // Insert spot record. hls_state defaults to 'pending' from migration 031.
    let spot_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO spots (user_id, title, original_url, mime_type, tags, city, is_global, expires_at, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW(), NOW())
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(&title)
    .bind(&path)
    .bind(&mime)
    .bind(tags.map(|t| json!(t)))
    .bind(&city)
    .bind(is_global)
    .bind(expires_at)
    .fetch_one(&state.db)
    .await?;

    // Kick off HLS transcoding in the background. Mirrors the reels path
    // (handlers/mod.rs:8095). iOS prefers hls_url when ready, falls back to
    // original_url while hls_state is 'pending' / 'failed'.
    if mime.starts_with("video/") {
        let db_clone = state.db.clone();
        let upload_dir_clone = state.config.upload_dir.clone();
        let disk_path_clone = path.clone();
        tokio::spawn(async move {
            let _ = sqlx::query("UPDATE spots SET hls_state = 'processing' WHERE id = $1")
                .bind(spot_id)
                .execute(&db_clone)
                .await;

            let start = std::time::Instant::now();
            let probe = crate::hls::probe_video(&disk_path_clone).await;
            let needs_normalize = match &probe {
                Ok(p) => {
                    let dominated = p.duration_secs <= 31.0 && p.codec == "h264";
                    tracing::info!(
                        "Spot {} probe: {:.1}s, codec={}, {}px wide, skip_normalize={}",
                        spot_id, p.duration_secs, p.codec, p.width, dominated
                    );
                    !dominated
                }
                Err(e) => {
                    tracing::warn!("ffprobe failed for spot {}: {} — will normalize", spot_id, e);
                    true
                }
            };

            let result = if needs_normalize {
                if let Err(e) = crate::hls::normalize_video(&disk_path_clone).await {
                    tracing::warn!("Normalization failed for spot {} (proceeding with original): {}", spot_id, e);
                }
                crate::hls::transcode_to_hls(spot_id, &disk_path_clone, &upload_dir_clone, "spots").await
            } else {
                crate::hls::normalize_and_hls(spot_id, &disk_path_clone, &upload_dir_clone, "spots").await
            };

            match result {
                Ok(hls_url) => {
                    let _ = sqlx::query(
                        "UPDATE spots SET hls_url = $1, hls_state = 'ready' WHERE id = $2",
                    )
                    .bind(&hls_url)
                    .bind(spot_id)
                    .execute(&db_clone)
                    .await;
                    tracing::info!("HLS ready for spot {} in {:.1}s: {}", spot_id, start.elapsed().as_secs_f64(), hls_url);
                }
                Err(e) => {
                    let _ = sqlx::query("UPDATE spots SET hls_state = 'failed' WHERE id = $1")
                        .bind(spot_id)
                        .execute(&db_clone)
                        .await;
                    tracing::warn!("HLS failed for spot {}: {}", spot_id, e);
                }
            }
        });
    } else {
        // Audio-only spots: skip transcoding entirely, mark failed so iOS reads original_url.
        let _ = sqlx::query("UPDATE spots SET hls_state = 'failed' WHERE id = $1")
            .bind(spot_id)
            .execute(&state.db)
            .await;
    }

    Ok(Json(json!({
        "message": "Spot created successfully",
        "spot_id": spot_id.to_string(),
        "url": path,
        "expires_at": expires_at.map(format_datetime),
    })))
}

pub async fn get_spots(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let spots = fetch_user_spots(state.read_pool(), user_id, 50).await?;

    let results: Vec<Value> = spots
        .into_iter()
        .map(|s| {
            json!({
                "id": s.id,
                "title": s.title,
                "poster_url": s.poster_url,
                "renditions": s.renditions,
                "expires_at": s.expires_at.map(format_datetime),
                "created_at": s.created_at.map(format_datetime),
                "is_global": s.is_global,
                "city": s.city,
                "tags": s.tags,
            })
        })
        .collect();

    Ok(Json(json!({ "spots": results })))
}

// ============================================================================
// Spots Feed & Messaging
// ============================================================================

/// GET /spots/feed — ML-ranked spots: same city > shared interests > recency
pub async fn get_spots_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let limit = params.get("limit").and_then(|v| v.parse::<i64>().ok()).unwrap_or(20);

    // Over-fetch by 3× so the ranker has headroom to reorder before truncation.
    let spots = sqlx::query_as::<_, SpotFullRow>(
        r#"SELECT s.id, s.user_id, s.title, s.original_url, s.poster_url, s.mime_type,
                  s.duration_sec, s.renditions, s.tags, s.city, s.is_global,
                  s.expires_at, s.created_at, s.updated_at, s.hls_url, s.hls_state
           FROM spots s
           WHERE s.user_id != $1
             AND (s.expires_at IS NULL OR s.expires_at > NOW())
           ORDER BY s.created_at DESC LIMIT $2"#,
    )
    .bind(user_id)
    .bind(limit * 3)
    .fetch_all(&state.db)
    .await?;

    let user_info = sqlx::query_as::<_, (Option<String>, Option<serde_json::Value>)>(
        "SELECT ul.city, u.interests FROM users u LEFT JOIN user_locations ul ON ul.user_id = u.id WHERE u.id = $1"
    ).bind(user_id).fetch_optional(&state.db).await?.unwrap_or((None, None));

    let user_city = user_info.0.unwrap_or_default();
    let user_interests: Vec<String> = user_info.1
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    let candidates: Vec<crate::ml::router::SpotCandidate> =
        spots.into_iter().map(Into::into).collect();
    let ctx = crate::ml::router::SpotsFeedCtx {
        user_id,
        user_city,
        user_interests,
        limit: limit as usize,
    };

    let result = state.ranking_router.rank_spots_feed(&ctx, candidates).await?;

    let results: Vec<Value> = result
        .ranked
        .into_iter()
        .map(|s| {
            json!({
                "id": s.spot.id.to_string(),
                "user_id": s.spot.user_id.to_string(),
                "title": s.spot.title,
                "poster_url": s.spot.poster_url,
                "original_url": s.spot.original_url,
                "hls_url": s.spot.hls_url,
                "hls_state": s.spot.hls_state,
                "city": s.spot.city,
                "tags": s.spot.tags,
                "relevance_score": (s.score * 100.0) as i32,
                "created_at": s.spot.created_at.map(format_datetime),
                "expires_at": s.spot.expires_at.map(format_datetime),
            })
        })
        .collect();

    Ok(Json(json!({
        "spots": results,
        "model_id": result.model_id,
        "experiment_id": result.experiment_id,
        "experiment_cell": result.experiment_cell,
    })))
}

/// GET /spots/:id/messages?since=2024-01-01T00:00:00
pub async fn get_spot_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(spot_id): AxumPath<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _user_id = decode_access_token(&token, &state.config.secret_key)?;

    let since = params.get("since").and_then(|s|
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok()
            .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok()));

    let msgs = if let Some(since_ts) = since {
        sqlx::query_as::<_, SpotMessageRow>(
            "SELECT id, spot_id, sender_id, text, created_at FROM spot_messages WHERE spot_id = $1 AND created_at > $2 ORDER BY created_at ASC LIMIT 500"
        )
        .bind(spot_id)
        .bind(since_ts)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, SpotMessageRow>(
            "SELECT id, spot_id, sender_id, text, created_at FROM spot_messages WHERE spot_id = $1 ORDER BY created_at ASC LIMIT 100"
        )
        .bind(spot_id)
        .fetch_all(&state.db)
        .await?
    };

    let results: Vec<Value> = msgs.into_iter().map(|m| {
        json!({ "id": m.id.to_string(), "spot_id": m.spot_id.to_string(), "sender_id": m.sender_id.to_string(), "text": m.text, "created_at": m.created_at.map(format_datetime) })
    }).collect();

    Ok(Json(json!({ "messages": results })))
}

/// POST /spots/:id/messages
pub async fn send_spot_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(spot_id): AxumPath<i64>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let text = payload["text"].as_str().unwrap_or("").to_string();
    if text.is_empty() { return Err(AppError::bad_request("Missing 'text'")); }

    let id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO spot_messages (spot_id, sender_id, text) VALUES ($1, $2, $3) RETURNING id"
    )
    .bind(spot_id).bind(user_id).bind(&text)
    .fetch_one(&state.db).await?;

    // Auto-queue spot message for LLM labeling
    auto_queue_for_labeling(state.db.clone(), state.config.llm_enabled, "spot_message", id, 4);

    // Update pair status for matching
    let _ = sqlx::query(
        r#"INSERT INTO spot_pair_status (spot_id, user_a, user_b, a_count)
           SELECT $1, $2, s.user_id, 1 FROM spots s WHERE s.id = $1 AND s.user_id != $2
           ON CONFLICT (spot_id, user_a, user_b) DO UPDATE SET a_count = spot_pair_status.a_count + 1, updated_at = NOW()"#
    ).bind(spot_id).bind(user_id).execute(&state.db).await;

    Ok(Json(json!({ "message_id": id.to_string() })))
}

/// POST /spots/:id/react — react to a spot (tracks engagement for pair matching)
pub async fn react_to_spot(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(spot_id): AxumPath<i64>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let _ = sqlx::query(
        r#"INSERT INTO spot_pair_status (spot_id, user_a, user_b, a_count)
           SELECT $1, $2, s.user_id, 1 FROM spots s WHERE s.id = $1 AND s.user_id != $2
           ON CONFLICT (spot_id, user_a, user_b) DO UPDATE SET a_count = spot_pair_status.a_count + 1, updated_at = NOW()"#
    ).bind(spot_id).bind(user_id).execute(&state.db).await;

    Ok(Json(json!({ "ok": true })))
}

// ============================================================================
// Playgrounds (Group Hangouts)
// ============================================================================

/// GET /playgrounds — ML-ranked: friends-of-friends > same university > same city > interest match
pub async fn get_playgrounds(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let pg_type = params.get("type").cloned();
    let limit = params.get("limit").and_then(|v| v.parse::<i64>().ok()).unwrap_or(20);

    // Get user context for scoring
    let (user_city, user_uni_id, user_interests) = {
        let row = sqlx::query_as::<_, (Option<String>, Option<serde_json::Value>)>(
            "SELECT ul.city, u.interests FROM users u LEFT JOIN user_locations ul ON ul.user_id = u.id WHERE u.id = $1"
        ).bind(user_id).fetch_optional(&state.db).await?.unwrap_or((None, None));

        let uni_id = sqlx::query_scalar::<_, Option<i64>>(
            "SELECT university_id FROM student_verifications WHERE user_id = $1 AND status = 'verified' LIMIT 1"
        ).bind(user_id).fetch_optional(&state.db).await?.flatten();

        let interests: Vec<String> = row.1.and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
        (row.0.unwrap_or_default().to_lowercase(), uni_id, interests)
    };

    // Get playground IDs user already joined
    let joined: Vec<i64> = sqlx::query_scalar(
        "SELECT playground_id FROM playground_members WHERE user_id = $1 AND is_active = true"
    ).bind(user_id).fetch_all(&state.db).await?;

    // Get playgrounds with friends count (users who matched with me that are in each playground)
    let playgrounds = sqlx::query_as::<_, (i64, String, Option<String>, String, Option<String>, i32, i32, Option<String>, Option<String>, Option<i64>, Option<i32>)>(
        r#"SELECT p.id, p.name, p.description, p.playground_type, p.city,
                  p.member_count, p.active_today, p.cover_image_url, p.icon_url, p.university_id,
                  p.max_members
           FROM playgrounds p
           WHERE p.is_active = true AND ($1::text IS NULL OR p.playground_type = $1)
           ORDER BY p.active_today DESC, p.member_count DESC LIMIT $2"#,
    )
    .bind(&pg_type).bind(limit * 3)
    .fetch_all(&state.db).await?;

    // Score and rank
    let mut scored: Vec<(f64, Value)> = playgrounds.into_iter().map(|p| {
        let mut score = 0.0;
        let is_joined = joined.contains(&p.0);

        // Already joined gets top priority
        if is_joined { score += 1.0; }

        // Same university bonus (+35%)
        if let (Some(u_id), Some(p_uni)) = (user_uni_id, p.9) {
            if u_id == p_uni { score += 0.35; }
        }

        // Same city bonus (+25%)
        if let Some(ref city) = p.4 {
            if city.to_lowercase() == user_city { score += 0.25; }
        }

        // Interest-type match (+20%) — playground name/type matches user interests
        let name_lower = p.1.to_lowercase();
        let interest_match = user_interests.iter().any(|i| name_lower.contains(&i.to_lowercase()));
        if interest_match || p.3 == "interest" { score += 0.2; }

        // Activity bonus (+20%) — more active today = more engaging
        score += 0.2 * (p.6 as f64 / (p.6 as f64 + 10.0)); // sigmoid-like

        let val = json!({
            "id": p.0.to_string(), "name": p.1, "description": p.2, "type": p.3, "city": p.4,
            "member_count": p.5, "max_members": p.10, "active_today": p.6,
            "cover_image_url": p.7, "icon_url": p.8,
            "is_joined": is_joined, "relevance_score": (score * 100.0) as i32
        });
        (score, val)
    }).collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let results: Vec<Value> = scored.into_iter().take(limit as usize).map(|(_, v)| v).collect();

    Ok(Json(json!({ "playgrounds": results })))
}

/// POST /playgrounds — create a new playground
pub async fn create_playground(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let name = payload["name"].as_str().unwrap_or("").to_string();
    if name.is_empty() { return Err(AppError::bad_request("Missing 'name'")); }
    let description = payload["description"].as_str().map(|s| s.to_string());
    let pg_type = payload["type"].as_str().unwrap_or("interest").to_string();
    let city = payload["city"].as_str().map(|s| s.to_string());
    let max_members = payload["max_members"].as_i64().unwrap_or(50) as i32;
    let banner_url = extract_banner_url(&state, user_id, &payload, "playground").await?;

    let id = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO playgrounds (name, description, playground_type, city, max_members, is_public, is_active, banner_url)
           VALUES ($1, $2, $3, $4, $5, true, true, $6) RETURNING id"#,
    )
    .bind(&name).bind(&description).bind(&pg_type).bind(&city).bind(max_members).bind(&banner_url)
    .fetch_one(&state.db).await?;

    // Creator auto-joins as admin
    sqlx::query("INSERT INTO playground_members (playground_id, user_id, role) VALUES ($1, $2, 'admin')")
        .bind(id).bind(user_id).execute(&state.db).await?;

    sqlx::query("UPDATE playgrounds SET member_count = 1 WHERE id = $1")
        .bind(id).execute(&state.db).await?;

    Ok(Json(json!({ "playground_id": id.to_string() })))
}

/// GET /playgrounds/:id
pub async fn get_playground_detail(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(pg_id): AxumPath<i64>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let pg = sqlx::query_as::<_, (i64, String, Option<String>, String, Option<String>, i32, i32, bool, Option<String>, Option<i32>)>(
        "SELECT id, name, description, playground_type, city, member_count, active_today, is_public, banner_url, max_members FROM playgrounds WHERE id = $1"
    ).bind(pg_id).fetch_optional(&state.db).await?
    .ok_or_else(|| AppError::not_found("Playground not found"))?;

    let is_member = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM playground_members WHERE playground_id = $1 AND user_id = $2 AND is_active = true)"
    ).bind(pg_id).bind(user_id).fetch_one(&state.db).await.unwrap_or(false);

    Ok(Json(json!({
        "id": pg.0.to_string(), "name": pg.1, "description": pg.2, "type": pg.3, "city": pg.4,
        "member_count": pg.5, "max_members": pg.9, "active_today": pg.6, "is_public": pg.7,
        "is_member": is_member, "banner_url": pg.8
    })))
}

/// POST /playgrounds/:id/join
pub async fn join_playground(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(pg_id): AxumPath<i64>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Enforce max_members cap (if set). Skip check for users who are
    // already members being re-activated (ON CONFLICT path).
    let (member_count, max_members): (i32, Option<i32>) = sqlx::query_as(
        "SELECT member_count, max_members FROM playgrounds WHERE id = $1"
    ).bind(pg_id).fetch_optional(&state.db).await?
        .ok_or_else(|| AppError::not_found("Playground not found"))?;

    let already_member = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM playground_members WHERE playground_id = $1 AND user_id = $2)"
    ).bind(pg_id).bind(user_id).fetch_one(&state.db).await.unwrap_or(false);

    if !already_member {
        if let Some(cap) = max_members {
            if member_count >= cap {
                return Err(AppError::bad_request("This playground is full"));
            }
        }
    }

    sqlx::query(
        "INSERT INTO playground_members (playground_id, user_id, role) VALUES ($1, $2, 'member') ON CONFLICT (playground_id, user_id) DO UPDATE SET is_active = true, last_active_at = NOW()"
    ).bind(pg_id).bind(user_id).execute(&state.db).await?;

    sqlx::query("UPDATE playgrounds SET member_count = (SELECT COUNT(*) FROM playground_members WHERE playground_id = $1 AND is_active = true) WHERE id = $1")
        .bind(pg_id).execute(&state.db).await?;

    Ok(Json(json!({ "joined": true })))
}

/// POST /playgrounds/:id/leave
pub async fn leave_playground(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(pg_id): AxumPath<i64>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    sqlx::query("UPDATE playground_members SET is_active = false WHERE playground_id = $1 AND user_id = $2")
        .bind(pg_id).bind(user_id).execute(&state.db).await?;

    sqlx::query("UPDATE playgrounds SET member_count = (SELECT COUNT(*) FROM playground_members WHERE playground_id = $1 AND is_active = true) WHERE id = $1")
        .bind(pg_id).execute(&state.db).await?;

    Ok(Json(json!({ "left": true })))
}

/// GET /playgrounds/:id/members
pub async fn get_playground_members(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(pg_id): AxumPath<i64>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _user_id = decode_access_token(&token, &state.config.secret_key)?;

    let members = sqlx::query_as::<_, (i64, Option<String>, Option<String>, String, Option<chrono::NaiveDateTime>)>(
        r#"SELECT u.id, u.name, u.profile_photo_1, pm.role, pm.last_active_at
           FROM playground_members pm JOIN users u ON u.id = pm.user_id
           WHERE pm.playground_id = $1 AND pm.is_active = true
           ORDER BY pm.role DESC, pm.joined_at ASC LIMIT 100"#,
    ).bind(pg_id).fetch_all(&state.db).await?;

    let results: Vec<Value> = members.into_iter().map(|m| {
        json!({ "user_id": m.0.to_string(), "name": m.1, "photo": m.2, "role": m.3, "last_active": m.4.map(format_datetime) })
    }).collect();

    Ok(Json(json!({ "members": results })))
}

/// GET /playgrounds/:id/messages?before=ISO8601&limit=N
/// Members-only. Returns ASC (oldest first) for natural chat rendering.
pub async fn get_playground_messages(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(pg_id): AxumPath<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Member check
    let is_member = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM playground_members WHERE playground_id = $1 AND user_id = $2 AND is_active = true)"
    ).bind(pg_id).bind(user_id).fetch_one(&state.db).await.unwrap_or(false);
    if !is_member {
        return Err(AppError::forbidden("Not a member of this playground"));
    }

    let limit: i64 = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50).clamp(1, 200);
    // 'before' cursor — return messages strictly older than this timestamp.
    let before = params.get("before").and_then(|s|
        chrono::DateTime::parse_from_rfc3339(s).ok().map(|dt| dt.naive_utc())
            .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok())
            .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok())
    );

    // Query newest-first to respect the 'before' cursor + limit, then reverse for ASC output.
    let rows: Vec<(i64, i64, i64, String, chrono::NaiveDateTime, Option<String>, Option<String>)> = match before {
        Some(cutoff) => sqlx::query_as(
            r#"SELECT m.id, m.playground_id, m.sender_id, m.content, m.created_at,
                      u.name as sender_name, u.profile_photo_1 as sender_photo
               FROM playground_messages m
               JOIN users u ON u.id = m.sender_id
               WHERE m.playground_id = $1 AND m.created_at < $2
               ORDER BY m.created_at DESC LIMIT $3"#
        ).bind(pg_id).bind(cutoff).bind(limit).fetch_all(&state.db).await?,
        None => sqlx::query_as(
            r#"SELECT m.id, m.playground_id, m.sender_id, m.content, m.created_at,
                      u.name as sender_name, u.profile_photo_1 as sender_photo
               FROM playground_messages m
               JOIN users u ON u.id = m.sender_id
               WHERE m.playground_id = $1
               ORDER BY m.created_at DESC LIMIT $2"#
        ).bind(pg_id).bind(limit).fetch_all(&state.db).await?,
    };

    // Query is DESC (newest-first) so 'before' + limit work as a cursor;
    // reverse once here to emit ASC (oldest-first) for chat rendering.
    let messages: Vec<Value> = rows.into_iter().rev().map(|r| json!({
        "id": r.0.to_string(),
        "playground_id": r.1.to_string(),
        "sender_id": r.2.to_string(),
        "sender_name": r.5,
        "sender_photo": r.6,
        "content": r.3,
        "created_at": format_datetime(r.4),
    })).collect();

    Ok(Json(json!({ "messages": messages })))
}

/// POST /playgrounds/:id/messages
#[derive(Deserialize)]
pub struct SendPlaygroundMessagePayload { pub content: String }

pub async fn send_playground_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(pg_id): AxumPath<i64>,
    Json(payload): Json<SendPlaygroundMessagePayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let trimmed = payload.content.trim();
    if trimmed.is_empty() {
        return Err(AppError::bad_request("content required"));
    }
    if trimmed.chars().count() > 2000 {
        return Err(AppError::bad_request("content must be 2000 characters or less"));
    }

    // Member check
    let is_member = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM playground_members WHERE playground_id = $1 AND user_id = $2 AND is_active = true)"
    ).bind(pg_id).bind(user_id).fetch_one(&state.db).await.unwrap_or(false);
    if !is_member {
        return Err(AppError::forbidden("Not a member of this playground"));
    }

    // Insert + fetch joined row for response
    let row = sqlx::query_as::<_, (i64, i64, i64, String, chrono::NaiveDateTime, Option<String>, Option<String>)>(
        r#"WITH inserted AS (
             INSERT INTO playground_messages (playground_id, sender_id, content)
             VALUES ($1, $2, $3)
             RETURNING id, playground_id, sender_id, content, created_at
           )
           SELECT i.id, i.playground_id, i.sender_id, i.content, i.created_at,
                  u.name as sender_name, u.profile_photo_1 as sender_photo
           FROM inserted i JOIN users u ON u.id = i.sender_id"#
    )
    .bind(pg_id).bind(user_id).bind(trimmed)
    .fetch_one(&state.db).await?;

    // Bump playground activity for ranking / member_count is unchanged.
    let _ = sqlx::query("UPDATE playgrounds SET updated_at = NOW() WHERE id = $1")
        .bind(pg_id).execute(&state.db).await;

    let message = json!({
        "id": row.0.to_string(),
        "playground_id": row.1.to_string(),
        "sender_id": row.2.to_string(),
        "sender_name": row.5,
        "sender_photo": row.6,
        "content": row.3,
        "created_at": format_datetime(row.4),
    });

    // Realtime fanout: publish to every active member's /ws/events channel.
    // Uses the durable outbox, so offline members receive it on reconnect.
    // Fire-and-forget in a background task — never blocks the POST response.
    {
        let state_clone = state.clone();
        let msg_payload = message.clone();
        let pg_id_str = pg_id.to_string();
        tokio::spawn(async move {
            let member_ids: Vec<i32> = sqlx::query_scalar(
                "SELECT user_id FROM playground_members WHERE playground_id = $1 AND is_active = true"
            )
            .bind(pg_id)
            .fetch_all(&state_clone.db)
            .await
            .unwrap_or_default();

            for uid in member_ids {
                let evt = json!({
                    "playground_id": pg_id_str,
                    "message": msg_payload.clone(),
                });
                publish_user_event(&state_clone, uid, "playground_message", evt).await;
            }
        });
    }

    Ok(Json(json!({ "message": message })))
}

// ============================================================================
// Events (Real-World Meetups)
// ============================================================================

/// POST /events — create a new event
pub async fn create_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let title = payload["title"].as_str().unwrap_or("").to_string();
    if title.is_empty() { return Err(AppError::bad_request("Missing 'title'")); }
    let description = payload["description"].as_str().map(|s| s.to_string());
    let category = payload["category"].as_str().map(|s| s.to_string());
    let location_name = payload["location_name"].as_str().map(|s| s.to_string());
    let latitude = payload["latitude"].as_f64();
    let longitude = payload["longitude"].as_f64();
    let starts_at = payload["starts_at"].as_str()
        .and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok())
        .ok_or_else(|| AppError::bad_request("Missing or invalid 'starts_at' (format: YYYY-MM-DDTHH:MM:SS)"))?;
    let max_attendees = payload["max_attendees"].as_i64().map(|v| v as i32);
    let banner_url = extract_banner_url(&state, user_id, &payload, "event").await?;

    let id = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO events (creator_id, title, description, category, location_name, latitude, longitude, starts_at, max_attendees, banner_url)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10) RETURNING id"#,
    )
    .bind(user_id).bind(&title).bind(&description).bind(&category)
    .bind(&location_name).bind(latitude).bind(longitude).bind(starts_at).bind(max_attendees).bind(&banner_url)
    .fetch_one(&state.db).await?;

    // Creator auto-RSVPs
    sqlx::query("INSERT INTO event_rsvps (event_id, user_id, status) VALUES ($1, $2, 'going')")
        .bind(id).bind(user_id).execute(&state.db).await?;

    Ok(Json(json!({ "event_id": id.to_string() })))
}

/// GET /events — ML-ranked: nearby + interest match + friends going + urgency
pub async fn get_events_near_me(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let limit = params.get("limit").and_then(|v| v.parse::<i64>().ok()).unwrap_or(20);

    // Get user location + interests in parallel
    let (user_loc, user_interests) = tokio::try_join!(
        sqlx::query_as::<_, (Option<f64>, Option<f64>, Option<String>)>(
            "SELECT latitude, longitude, city FROM user_locations WHERE user_id = $1"
        ).bind(user_id).fetch_optional(&state.db),
        sqlx::query_scalar::<_, Option<serde_json::Value>>(
            "SELECT interests FROM users WHERE id = $1"
        ).bind(user_id).fetch_optional(&state.db),
    )?;

    let (user_lat, user_lng) = user_loc.as_ref()
        .and_then(|l| l.0.zip(l.1))
        .unwrap_or((0.0, 0.0));
    let interests: Vec<String> = user_interests.flatten()
        .and_then(|v| serde_json::from_value(v).ok())
        .unwrap_or_default();

    // Get events with RSVP counts + whether user's matches are going
    let events = sqlx::query_as::<_, (i64, i64, String, Option<String>, Option<String>, Option<String>, Option<f64>, Option<f64>, chrono::NaiveDateTime, Option<i32>, i64, i64, Option<String>)>(
        r#"SELECT e.id, e.creator_id, e.title, e.description, e.category, e.location_name,
                  e.latitude, e.longitude, e.starts_at, e.max_attendees,
                  (SELECT COUNT(*) FROM event_rsvps WHERE event_id = e.id) as rsvp_count,
                  (SELECT COUNT(*) FROM event_rsvps er
                   JOIN matches m ON (m.user1_id = $1 AND m.user2_id = er.user_id) OR (m.user2_id = $1 AND m.user1_id = er.user_id)
                   WHERE er.event_id = e.id AND m.is_mutual_match = true) as friends_going,
                  e.banner_url
           FROM events e WHERE e.is_active = true AND e.starts_at > NOW()
           ORDER BY e.starts_at ASC LIMIT $2"#,
    ).bind(user_id).bind(limit * 3).fetch_all(&state.db).await?;

    // Score and rank
    let mut scored: Vec<(f64, Value)> = events.into_iter().map(|e| {
        let mut score = 0.0;

        // Distance score (+30%) — closer events rank higher
        if let (Some(lat), Some(lng)) = (e.6, e.7) {
            if user_lat != 0.0 {
                let dist = haversine_km(user_lat, user_lng, lat, lng);
                score += 0.3 * (1.0 / (1.0 + dist / 10.0)); // 10km half-life
            }
        }

        // Friends going bonus (+25%) — social proof
        score += 0.25 * (e.11 as f64 / (e.11 as f64 + 2.0));

        // Interest/category match (+20%)
        if let Some(ref cat) = e.4 {
            if interests.iter().any(|i| i.to_lowercase() == cat.to_lowercase()) {
                score += 0.2;
            }
        }

        // Urgency bonus (+15%) — events happening sooner rank higher
        let hours_until = (e.8 - chrono::Utc::now().naive_utc()).num_hours() as f64;
        score += 0.15 * (1.0 / (1.0 + hours_until / 24.0)); // 24h half-life

        // Popularity bonus (+10%)
        score += 0.1 * (e.10 as f64 / (e.10 as f64 + 5.0));

        let val = json!({
            "id": e.0.to_string(), "creator_id": e.1.to_string(), "title": e.2, "description": e.3,
            "category": e.4, "location_name": e.5, "latitude": e.6, "longitude": e.7,
            "starts_at": format_datetime(e.8), "max_attendees": e.9,
            "rsvp_count": e.10, "friends_going": e.11,
            "banner_url": e.12,
            "relevance_score": (score * 100.0) as i32
        });
        (score, val)
    }).collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let results: Vec<Value> = scored.into_iter().take(limit as usize).map(|(_, v)| v).collect();

    Ok(Json(json!({ "events": results })))
}

/// POST /events/:id/rsvp
pub async fn rsvp_event(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(event_id): AxumPath<i64>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let status = "going";
    sqlx::query(
        "INSERT INTO event_rsvps (event_id, user_id, status) VALUES ($1, $2, $3) ON CONFLICT (event_id, user_id) DO UPDATE SET status = $3"
    ).bind(event_id).bind(user_id).bind(status).execute(&state.db).await?;

    Ok(Json(json!({ "rsvp": status })))
}

// ============================================================================
// Music Taste Sync & Matching
// ============================================================================

/// POST /music/sync — sync user's music library (Apple Music / Spotify)
pub async fn sync_music_taste(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let source = payload["source"].as_str().unwrap_or("apple_music");
    let tracks = payload["tracks"].as_array()
        .ok_or_else(|| AppError::bad_request("Missing 'tracks' array"))?;

    let mut synced = 0i64;
    for track in tracks {
        let track_id = track["id"].as_str().unwrap_or("").to_string();
        let track_name = track["name"].as_str().map(|s| s.to_string());
        let artist = track["artist"].as_str().map(|s| s.to_string());
        let album = track["album"].as_str().map(|s| s.to_string());
        let genre = track["genre"].as_str().map(|s| s.to_string());
        let play_count = track["play_count"].as_i64().unwrap_or(1) as i32;

        if track_id.is_empty() { continue; }

        let _ = sqlx::query(
            r#"INSERT INTO user_music_taste (user_id, source, track_id, track_name, artist_name, album_name, genre, play_count, synced_at)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, NOW())
               ON CONFLICT (user_id, source, track_id) DO UPDATE SET
                   play_count = EXCLUDED.play_count, track_name = COALESCE(EXCLUDED.track_name, user_music_taste.track_name),
                   artist_name = COALESCE(EXCLUDED.artist_name, user_music_taste.artist_name), synced_at = NOW()"#,
        )
        .bind(user_id).bind(source).bind(&track_id).bind(&track_name)
        .bind(&artist).bind(&album).bind(&genre).bind(play_count)
        .execute(&state.db).await?;

        // Update genre profile
        if let Some(ref g) = genre {
            let _ = sqlx::query(
                r#"INSERT INTO user_genre_profile (user_id, genre, weight, track_count, updated_at)
                   VALUES ($1, $2, $3, 1, NOW())
                   ON CONFLICT (user_id, genre) DO UPDATE SET
                       weight = user_genre_profile.weight + $3, track_count = user_genre_profile.track_count + 1, updated_at = NOW()"#,
            ).bind(user_id).bind(g).bind(play_count as f64).execute(&state.db).await?;
        }
        synced += 1;
    }

    Ok(Json(json!({ "synced": synced })))
}

/// GET /music/taste — get user's top genres and artists
pub async fn get_music_taste(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let genres = sqlx::query_as::<_, (String, f64, i32)>(
        "SELECT genre, weight, track_count FROM user_genre_profile WHERE user_id = $1 ORDER BY weight DESC LIMIT 10"
    ).bind(user_id).fetch_all(&state.db).await?;

    let top_artists = sqlx::query_as::<_, (String, i64)>(
        "SELECT artist_name, SUM(play_count) as total FROM user_music_taste WHERE user_id = $1 AND artist_name IS NOT NULL GROUP BY artist_name ORDER BY total DESC LIMIT 10"
    ).bind(user_id).fetch_all(&state.db).await?;

    let genre_list: Vec<Value> = genres.into_iter().map(|g| json!({ "genre": g.0, "weight": g.1, "tracks": g.2 })).collect();
    let artist_list: Vec<Value> = top_artists.into_iter().map(|a| json!({ "artist": a.0, "plays": a.1 })).collect();

    Ok(Json(json!({ "genres": genre_list, "top_artists": artist_list })))
}

/// GET /music/compatibility/:target_id — music taste overlap with another user
pub async fn get_music_compatibility(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(target_id): AxumPath<i64>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Genre overlap (Jaccard-like)
    let overlap = sqlx::query_as::<_, (i64, i64, i64)>(
        r#"WITH my_genres AS (SELECT genre FROM user_genre_profile WHERE user_id = $1),
              their_genres AS (SELECT genre FROM user_genre_profile WHERE user_id = $2),
              shared AS (SELECT genre FROM my_genres INTERSECT SELECT genre FROM their_genres)
           SELECT (SELECT COUNT(*) FROM shared), (SELECT COUNT(*) FROM my_genres), (SELECT COUNT(*) FROM their_genres)"#,
    ).bind(user_id).bind(target_id).fetch_one(&state.db).await?;

    let shared = overlap.0 as f64;
    let total = (overlap.1 + overlap.2) as f64 - shared;
    let score = if total > 0.0 { (shared / total * 100.0) as i32 } else { 0 };

    // Shared artists
    let shared_artists = sqlx::query_as::<_, (String,)>(
        r#"SELECT DISTINCT a.artist_name FROM user_music_taste a
           JOIN user_music_taste b ON a.artist_name = b.artist_name
           WHERE a.user_id = $1 AND b.user_id = $2 AND a.artist_name IS NOT NULL LIMIT 10"#,
    ).bind(user_id).bind(target_id).fetch_all(&state.db).await?;

    let artists: Vec<String> = shared_artists.into_iter().map(|a| a.0).collect();

    Ok(Json(json!({ "music_compatibility": score, "shared_artists": artists })))
}

// ============================================================================
// ============================================================================
// Now Playing / Listening History — captures music from ANY app
// ============================================================================

/// POST /music/now-playing — iOS sends currently playing track (from any app)
pub async fn track_now_playing(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let track_name = payload["track"].as_str().unwrap_or("").to_string();
    let artist = payload["artist"].as_str().unwrap_or("").to_string();
    if track_name.is_empty() || artist.is_empty() { return Ok(Json(json!({ "ok": false }))); }

    let album = payload["album"].as_str().map(|s| s.to_string());
    let genre = payload["genre"].as_str().map(|s| s.to_string());
    let source = payload["source"].as_str().unwrap_or("now_playing");
    let duration = payload["duration_sec"].as_i64().map(|v| v as i32);
    let session_dur = payload["session_duration_sec"].as_i64().map(|v| v as i32);
    let completed = payload["completed"].as_bool().unwrap_or(false);

    // Save to listening history
    sqlx::query(
        r#"INSERT INTO user_listening_history (user_id, source, track_name, artist_name, album_name, genre, duration_sec, session_duration_sec, completed)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)"#,
    )
    .bind(user_id).bind(source).bind(&track_name).bind(&artist)
    .bind(&album).bind(&genre).bind(duration).bind(session_dur).bind(completed)
    .execute(&state.db).await?;

    // Update engagement profile (weighted by completion + listen time)
    let listen_sec = session_dur.unwrap_or(duration.unwrap_or(0));
    let engagement = if completed { 1.0 } else { (listen_sec as f64 / duration.unwrap_or(200) as f64).min(1.0) };

    sqlx::query(
        r#"INSERT INTO user_music_engagement (user_id, artist_name, genre, listen_count, total_listen_sec, completion_rate, engagement_score, last_listened_at)
           VALUES ($1, $2, $3, 1, $4, $5, $5, NOW())
           ON CONFLICT (user_id, artist_name) DO UPDATE SET
               listen_count = user_music_engagement.listen_count + 1,
               total_listen_sec = user_music_engagement.total_listen_sec + $4,
               skip_count = CASE WHEN $6 THEN user_music_engagement.skip_count ELSE user_music_engagement.skip_count + 1 END,
               completion_rate = (user_music_engagement.completion_rate * user_music_engagement.listen_count + $5) / (user_music_engagement.listen_count + 1),
               engagement_score = (user_music_engagement.engagement_score * 0.9) + ($5 * 0.1),
               genre = COALESCE($3, user_music_engagement.genre),
               last_listened_at = NOW()"#,
    )
    .bind(user_id).bind(&artist).bind(&genre).bind(listen_sec)
    .bind(engagement).bind(completed)
    .execute(&state.db).await?;

    // Also update genre profile
    if let Some(ref g) = genre {
        sqlx::query(
            r#"INSERT INTO user_genre_profile (user_id, genre, weight, track_count, updated_at)
               VALUES ($1, $2, $3, 1, NOW())
               ON CONFLICT (user_id, genre) DO UPDATE SET
                   weight = user_genre_profile.weight + $3, track_count = user_genre_profile.track_count + 1, updated_at = NOW()"#,
        ).bind(user_id).bind(g).bind(engagement).execute(&state.db).await?;
    }

    Ok(Json(json!({ "ok": true })))
}

/// GET /music/engagement — user's actual listening behavior (not just library)
pub async fn get_music_engagement(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Top artists by engagement (listen time + completion rate)
    let artists = sqlx::query_as::<_, (String, Option<String>, i32, i32, f64, f64)>(
        r#"SELECT artist_name, genre, listen_count, total_listen_sec, completion_rate, engagement_score
           FROM user_music_engagement WHERE user_id = $1
           ORDER BY engagement_score DESC LIMIT 20"#,
    ).bind(user_id).fetch_all(&state.db).await?;

    let results: Vec<Value> = artists.into_iter().map(|a| json!({
        "artist": a.0, "genre": a.1, "listens": a.2,
        "total_minutes": a.3 / 60, "completion_rate": (a.4 * 100.0) as i32,
        "engagement_score": (a.5 * 100.0) as i32
    })).collect();

    // Recent listening (last 24h)
    let recent = sqlx::query_as::<_, (String, String, Option<String>, String)>(
        r#"SELECT track_name, artist_name, genre, source FROM user_listening_history
           WHERE user_id = $1 AND listened_at > NOW() - INTERVAL '24 hours'
           ORDER BY listened_at DESC LIMIT 10"#,
    ).bind(user_id).fetch_all(&state.db).await?;

    let recent_list: Vec<Value> = recent.into_iter().map(|r| json!({
        "track": r.0, "artist": r.1, "genre": r.2, "source": r.3
    })).collect();

    Ok(Json(json!({ "top_artists": results, "recently_played": recent_list })))
}

/// GET /music/compatibility-deep/:target_id — engagement-weighted music compatibility
pub async fn get_deep_music_compatibility(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(target_id): AxumPath<i64>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Shared artists weighted by engagement (not just library overlap)
    let shared = sqlx::query_as::<_, (String, Option<String>, f64, f64)>(
        r#"SELECT a.artist_name, a.genre,
                  a.engagement_score as my_score, b.engagement_score as their_score
           FROM user_music_engagement a
           JOIN user_music_engagement b ON a.artist_name = b.artist_name
           WHERE a.user_id = $1 AND b.user_id = $2
           ORDER BY (a.engagement_score + b.engagement_score) DESC LIMIT 15"#,
    ).bind(user_id).bind(target_id).fetch_all(&state.db).await?;

    // Genre overlap weighted by listening time
    let genre_overlap = sqlx::query_as::<_, (String, f64, f64)>(
        r#"SELECT a.genre, a.weight as my_weight, b.weight as their_weight
           FROM user_genre_profile a
           JOIN user_genre_profile b ON a.genre = b.genre
           WHERE a.user_id = $1 AND b.user_id = $2
           ORDER BY (a.weight + b.weight) DESC LIMIT 10"#,
    ).bind(user_id).bind(target_id).fetch_all(&state.db).await?;

    // Calculate weighted score
    let artist_score: f64 = shared.iter()
        .map(|s| (s.2.min(1.0) + s.3.min(1.0)) / 2.0)
        .sum::<f64>() / shared.len().max(1) as f64;

    let genre_score: f64 = genre_overlap.iter()
        .map(|g| (g.1.min(100.0) + g.2.min(100.0)) / 200.0)
        .sum::<f64>() / genre_overlap.len().max(1) as f64;

    // 60% artist engagement + 40% genre overlap
    let compatibility = ((0.6 * artist_score + 0.4 * genre_score) * 100.0) as i32;

    let shared_list: Vec<Value> = shared.into_iter().map(|s| json!({
        "artist": s.0, "genre": s.1,
        "my_engagement": (s.2 * 100.0) as i32,
        "their_engagement": (s.3 * 100.0) as i32
    })).collect();

    let genre_list: Vec<Value> = genre_overlap.into_iter().map(|g| json!({
        "genre": g.0, "my_weight": g.1 as i32, "their_weight": g.2 as i32
    })).collect();

    Ok(Json(json!({
        "deep_compatibility": compatibility,
        "shared_artists": shared_list,
        "shared_genres": genre_list,
        "data_source": "engagement_weighted"
    })))
}

/// POST /accounts/connect — link Spotify/Instagram/YouTube account
pub async fn connect_account(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let platform = payload["platform"].as_str().unwrap_or("").to_string();
    if platform.is_empty() { return Err(AppError::bad_request("Missing 'platform'")); }
    let platform_user_id = payload["platform_user_id"].as_str().map(|s| s.to_string());
    let access_token = payload["access_token"].as_str().map(|s| s.to_string());
    let refresh_token = payload["refresh_token"].as_str().map(|s| s.to_string());

    sqlx::query(
        r#"INSERT INTO user_connected_accounts (user_id, platform, platform_user_id, access_token, refresh_token)
           VALUES ($1, $2, $3, $4, $5)
           ON CONFLICT (user_id, platform) DO UPDATE SET
               platform_user_id = COALESCE(EXCLUDED.platform_user_id, user_connected_accounts.platform_user_id),
               access_token = COALESCE(EXCLUDED.access_token, user_connected_accounts.access_token),
               refresh_token = COALESCE(EXCLUDED.refresh_token, user_connected_accounts.refresh_token),
               is_active = true, connected_at = NOW()"#,
    )
    .bind(user_id).bind(&platform).bind(&platform_user_id)
    .bind(&access_token).bind(&refresh_token)
    .execute(&state.db).await?;

    Ok(Json(json!({ "connected": true, "platform": platform })))
}

/// GET /accounts/connected — list user's connected accounts
pub async fn get_connected_accounts(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let accounts = sqlx::query_as::<_, (String, Option<String>, bool)>(
        "SELECT platform, platform_user_id, is_active FROM user_connected_accounts WHERE user_id = $1"
    ).bind(user_id).fetch_all(&state.db).await?;

    let results: Vec<Value> = accounts.into_iter().map(|a| json!({
        "platform": a.0, "platform_user_id": a.1, "is_active": a.2
    })).collect();

    Ok(Json(json!({ "accounts": results })))
}

// ============================================================================
// ============================================================================
// Fitness Tracking (HealthKit / Whoop / Garmin via HealthKit)
// ============================================================================

/// POST /fitness/sync — sync workouts from HealthKit
pub async fn sync_fitness(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let activities = payload["activities"].as_array()
        .ok_or_else(|| AppError::bad_request("Missing 'activities' array"))?;

    let mut synced = 0i64;
    let mut total_cal = 0.0f64;
    let mut total_min = 0.0f64;
    let mut total_dist = 0.0f64;

    for a in activities {
        let activity_type = a["type"].as_str().unwrap_or("workout").to_string();
        let calories = a["calories"].as_f64();
        let duration = a["duration_min"].as_f64();
        let distance = a["distance_km"].as_f64();
        let elevation = a["elevation_gain_m"].as_f64();
        let heart_rate = a["avg_heart_rate"].as_i64().map(|v| v as i32);
        let location_name = a["location_name"].as_str().map(|s| s.to_string());
        let latitude = a["latitude"].as_f64();
        let longitude = a["longitude"].as_f64();
        let started_at = a["started_at"].as_str()
            .and_then(|s| chrono::NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok())
            .unwrap_or_else(|| chrono::Utc::now().naive_utc());
        let source = a["source"].as_str().unwrap_or("healthkit");

        sqlx::query(
            r#"INSERT INTO user_fitness_activities
               (user_id, activity_type, calories_burned, duration_min, distance_km, elevation_gain_m,
                avg_heart_rate, location_name, latitude, longitude, started_at, source)
               VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)"#,
        )
        .bind(user_id).bind(&activity_type).bind(calories).bind(duration)
        .bind(distance).bind(elevation).bind(heart_rate).bind(&location_name)
        .bind(latitude).bind(longitude).bind(started_at).bind(source)
        .execute(&state.db).await?;

        total_cal += calories.unwrap_or(0.0);
        total_min += duration.unwrap_or(0.0);
        total_dist += distance.unwrap_or(0.0);
        synced += 1;
    }

    // Update aggregated fitness profile
    sqlx::query(
        r#"INSERT INTO user_fitness_profile (user_id, weekly_active_minutes, weekly_calories, weekly_workouts, total_distance_km, last_workout_at, updated_at)
           VALUES ($1, $2, $3, $4, $5, NOW(), NOW())
           ON CONFLICT (user_id) DO UPDATE SET
               weekly_active_minutes = (SELECT COALESCE(SUM(duration_min), 0)::int FROM user_fitness_activities WHERE user_id = $1 AND started_at > NOW() - INTERVAL '7 days'),
               weekly_calories = (SELECT COALESCE(SUM(calories_burned), 0)::int FROM user_fitness_activities WHERE user_id = $1 AND started_at > NOW() - INTERVAL '7 days'),
               weekly_workouts = (SELECT COUNT(*) FROM user_fitness_activities WHERE user_id = $1 AND started_at > NOW() - INTERVAL '7 days'),
               total_distance_km = user_fitness_profile.total_distance_km + $5,
               favorite_activity = (SELECT activity_type FROM user_fitness_activities WHERE user_id = $1 GROUP BY activity_type ORDER BY COUNT(*) DESC LIMIT 1),
               last_workout_at = NOW(), updated_at = NOW()"#,
    )
    .bind(user_id).bind(total_min as i32).bind(total_cal as i32)
    .bind(synced as i32).bind(total_dist)
    .execute(&state.db).await?;

    Ok(Json(json!({ "synced": synced, "total_calories": total_cal as i32, "total_minutes": total_min as i32 })))
}

/// GET /fitness/profile — user's fitness summary for dating profile
pub async fn get_fitness_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _user_id = decode_access_token(&token, &state.config.secret_key)? as i64;
    let target_id: i64 = params.get("user_id").and_then(|v| v.parse().ok()).unwrap_or(_user_id);

    // Check privacy
    let share = sqlx::query_scalar::<_, Option<bool>>(
        "SELECT share_fitness FROM users WHERE id = $1"
    ).bind(target_id).fetch_one(&state.db).await?;

    if target_id != _user_id && !share.unwrap_or(false) {
        return Ok(Json(json!({ "fitness": null, "private": true })));
    }

    let profile = sqlx::query_as::<_, (i32, i32, i32, f64, Option<String>, Option<String>, i32)>(
        r#"SELECT weekly_active_minutes, weekly_calories, weekly_workouts, total_distance_km,
                  favorite_activity, fitness_level, streak_days
           FROM user_fitness_profile WHERE user_id = $1"#,
    ).bind(target_id).fetch_optional(&state.db).await?;

    let recent = sqlx::query_as::<_, (String, Option<f64>, Option<f64>, Option<f64>, Option<String>, chrono::NaiveDateTime)>(
        r#"SELECT activity_type, calories_burned, duration_min, distance_km, location_name, started_at
           FROM user_fitness_activities WHERE user_id = $1
           ORDER BY started_at DESC LIMIT 5"#,
    ).bind(target_id).fetch_all(&state.db).await?;

    let recent_list: Vec<Value> = recent.into_iter().map(|r| json!({
        "type": r.0, "calories": r.1.map(|v| v as i32), "duration_min": r.2.map(|v| v as i32),
        "distance_km": r.3.map(|v| format!("{:.1}", v)), "location": r.4, "date": format_datetime(r.5)
    })).collect();

    match profile {
        Some(p) => Ok(Json(json!({
            "weekly_active_minutes": p.0, "weekly_calories": p.1, "weekly_workouts": p.2,
            "total_distance_km": format!("{:.1}", p.3), "favorite_activity": p.4,
            "fitness_level": p.5, "streak_days": p.6, "recent_activities": recent_list
        }))),
        None => Ok(Json(json!({ "fitness": null, "no_data": true })))
    }
}

/// GET /fitness/stats — self stats in the field names iOS expects.
/// Never 404s for new users — returns zeros. Also derives a fitness_score in 0..100.
pub async fn get_fitness_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)? as i64;

    let row = sqlx::query_as::<_, (Option<i32>, Option<i32>, Option<i32>, Option<i32>)>(
        r#"SELECT weekly_active_minutes, weekly_calories, weekly_workouts, streak_days
           FROM user_fitness_profile WHERE user_id = $1"#,
    ).bind(user_id).fetch_optional(&state.db).await?;

    let (wam, wcal, wwk, streak) = row.map(|r| (r.0.unwrap_or(0), r.1.unwrap_or(0), r.2.unwrap_or(0), r.3.unwrap_or(0)))
        .unwrap_or((0, 0, 0, 0));

    // fitness_score in 0..100, each component capped at 1.0 then weighted.
    let active_part   = (wam as f64 / 150.0).min(1.0)  * 30.0; // WHO: 150 min/week
    let workouts_part = (wwk as f64 / 5.0).min(1.0)    * 20.0; // 5 sessions/week
    let streak_part   = (streak as f64 / 30.0).min(1.0) * 30.0; // 30-day streak
    let cal_part      = (wcal as f64 / 3500.0).min(1.0) * 20.0;
    let fitness_score = (active_part + workouts_part + streak_part + cal_part).round() as i32;

    Ok(Json(json!({
        "weekly_calories": wcal,
        "weekly_active_minutes": wam,
        "weekly_workout_count": wwk,
        "current_streak": streak,
        "fitness_score": fitness_score
    })))
}

/// GET /fitness/workouts?limit=&offset=
pub async fn get_fitness_workouts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)? as i64;
    let limit: i64 = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(20).clamp(1, 100);
    let offset: i64 = params.get("offset").and_then(|v| v.parse().ok()).unwrap_or(0).max(0);

    let rows = sqlx::query_as::<_, (
        i64, String, Option<f64>, Option<f64>, Option<f64>, Option<String>,
        chrono::NaiveDateTime, Option<String>, Option<f64>, Option<f64>,
    )>(
        r#"SELECT id, activity_type, calories_burned, duration_min, distance_km, location_name,
                  started_at, source, latitude, longitude
           FROM user_fitness_activities WHERE user_id = $1
           ORDER BY started_at DESC LIMIT $2 OFFSET $3"#,
    ).bind(user_id).bind(limit).bind(offset).fetch_all(&state.db).await?;

    let workouts: Vec<Value> = rows.into_iter().map(|r| json!({
        "id": r.0,
        "activity_type": r.1,
        "calories": r.2.map(|v| v as i32).unwrap_or(0),
        "duration_min": r.3.map(|v| v as i32).unwrap_or(0),
        "distance_km": r.4.unwrap_or(0.0),
        "location": r.5,
        "started_at": format_datetime(r.6),
        "source": r.7,
        "latitude": r.8,
        "longitude": r.9,
    })).collect();

    Ok(Json(json!({ "workouts": workouts, "limit": limit, "offset": offset })))
}

/// GET /fitness/goals
pub async fn get_fitness_goals(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)? as i64;

    let goals = sqlx::query_as::<_, (i64, String, f64, String, String, bool)>(
        "SELECT id, goal_type, target_value, unit, period, is_active FROM user_fitness_goals WHERE user_id = $1 ORDER BY created_at DESC"
    ).bind(user_id).fetch_all(&state.db).await.unwrap_or_default();

    // Compute current progress for weekly goals from user_fitness_profile.
    let prof = sqlx::query_as::<_, (Option<i32>, Option<i32>, Option<i32>, Option<f64>, Option<i32>)>(
        "SELECT weekly_active_minutes, weekly_calories, weekly_workouts, total_distance_km, streak_days FROM user_fitness_profile WHERE user_id = $1"
    ).bind(user_id).fetch_optional(&state.db).await?;
    let (wam, wcal, wwk, tot_dist, streak) = prof.map(|r| (
        r.0.unwrap_or(0), r.1.unwrap_or(0), r.2.unwrap_or(0), r.3.unwrap_or(0.0), r.4.unwrap_or(0)
    )).unwrap_or((0, 0, 0, 0.0, 0));

    let goal_list: Vec<Value> = goals.into_iter().map(|(id, gtype, target, unit, period, active)| {
        let current = match gtype.as_str() {
            "active_minutes" => wam as f64,
            "calories"       => wcal as f64,
            "workouts"       => wwk as f64,
            "distance_km"    => tot_dist,
            "streak"         => streak as f64,
            _ => 0.0,
        };
        let pct = if target > 0.0 { (current / target * 100.0).min(100.0) } else { 0.0 };
        json!({
            "id": id, "goal_type": gtype, "target_value": target, "unit": unit,
            "period": period, "is_active": active, "current_value": current, "progress_pct": pct
        })
    }).collect();

    Ok(Json(json!({ "goals": goal_list })))
}

/// GET /fitness/leaderboard?metric=weekly_calories&limit=50
/// Respects share_fitness privacy flag on users.
pub async fn get_fitness_leaderboard(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let viewer_id = decode_access_token(&token, &state.config.secret_key)? as i64;
    let metric = params.get("metric").cloned().unwrap_or_else(|| "weekly_calories".to_string());
    let limit: i64 = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50).clamp(1, 100);

    let (col, label) = match metric.as_str() {
        "weekly_active_minutes" => ("weekly_active_minutes", "weekly_active_minutes"),
        "weekly_workouts"       => ("weekly_workouts", "weekly_workouts"),
        "streak_days"           => ("streak_days", "streak_days"),
        _                       => ("weekly_calories", "weekly_calories"),
    };

    let sql = format!(
        r#"SELECT p.user_id, u.full_name, u.profile_photo_1, p.{col}::bigint, p.streak_days
           FROM user_fitness_profile p
           JOIN users u ON u.id = p.user_id
           WHERE COALESCE(u.share_fitness, FALSE) = TRUE OR u.id = $1
           ORDER BY p.{col} DESC NULLS LAST
           LIMIT $2"#
    );

    let rows = sqlx::query_as::<_, (i64, Option<String>, Option<String>, Option<i64>, Option<i32>)>(&sql)
        .bind(viewer_id).bind(limit).fetch_all(&state.db).await.unwrap_or_default();

    let mut my_rank: Option<i64> = None;
    let entries: Vec<Value> = rows.into_iter().enumerate().map(|(i, r)| {
        let rank = (i as i64) + 1;
        if r.0 == viewer_id { my_rank = Some(rank); }
        json!({
            "rank": rank, "user_id": r.0, "full_name": r.1, "photo": r.2,
            "value": r.3.unwrap_or(0), "streak_days": r.4.unwrap_or(0)
        })
    }).collect();

    Ok(Json(json!({ "metric": label, "my_rank": my_rank, "entries": entries })))
}

/// POST /fitness/challenge — create a fitness challenge with a match
pub async fn create_fitness_challenge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let partner_id = payload["partner_id"].as_i64().ok_or_else(|| AppError::bad_request("Missing 'partner_id'"))?;
    let challenge_type = payload["type"].as_str().unwrap_or("steps").to_string();
    let target = payload["target"].as_f64().ok_or_else(|| AppError::bad_request("Missing 'target'"))?;
    let unit = payload["unit"].as_str().unwrap_or("calories").to_string();
    let days = payload["days"].as_i64().unwrap_or(7);

    let id = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO fitness_challenges (creator_id, partner_id, challenge_type, target_value, target_unit, ends_at)
           VALUES ($1, $2, $3, $4, $5, NOW() + ($6 || ' days')::interval) RETURNING id"#,
    )
    .bind(user_id).bind(partner_id).bind(&challenge_type).bind(target).bind(&unit).bind(days.to_string())
    .fetch_one(&state.db).await?;

    Ok(Json(json!({ "challenge_id": id, "type": challenge_type, "target": target, "unit": unit, "days": days })))
}

/// GET /fitness/challenges — active challenges
pub async fn get_fitness_challenges(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)? as i64;

    let challenges = sqlx::query_as::<_, (i64, i64, Option<i64>, String, f64, String, f64, f64, Option<chrono::NaiveDateTime>, String)>(
        r#"SELECT id, creator_id, partner_id, challenge_type, target_value, target_unit,
                  creator_progress, partner_progress, ends_at, status
           FROM fitness_challenges
           WHERE (creator_id = $1 OR partner_id = $1) AND status = 'active'
           ORDER BY created_at DESC"#,
    ).bind(user_id).fetch_all(&state.db).await?;

    let results: Vec<Value> = challenges.into_iter().map(|c| {
        let (my_progress, their_progress) = if c.1 == user_id { (c.6, c.7) } else { (c.7, c.6) };
        json!({
            "id": c.0, "type": c.3, "target": c.4, "unit": c.5,
            "my_progress": my_progress, "their_progress": their_progress,
            "ends_at": c.8.map(format_datetime), "status": c.9,
            "my_percent": (my_progress / c.4 * 100.0) as i32,
            "their_percent": (their_progress / c.4 * 100.0) as i32
        })
    }).collect();

    Ok(Json(json!({ "challenges": results })))
}

// ============================================================================
// Outdoor Spots + Weather + Memories
// ============================================================================

/// GET /outdoor/spots — nearby outdoor spots ranked by weather, season, time of day, user history
pub async fn get_outdoor_spots(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let lat: f64 = params.get("lat").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let lng: f64 = params.get("lng").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let category = params.get("category").cloned();
    let limit = params.get("limit").and_then(|v| v.parse::<i64>().ok()).unwrap_or(20);

    // Current month + hour for seasonal/time scoring
    let now = chrono::Utc::now();
    let current_month = now.format("%m").to_string().parse::<i32>().unwrap_or(1);
    let current_hour = now.format("%H").to_string().parse::<i32>().unwrap_or(12);

    #[derive(sqlx::FromRow)]
    struct OutdoorSpotRow {
        id: i64, name: String, description: Option<String>, category: String,
        latitude: f64, longitude: f64, city: Option<String>, elevation_m: Option<i32>,
        difficulty: Option<String>, distance_km: Option<f64>, best_months: Option<serde_json::Value>,
        best_time_of_day: Option<String>, photo_golden_hour: bool, sunset_viewpoint: bool,
        sunrise_viewpoint: bool, avg_rating: f64, visit_count: i32,
    }
    let spots = sqlx::query_as::<_, OutdoorSpotRow>(
        r#"SELECT id, name, description, category, latitude, longitude, city, elevation_m, difficulty,
                  distance_km, best_months, best_time_of_day, photo_golden_hour, sunset_viewpoint,
                  sunrise_viewpoint, avg_rating, visit_count
           FROM outdoor_spots
           WHERE ($1::text IS NULL OR category = $1)
           ORDER BY visit_count DESC LIMIT $2"#,
    ).bind(&category).bind(limit * 3).fetch_all(&state.db).await?;

    // Get user's past visits for memory matching
    let past_visits = sqlx::query_as::<_, (Option<i64>, Option<String>, Option<f64>, Option<f64>, Option<chrono::NaiveDateTime>, Option<f64>)>(
        r#"SELECT spot_id, spot_name, calories_burned, duration_min, visited_at, latitude
           FROM spot_visits WHERE user_id = $1 ORDER BY visited_at DESC"#,
    ).bind(user_id).fetch_all(&state.db).await?;

    let mut scored: Vec<(f64, Value)> = spots.into_iter().map(|s| {
        let mut score = 0.0;

        // Distance score (+25%)
        if lat != 0.0 {
            let dist = haversine_km(lat, lng, s.latitude, s.longitude);
            score += 0.25 * (1.0 / (1.0 + dist / 20.0));
        }

        // Seasonal match (+25%)
        if let Some(ref months) = s.best_months {
            if let Ok(month_list) = serde_json::from_value::<Vec<i32>>(months.clone()) {
                if month_list.contains(&current_month) { score += 0.25; }
            }
        }

        // Time of day match (+20%)
        let time_match = match s.best_time_of_day.as_deref() {
            Some("sunrise") => current_hour >= 5 && current_hour <= 8,
            Some("sunset") => current_hour >= 16 && current_hour <= 19,
            Some("morning") => current_hour >= 6 && current_hour <= 11,
            Some("evening") => current_hour >= 15 && current_hour <= 20,
            _ => true,
        };
        if time_match { score += 0.20; }

        // Golden hour photo spot bonus
        let is_golden = (current_hour >= 6 && current_hour <= 8) || (current_hour >= 17 && current_hour <= 19);
        if s.photo_golden_hour && is_golden { score += 0.10; }

        score += 0.10 * (s.avg_rating / 5.0);
        score += 0.10 * (s.visit_count as f64 / (s.visit_count as f64 + 50.0));

        // Check for memories
        let memory = past_visits.iter().find(|v| {
            v.0 == Some(s.id) || (v.5.is_some() && (v.5.unwrap() - s.latitude).abs() < 0.01)
        });
        let memory_data = memory.map(|m| json!({
            "visited_at": m.4.map(format_datetime),
            "calories_burned": m.2.map(|v| v as i32),
            "duration_min": m.3.map(|v| v as i32),
            "has_memory": true
        }));
        if memory.is_some() { score += 0.05; }

        let val = json!({
            "id": s.id, "name": s.name, "description": s.description, "category": s.category,
            "latitude": s.latitude, "longitude": s.longitude, "city": s.city,
            "elevation_m": s.elevation_m, "difficulty": s.difficulty, "distance_km": s.distance_km,
            "best_time": s.best_time_of_day, "photo_golden_hour": s.photo_golden_hour,
            "sunset_viewpoint": s.sunset_viewpoint, "sunrise_viewpoint": s.sunrise_viewpoint,
            "rating": s.avg_rating, "visits": s.visit_count,
            "relevance_score": (score * 100.0) as i32,
            "is_golden_hour_now": is_golden,
            "is_best_season": score > 0.2,
            "memory": memory_data
        });
        (score, val)
    }).collect();

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let results: Vec<Value> = scored.into_iter().take(limit as usize).map(|(_, v)| v).collect();

    Ok(Json(json!({ "spots": results, "current_month": current_month, "current_hour": current_hour })))
}

/// POST /outdoor/spots — user adds a new outdoor spot
pub async fn create_outdoor_spot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let name = payload["name"].as_str().unwrap_or("").to_string();
    if name.is_empty() { return Err(AppError::bad_request("Missing 'name'")); }
    let banner_url = extract_banner_url(&state, user_id, &payload, "spot").await?;

    let id = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO outdoor_spots (name, description, category, latitude, longitude, city, elevation_m,
                  difficulty, distance_km, best_months, best_time_of_day, photo_golden_hour,
                  sunset_viewpoint, sunrise_viewpoint, created_by, banner_url)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16) RETURNING id"#,
    )
    .bind(&name)
    .bind(payload["description"].as_str())
    .bind(payload["category"].as_str().unwrap_or("trek"))
    .bind(payload["latitude"].as_f64().unwrap_or(0.0))
    .bind(payload["longitude"].as_f64().unwrap_or(0.0))
    .bind(payload["city"].as_str())
    .bind(payload["elevation_m"].as_i64().map(|v| v as i32))
    .bind(payload["difficulty"].as_str())
    .bind(payload["distance_km"].as_f64())
    .bind(payload["best_months"].as_array().map(|a| serde_json::Value::Array(a.clone())))
    .bind(payload["best_time_of_day"].as_str())
    .bind(payload["photo_golden_hour"].as_bool().unwrap_or(false))
    .bind(payload["sunset_viewpoint"].as_bool().unwrap_or(false))
    .bind(payload["sunrise_viewpoint"].as_bool().unwrap_or(false))
    .bind(user_id)
    .bind(&banner_url)
    .fetch_one(&state.db).await?;

    // Graph: user created outdoor_spot
    {
        let db = state.db.clone();
        let uid = user_id.to_string();
        let sid = id.to_string();
        tokio::spawn(async move {
            let _ = sqlx::query("INSERT INTO graph_nodes (node_type, node_id, properties) VALUES ('outdoor_spot', $1, '{}') ON CONFLICT DO NOTHING")
                .bind(&sid).execute(&db).await;
            let _ = sqlx::query("INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id) VALUES ('user', $1, 'created_spot', 'outdoor_spot', $2) ON CONFLICT DO NOTHING")
                .bind(&uid).bind(&sid).execute(&db).await;
            let _ = sqlx::query("INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id) VALUES ('outdoor_spot', $2, 'created_spot', 'user', $1) ON CONFLICT DO NOTHING")
                .bind(&uid).bind(&sid).execute(&db).await;
        });
    }

    Ok(Json(json!({ "spot_id": id })))
}

/// POST /outdoor/visit — log a visit with weather + fitness data + memories
pub async fn log_spot_visit(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let spot_id = payload["spot_id"].as_i64();
    let spot_name = payload["spot_name"].as_str().map(|s| s.to_string());

    let id = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO spot_visits (user_id, spot_id, spot_name, latitude, longitude,
                  weather_temp_c, weather_condition, weather_humidity, weather_wind_kmh,
                  uv_index, visibility_km, sunrise_time, sunset_time,
                  calories_burned, duration_min, rating, notes, photo_url)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, $18)
           RETURNING id"#,
    )
    .bind(user_id).bind(spot_id).bind(&spot_name)
    .bind(payload["latitude"].as_f64()).bind(payload["longitude"].as_f64())
    .bind(payload["weather_temp_c"].as_f64())
    .bind(payload["weather_condition"].as_str())
    .bind(payload["weather_humidity"].as_i64().map(|v| v as i32))
    .bind(payload["weather_wind_kmh"].as_f64())
    .bind(payload["uv_index"].as_i64().map(|v| v as i32))
    .bind(payload["visibility_km"].as_f64())
    .bind(payload["sunrise_time"].as_str())
    .bind(payload["sunset_time"].as_str())
    .bind(payload["calories_burned"].as_f64())
    .bind(payload["duration_min"].as_f64())
    .bind(payload["rating"].as_i64().map(|v| v as i32))
    .bind(payload["notes"].as_str())
    .bind(payload["photo_url"].as_str())
    .fetch_one(&state.db).await?;

    // Update spot stats
    if let Some(sid) = spot_id {
        sqlx::query("UPDATE outdoor_spots SET visit_count = visit_count + 1 WHERE id = $1")
            .bind(sid).execute(&state.db).await?;
        if let Some(rating) = payload["rating"].as_i64() {
            sqlx::query(
                "UPDATE outdoor_spots SET avg_rating = (avg_rating * visit_count + $1) / (visit_count + 1) WHERE id = $2"
            ).bind(rating as f64).bind(sid).execute(&state.db).await?;
        }
    }

    // Graph: user visited outdoor_spot
    if let Some(sid) = spot_id {
        let db = state.db.clone();
        let uid = user_id.to_string();
        let sid_str = sid.to_string();
        tokio::spawn(async move {
            let _ = sqlx::query("INSERT INTO graph_nodes (node_type, node_id, properties) VALUES ('user', $1, '{}') ON CONFLICT DO NOTHING")
                .bind(&uid).execute(&db).await;
            let _ = sqlx::query("INSERT INTO graph_nodes (node_type, node_id, properties) VALUES ('outdoor_spot', $1, '{}') ON CONFLICT DO NOTHING")
                .bind(&sid_str).execute(&db).await;
            let _ = sqlx::query("INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id) VALUES ('user', $1, 'visited_spot', 'outdoor_spot', $2) ON CONFLICT DO NOTHING")
                .bind(&uid).bind(&sid_str).execute(&db).await;
            let _ = sqlx::query("INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id) VALUES ('outdoor_spot', $2, 'visited_spot', 'user', $1) ON CONFLICT DO NOTHING")
                .bind(&uid).bind(&sid_str).execute(&db).await;
        });
    }

    Ok(Json(json!({ "visit_id": id })))
}

/// GET /outdoor/memories — user's past visits, grouped by location (for revisit memories)
pub async fn get_spot_memories(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let lat = params.get("lat").and_then(|v| v.parse::<f64>().ok());
    let lng = params.get("lng").and_then(|v| v.parse::<f64>().ok());

    // If lat/lng provided, find memories near that location
    let visits = if let (Some(lat), Some(lng)) = (lat, lng) {
        sqlx::query_as::<_, (i64, Option<i64>, Option<String>, Option<f64>, Option<f64>, Option<f64>, Option<String>, Option<f64>, Option<f64>, chrono::NaiveDateTime, Option<String>, Option<i32>, Option<String>)>(
            r#"SELECT id, spot_id, spot_name, latitude, longitude, weather_temp_c, weather_condition,
                      calories_burned, duration_min, visited_at, photo_url, rating, notes
               FROM spot_visits WHERE user_id = $1
               AND latitude IS NOT NULL AND ABS(latitude - $2) < 0.05 AND ABS(longitude - $3) < 0.05
               ORDER BY visited_at DESC"#,
        ).bind(user_id).bind(lat).bind(lng).fetch_all(&state.db).await?
    } else {
        sqlx::query_as::<_, (i64, Option<i64>, Option<String>, Option<f64>, Option<f64>, Option<f64>, Option<String>, Option<f64>, Option<f64>, chrono::NaiveDateTime, Option<String>, Option<i32>, Option<String>)>(
            r#"SELECT id, spot_id, spot_name, latitude, longitude, weather_temp_c, weather_condition,
                      calories_burned, duration_min, visited_at, photo_url, rating, notes
               FROM spot_visits WHERE user_id = $1
               ORDER BY visited_at DESC LIMIT 50"#,
        ).bind(user_id).fetch_all(&state.db).await?
    };

    let results: Vec<Value> = visits.into_iter().map(|v| {
        let days_ago = (chrono::Utc::now().naive_utc() - v.9).num_days();
        json!({
            "id": v.0, "spot_id": v.1, "spot_name": v.2,
            "latitude": v.3, "longitude": v.4,
            "weather": { "temp_c": v.5, "condition": v.6 },
            "calories_burned": v.7.map(|c| c as i32),
            "duration_min": v.8.map(|d| d as i32),
            "visited_at": format_datetime(v.9),
            "days_ago": days_ago,
            "photo_url": v.10, "rating": v.11, "notes": v.12,
            "memory_label": if days_ago > 365 { format!("{}y ago", days_ago / 365) }
                           else if days_ago > 30 { format!("{}mo ago", days_ago / 30) }
                           else { format!("{}d ago", days_ago) }
        })
    }).collect();

    Ok(Json(json!({ "memories": results, "total": results.len() })))
}

/// GET /outdoor/seasonal-guide — best activities for current location + season
pub async fn get_seasonal_guide(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _user_id = decode_access_token(&token, &state.config.secret_key)?;

    let city = params.get("city").cloned().unwrap_or_default();
    let month = chrono::Utc::now().format("%m").to_string().parse::<i32>().unwrap_or(1);

    // Get weather stats for this city/month
    let stats = sqlx::query_as::<_, (Option<f64>, Option<i32>, Option<f64>, Option<String>, Option<String>)>(
        "SELECT avg_temp_c, avg_humidity, avg_rainfall_mm, best_activity, weather_rating FROM location_weather_stats WHERE city = $1 AND month = $2"
    ).bind(&city).bind(month).fetch_optional(&state.db).await?;

    // Get top spots for this season
    let seasonal_spots = sqlx::query_as::<_, (i64, String, String, Option<String>, f64, bool, bool)>(
        r#"SELECT id, name, category, difficulty, avg_rating, sunset_viewpoint, sunrise_viewpoint
           FROM outdoor_spots WHERE city = $1 AND best_months @> $2::jsonb
           ORDER BY avg_rating DESC LIMIT 10"#,
    ).bind(&city).bind(serde_json::json!([month])).fetch_all(&state.db).await?;

    let spots: Vec<Value> = seasonal_spots.into_iter().map(|s| json!({
        "id": s.0, "name": s.1, "category": s.2, "difficulty": s.3,
        "rating": s.4, "sunset": s.5, "sunrise": s.6
    })).collect();

    let season = match month {
        1..=2 | 12 => "winter",
        3..=5 => "spring",
        6..=9 => "monsoon",
        10..=11 => "autumn",
        _ => "unknown",
    };

    Ok(Json(json!({
        "city": city, "month": month, "season": season,
        "weather": stats.as_ref().map(|s| json!({
            "avg_temp_c": s.0, "humidity": s.1, "rainfall_mm": s.2,
            "best_activity": s.3, "rating": s.4
        })),
        "recommended_spots": spots,
        "tips": match season {
            "winter" => "Perfect for sunrise treks and long hikes. Cool temperatures, clear skies.",
            "spring" => "Best season for outdoor photos. Wildflowers blooming, pleasant weather.",
            "monsoon" => "Waterfalls are spectacular but trails may be slippery. Carry rain gear.",
            "autumn" => "Golden hour photography is stunning. Cool evenings perfect for sunset spots.",
            _ => "Check local conditions before heading out."
        }
    })))
}

/// POST /map/search — track what user searches on map
pub async fn track_map_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let query = payload["query"].as_str().map(|s| s.to_string());
    let category = payload["category"].as_str().map(|s| s.to_string());
    let result_name = payload["result_name"].as_str().map(|s| s.to_string());
    let result_lat = payload["result_latitude"].as_f64();
    let result_lng = payload["result_longitude"].as_f64();
    let user_lat = payload["user_latitude"].as_f64();
    let user_lng = payload["user_longitude"].as_f64();
    let selected = payload["selected"].as_bool().unwrap_or(false);
    let navigated = payload["navigated"].as_bool().unwrap_or(false);
    let source = payload["source"].as_str().unwrap_or("map");

    // Calculate distance if both positions available
    let distance = match (user_lat, user_lng, result_lat, result_lng) {
        (Some(ul), Some(uln), Some(rl), Some(rln)) => Some(haversine_km(ul, uln, rl, rln)),
        _ => None,
    };

    let id = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO user_map_searches (user_id, search_query, search_category, result_name,
                  result_latitude, result_longitude, user_latitude, user_longitude,
                  distance_from_user_km, selected, navigated, source)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) RETURNING id"#,
    )
    .bind(user_id).bind(&query).bind(&category).bind(&result_name)
    .bind(result_lat).bind(result_lng).bind(user_lat).bind(user_lng)
    .bind(distance).bind(selected).bind(navigated).bind(source)
    .fetch_one(&state.db).await?;

    Ok(Json(json!({ "tracked": true, "id": id })))
}

/// GET /map/trending — what people are searching for near a location
pub async fn get_map_trending(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _user_id = decode_access_token(&token, &state.config.secret_key)?;

    let lat: f64 = params.get("lat").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let lng: f64 = params.get("lng").and_then(|v| v.parse().ok()).unwrap_or(0.0);

    // Top searched places near this location
    let trending_places = sqlx::query_as::<_, (Option<String>, f64, f64, i64, i64)>(
        r#"SELECT result_name, AVG(result_latitude) as lat, AVG(result_longitude) as lng,
                  COUNT(*) as search_count, COUNT(DISTINCT user_id) as unique_users
           FROM user_map_searches
           WHERE result_latitude IS NOT NULL
             AND ABS(result_latitude - $1) < 0.2 AND ABS(result_longitude - $2) < 0.2
             AND searched_at > NOW() - INTERVAL '30 days'
             AND result_name IS NOT NULL
           GROUP BY result_name
           ORDER BY search_count DESC LIMIT 15"#,
    ).bind(lat).bind(lng).fetch_all(&state.db).await?;

    let places: Vec<Value> = trending_places.into_iter().map(|p| json!({
        "name": p.0, "latitude": p.1, "longitude": p.2,
        "searches": p.3, "unique_users": p.4
    })).collect();

    // Top search categories
    let categories = sqlx::query_as::<_, (Option<String>, i64)>(
        r#"SELECT search_category, COUNT(*) as cnt
           FROM user_map_searches
           WHERE user_latitude IS NOT NULL
             AND ABS(user_latitude - $1) < 0.2 AND ABS(user_longitude - $2) < 0.2
             AND searched_at > NOW() - INTERVAL '30 days'
             AND search_category IS NOT NULL
           GROUP BY search_category ORDER BY cnt DESC LIMIT 10"#,
    ).bind(lat).bind(lng).fetch_all(&state.db).await?;

    let cat_list: Vec<Value> = categories.into_iter().map(|c| json!({
        "category": c.0, "searches": c.1
    })).collect();

    // Top search queries
    let queries = sqlx::query_as::<_, (Option<String>, i64)>(
        r#"SELECT search_query, COUNT(*) as cnt
           FROM user_map_searches
           WHERE user_latitude IS NOT NULL
             AND ABS(user_latitude - $1) < 0.2 AND ABS(user_longitude - $2) < 0.2
             AND searched_at > NOW() - INTERVAL '7 days'
             AND search_query IS NOT NULL
           GROUP BY search_query ORDER BY cnt DESC LIMIT 10"#,
    ).bind(lat).bind(lng).fetch_all(&state.db).await?;

    let query_list: Vec<Value> = queries.into_iter().map(|q| json!({
        "query": q.0, "searches": q.1
    })).collect();

    Ok(Json(json!({
        "trending_places": places,
        "popular_categories": cat_list,
        "recent_searches_nearby": query_list
    })))
}

/// GET /map/user-interests — what THIS user typically searches for (ML personalization)
pub async fn get_map_user_interests(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // User's top search categories
    let categories = sqlx::query_as::<_, (Option<String>, i64)>(
        r#"SELECT search_category, COUNT(*) as cnt
           FROM user_map_searches WHERE user_id = $1 AND search_category IS NOT NULL
           GROUP BY search_category ORDER BY cnt DESC LIMIT 10"#,
    ).bind(user_id).fetch_all(&state.db).await?;

    // User's frequently searched places
    let fav_places = sqlx::query_as::<_, (Option<String>, f64, f64, i64)>(
        r#"SELECT result_name, AVG(result_latitude), AVG(result_longitude), COUNT(*) as visits
           FROM user_map_searches WHERE user_id = $1 AND selected = true AND result_name IS NOT NULL
           GROUP BY result_name ORDER BY visits DESC LIMIT 10"#,
    ).bind(user_id).fetch_all(&state.db).await?;

    // Average search distance (how far does user explore?)
    let avg_dist = sqlx::query_scalar::<_, Option<f64>>(
        "SELECT AVG(distance_from_user_km) FROM user_map_searches WHERE user_id = $1 AND distance_from_user_km IS NOT NULL"
    ).bind(user_id).fetch_one(&state.db).await?;

    // Search time patterns
    let time_patterns = sqlx::query_as::<_, (i32, i64)>(
        r#"SELECT EXTRACT(HOUR FROM searched_at)::int as hour, COUNT(*) as cnt
           FROM user_map_searches WHERE user_id = $1
           GROUP BY hour ORDER BY cnt DESC LIMIT 5"#,
    ).bind(user_id).fetch_all(&state.db).await?;

    let cat_list: Vec<Value> = categories.into_iter().map(|c| json!({ "category": c.0, "count": c.1 })).collect();
    let place_list: Vec<Value> = fav_places.into_iter().map(|p| json!({ "name": p.0, "lat": p.1, "lng": p.2, "visits": p.3 })).collect();
    let time_list: Vec<Value> = time_patterns.into_iter().map(|t| json!({ "hour": t.0, "searches": t.1 })).collect();

    let explorer_type = match avg_dist.unwrap_or(5.0) {
        d if d < 3.0 => "local",
        d if d < 15.0 => "explorer",
        d if d < 50.0 => "adventurer",
        _ => "nomad",
    };

    Ok(Json(json!({
        "top_categories": cat_list,
        "favorite_places": place_list,
        "avg_search_distance_km": avg_dist.map(|d| format!("{:.1}", d)),
        "explorer_type": explorer_type,
        "search_time_patterns": time_list
    })))
}

// ============================================================================
// Journey Tracking + Next-Place Recommendation
// ============================================================================

/// POST /journey/start — begin tracking a journey session
pub async fn start_journey(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let session_id = payload["session_id"].as_str()
        .unwrap_or(&Uuid::new_v4().to_string()).to_string();
    let city = payload["city"].as_str().map(|s| s.to_string());
    let weather = payload["weather_condition"].as_str().map(|s| s.to_string());

    let id = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO user_journeys (user_id, session_id, city, weather_condition)
           VALUES ($1, $2, $3, $4)
           ON CONFLICT (user_id, session_id) DO UPDATE SET started_at = NOW()
           RETURNING id"#,
    ).bind(user_id).bind(&session_id).bind(&city).bind(&weather)
    .fetch_one(&state.db).await?;

    Ok(Json(json!({ "journey_id": id, "session_id": session_id })))
}

/// POST /journey/stop — log arriving at a new place in the journey
pub async fn log_journey_stop(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let journey_id = payload["journey_id"].as_i64()
        .ok_or_else(|| AppError::bad_request("Missing 'journey_id'"))?;
    let place_name = payload["place_name"].as_str().map(|s| s.to_string());
    let category = payload["category"].as_str().map(|s| s.to_string());
    let lat = payload["latitude"].as_f64().unwrap_or(0.0);
    let lng = payload["longitude"].as_f64().unwrap_or(0.0);
    let weather_temp = payload["weather_temp_c"].as_f64();
    let weather_cond = payload["weather_condition"].as_str().map(|s| s.to_string());
    let calories = payload["calories"].as_f64();
    let label = payload["activity_label"].as_str().map(|s| s.to_string());

    // Get previous stop to calculate distance + transition time
    let prev = sqlx::query_as::<_, (i32, f64, f64, Option<chrono::NaiveDateTime>, Option<String>)>(
        r#"SELECT stop_order, latitude, longitude, arrived_at, place_category
           FROM journey_stops WHERE journey_id = $1 ORDER BY stop_order DESC LIMIT 1"#,
    ).bind(journey_id).fetch_optional(&state.db).await?;

    let (stop_order, dist_from_prev, prev_category) = match &prev {
        Some(p) => {
            let dist = haversine_km(p.1, p.2, lat, lng);
            (p.0 + 1, Some(dist), p.4.clone())
        }
        None => (1, None, None),
    };

    // Update previous stop's departure time
    if prev.is_some() {
        sqlx::query("UPDATE journey_stops SET departed_at = NOW(), dwell_time_min = EXTRACT(EPOCH FROM (NOW() - arrived_at)) / 60.0 WHERE journey_id = $1 AND stop_order = $2")
            .bind(journey_id).bind(stop_order - 1).execute(&state.db).await?;
    }

    let stop_id = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO journey_stops (journey_id, user_id, stop_order, place_name, place_category,
                  latitude, longitude, distance_from_prev_km, calories_at_stop,
                  weather_temp_c, weather_condition, labeled_activity)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12) RETURNING id"#,
    )
    .bind(journey_id).bind(user_id).bind(stop_order).bind(&place_name).bind(&category)
    .bind(lat).bind(lng).bind(dist_from_prev).bind(calories)
    .bind(weather_temp).bind(&weather_cond).bind(&label)
    .fetch_one(&state.db).await?;

    // Update journey totals
    sqlx::query(
        r#"UPDATE user_journeys SET total_stops = $1,
           total_distance_km = total_distance_km + COALESCE($2, 0),
           ended_at = NOW() WHERE id = $3"#,
    ).bind(stop_order).bind(dist_from_prev).bind(journey_id).execute(&state.db).await?;

    // Update transition patterns (from_category → to_category)
    if let (Some(from_cat), Some(ref to_cat)) = (prev_category, &category) {
        let now = chrono::Utc::now();
        let time_of_day = match now.format("%H").to_string().parse::<i32>().unwrap_or(12) {
            5..=8 => "morning", 9..=12 => "midday", 13..=17 => "afternoon", 18..=21 => "evening", _ => "night"
        };
        let season = match now.format("%m").to_string().parse::<i32>().unwrap_or(1) {
            1..=2 | 12 => "winter", 3..=5 => "spring", 6..=9 => "monsoon", _ => "autumn"
        };

        let db = state.db.clone();
        let fc = from_cat.clone();
        let tc = to_cat.clone();
        let dist = dist_from_prev;
        let tod = time_of_day.to_string();
        let ssn = season.to_string();
        tokio::spawn(async move {
            let _ = sqlx::query(
                r#"INSERT INTO journey_patterns (from_category, to_category, transition_count, avg_distance_km, city, time_of_day, season)
                   VALUES ($1, $2, 1, $3, $4, $5, $6)
                   ON CONFLICT DO NOTHING"#,
            ).bind(&fc).bind(&tc).bind(dist).bind::<Option<String>>(None).bind(&tod).bind(&ssn)
            .execute(&db).await;
        });
    }

    Ok(Json(json!({ "stop_id": stop_id, "stop_order": stop_order, "distance_from_prev_km": dist_from_prev })))
}

/// GET /journey/recommend-next — "where should I go next?" based on patterns
pub async fn recommend_next_place(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let current_category = params.get("category").cloned().unwrap_or_default();
    let lat: f64 = params.get("lat").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let lng: f64 = params.get("lng").and_then(|v| v.parse().ok()).unwrap_or(0.0);

    // What do people usually do AFTER this category?
    let patterns = sqlx::query_as::<_, (String, i32, Option<f64>)>(
        r#"SELECT to_category, SUM(transition_count) as total, AVG(avg_distance_km) as avg_dist
           FROM journey_patterns WHERE from_category = $1
           GROUP BY to_category ORDER BY total DESC LIMIT 5"#,
    ).bind(&current_category).fetch_all(&state.db).await?;

    // What does THIS USER usually do after this category?
    let personal = sqlx::query_as::<_, (Option<String>, i64)>(
        r#"SELECT js2.place_category, COUNT(*) as cnt
           FROM journey_stops js1
           JOIN journey_stops js2 ON js1.journey_id = js2.journey_id AND js2.stop_order = js1.stop_order + 1
           WHERE js1.user_id = $1 AND js1.place_category = $2 AND js2.place_category IS NOT NULL
           GROUP BY js2.place_category ORDER BY cnt DESC LIMIT 3"#,
    ).bind(user_id).bind(&current_category).fetch_all(&state.db).await?;

    // Find actual places nearby matching the top next categories
    let top_next = patterns.first().map(|p| p.0.clone()).unwrap_or("cafe".to_string());

    let nearby_places = sqlx::query_as::<_, (i64, String, Option<String>, f64, f64, f64)>(
        r#"SELECT id, name, category, latitude, longitude, avg_rating
           FROM outdoor_spots
           WHERE category = $1 AND ABS(latitude - $2) < 0.1 AND ABS(longitude - $3) < 0.1
           ORDER BY avg_rating DESC LIMIT 5"#,
    ).bind(&top_next).bind(lat).bind(lng).fetch_all(&state.db).await?;

    let pattern_list: Vec<Value> = patterns.into_iter().map(|p| {
        let total: i32 = p.1;
        json!({
            "next_category": p.0, "times_observed": total,
            "avg_distance_km": p.2.map(|d| format!("{:.1}", d)),
            "label": format!("{}% of people go to {} after {}",
                (total as f64 / total.max(1) as f64 * 100.0) as i32, p.0, current_category)
        })
    }).collect();

    let personal_list: Vec<Value> = personal.into_iter().map(|p| json!({
        "category": p.0, "your_count": p.1
    })).collect();

    let place_list: Vec<Value> = nearby_places.into_iter().map(|p| json!({
        "id": p.0, "name": p.1, "category": p.2, "latitude": p.3, "longitude": p.4, "rating": p.5
    })).collect();

    Ok(Json(json!({
        "current": current_category,
        "global_patterns": pattern_list,
        "your_patterns": personal_list,
        "recommended_places": place_list,
        "suggestion": format!("After {}, people usually visit a {}. Here are some nearby:", current_category, top_next)
    })))
}

/// GET /journey/history — user's past journeys
pub async fn get_journey_history(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let journeys = sqlx::query_as::<_, (i64, String, Option<String>, i32, Option<f64>, chrono::NaiveDateTime)>(
        r#"SELECT id, session_id, city, total_stops, total_distance_km, started_at
           FROM user_journeys WHERE user_id = $1 ORDER BY started_at DESC LIMIT 20"#,
    ).bind(user_id).fetch_all(&state.db).await?;

    let mut results = Vec::new();
    for j in journeys {
        let stops = sqlx::query_as::<_, (i32, Option<String>, Option<String>, f64, f64, Option<f64>)>(
            r#"SELECT stop_order, place_name, place_category, latitude, longitude, dwell_time_min
               FROM journey_stops WHERE journey_id = $1 ORDER BY stop_order"#,
        ).bind(j.0).fetch_all(&state.db).await?;

        let stop_list: Vec<Value> = stops.into_iter().map(|s| json!({
            "order": s.0, "name": s.1, "category": s.2,
            "lat": s.3, "lng": s.4, "dwell_min": s.5.map(|d| d as i32)
        })).collect();

        results.push(json!({
            "id": j.0, "city": j.2, "stops": j.3, "distance_km": j.4.map(|d| format!("{:.1}", d)),
            "date": format_datetime(j.5), "route": stop_list
        }));
    }

    Ok(Json(json!({ "journeys": results })))
}

/// GET /outdoor/location-activity — who posted content at a location + weather/time patterns
pub async fn get_location_activity(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let lat: f64 = params.get("lat").and_then(|v| v.parse().ok()).unwrap_or(0.0);
    let lng: f64 = params.get("lng").and_then(|v| v.parse().ok()).unwrap_or(0.0);

    // Who posted content here (reels, spots) with time/weather context
    let creators = sqlx::query_as::<_, (i64, String, Option<String>, i32, i32, Option<String>, chrono::NaiveDateTime)>(
        r#"SELECT cl.user_id, cl.content_type, cl.location_name, cl.hour_of_day, cl.month,
                  cl.season, cl.posted_at
           FROM location_content_log cl
           WHERE cl.user_id != $1
             AND ABS(cl.latitude - $2) < 0.05 AND ABS(cl.longitude - $3) < 0.05
           ORDER BY cl.posted_at DESC LIMIT 20"#,
    ).bind(user_id).bind(lat).bind(lng).fetch_all(&state.db).await?;

    let creator_list: Vec<Value> = creators.into_iter().map(|c| json!({
        "user_id": c.0, "content_type": c.1, "location_name": c.2,
        "hour": c.3, "month": c.4, "season": c.5, "posted_at": format_datetime(c.6)
    })).collect();

    // Best time patterns for this location (when do people post most)
    let time_patterns = sqlx::query_as::<_, (i32, i64)>(
        r#"SELECT hour_of_day, COUNT(*) as post_count
           FROM location_content_log
           WHERE ABS(latitude - $1) < 0.05 AND ABS(longitude - $2) < 0.05
           GROUP BY hour_of_day ORDER BY post_count DESC LIMIT 5"#,
    ).bind(lat).bind(lng).fetch_all(&state.db).await?;

    let peak_hours: Vec<Value> = time_patterns.into_iter().map(|t| json!({
        "hour": t.0, "posts": t.1,
        "label": match t.0 { 5..=7 => "sunrise", 8..=11 => "morning", 12..=15 => "afternoon", 16..=19 => "golden hour", _ => "night" }
    })).collect();

    // Best season patterns
    let season_patterns = sqlx::query_as::<_, (Option<String>, i64)>(
        r#"SELECT season, COUNT(*) as post_count
           FROM location_content_log
           WHERE ABS(latitude - $1) < 0.05 AND ABS(longitude - $2) < 0.05
           GROUP BY season ORDER BY post_count DESC"#,
    ).bind(lat).bind(lng).fetch_all(&state.db).await?;

    let seasons: Vec<Value> = season_patterns.into_iter().map(|s| json!({
        "season": s.0, "posts": s.1
    })).collect();

    // Who interacted with content at this location
    let interactions = sqlx::query_as::<_, (i64, i64, String, Option<String>, chrono::NaiveDateTime)>(
        r#"SELECT user_id, target_user_id, interaction_type, content_type, created_at
           FROM location_interactions
           WHERE (user_id = $1 OR target_user_id = $1)
             AND ABS(latitude - $2) < 0.05 AND ABS(longitude - $3) < 0.05
           ORDER BY created_at DESC LIMIT 20"#,
    ).bind(user_id).bind(lat).bind(lng).fetch_all(&state.db).await?;

    let interaction_list: Vec<Value> = interactions.into_iter().map(|i| json!({
        "user_id": i.0, "target_user_id": i.1, "type": i.2,
        "content_type": i.3, "at": format_datetime(i.4)
    })).collect();

    Ok(Json(json!({
        "creators": creator_list,
        "peak_hours": peak_hours,
        "best_seasons": seasons,
        "your_interactions": interaction_list,
        "total_posts_here": creator_list.len()
    })))
}

// ============================================================================
// Contact Matching
// ============================================================================

/// POST /contacts/sync — sync hashed phone numbers to find friends on app
pub async fn sync_contacts(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let hashes = payload["hashes"].as_array()
        .ok_or_else(|| AppError::bad_request("Missing 'hashes' array"))?;

    // Store contact hashes
    for h in hashes {
        let phone_hash = h["hash"].as_str().unwrap_or("").to_string();
        let name = h["name"].as_str().map(|s| s.to_string());
        if phone_hash.is_empty() { continue; }

        let _ = sqlx::query(
            "INSERT INTO user_contact_hashes (user_id, phone_hash, contact_name) VALUES ($1, $2, $3) ON CONFLICT (user_id, phone_hash) DO NOTHING"
        ).bind(user_id).bind(&phone_hash).bind(&name).execute(&state.db).await;
    }

    // Find matches: contacts whose phone_hash matches a user's phone number hash
    // Only return users who have discoverable_by_contacts = true
    let hash_list: Vec<String> = hashes.iter()
        .filter_map(|h| h["hash"].as_str().map(|s| s.to_string()))
        .collect();

    let matches = sqlx::query_as::<_, (i64, Option<String>, Option<String>)>(
        r#"SELECT u.id, u.name, u.profile_photo_1
           FROM users u
           WHERE u.id != $1
             AND u.is_active = true
             AND u.discoverable_by_contacts = true
             AND encode(sha256(u.phone_number::bytea), 'hex') = ANY($2)"#,
    )
    .bind(user_id)
    .bind(&hash_list)
    .fetch_all(&state.db)
    .await?;

    let results: Vec<Value> = matches.into_iter().map(|m| {
        json!({ "user_id": m.0, "name": m.1, "photo": m.2 })
    }).collect();

    Ok(Json(json!({ "contacts_on_app": results, "count": results.len() })))
}

// ============================================================================
// Privacy Controls
// ============================================================================

/// POST /privacy/settings — update privacy preferences
pub async fn update_privacy_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    if let Some(discoverable) = payload["discoverable_by_contacts"].as_bool() {
        sqlx::query("UPDATE users SET discoverable_by_contacts = $1 WHERE id = $2")
            .bind(discoverable).bind(user_id).execute(&state.db).await?;
    }

    if let Some(share_music) = payload["share_music_taste"].as_bool() {
        sqlx::query("UPDATE users SET share_music_taste = $1 WHERE id = $2")
            .bind(share_music).bind(user_id).execute(&state.db).await?;
    }

    Ok(Json(json!({ "updated": true })))
}

/// GET /privacy/settings
pub async fn get_privacy_settings(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let row = sqlx::query_as::<_, (Option<bool>, Option<bool>)>(
        "SELECT discoverable_by_contacts, share_music_taste FROM users WHERE id = $1"
    ).bind(user_id).fetch_one(&state.db).await?;

    Ok(Json(json!({
        "discoverable_by_contacts": row.0.unwrap_or(true),
        "share_music_taste": row.1.unwrap_or(true)
    })))
}

// ============================================================================
// Vision Analysis
// ============================================================================

#[derive(Deserialize)]
pub struct VisionAnalyzeRequest {
    image_base64: String,
}

pub async fn vision_analyze(
    State(state): State<AppState>,
    Json(payload): Json<VisionAnalyzeRequest>,
) -> Result<Json<Value>, AppError> {
    let vision = state
        .vision
        .as_ref()
        .ok_or_else(|| {
            state.metrics.inc_vision_unavailable();
            AppError::service_unavailable("Vision service is not available. Try again later.")
        })?
        .clone();
    let bytes = STANDARD
        .decode(payload.image_base64.as_bytes())
        .map_err(|_| AppError::bad_request("Invalid base64 image payload"))?;
    let result = analyze_photo_bytes(vision, bytes).await?;
    let analysis = result.analysis.ok_or_else(|| AppError::internal("Vision analysis failed"))?;
    Ok(Json(serde_json::to_value(analysis).unwrap_or(json!({}))))
}

// ============================================================================
// Selfie Verification
// ============================================================================

pub async fn verify_selfie(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Read the upload FIRST, always draining the request body. Rejecting before
    // consuming a multi-MB upload makes the client see a dropped connection
    // (URLSession -1005) instead of a clean HTTP status.
    let mut selfie_bytes: Option<Vec<u8>> = None;
    let mut wrong_content_type = false;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::bad_request("Invalid multipart data"))?
    {
        if field.name().unwrap_or("") != "selfie" {
            continue;
        }
        let content_type = field
            .content_type()
            .map(|value| value.to_string())
            .unwrap_or_default();
        if !content_type.starts_with("image/") {
            // Drain this field's bytes before we bail, so the body is consumed.
            wrong_content_type = true;
            let _ = read_binary_field(&mut field, state.config.max_photo_bytes).await;
            continue;
        }
        selfie_bytes = Some(read_binary_field(&mut field, state.config.max_photo_bytes).await?);
    }

    if wrong_content_type && selfie_bytes.is_none() {
        return Err(AppError::bad_request("Selfie must be an image"));
    }
    let selfie_bytes =
        selfie_bytes.ok_or_else(|| AppError::bad_request("selfie is required"))?;

    // Now that the body is fully read, check that vision is available. If the
    // ONNX models aren't deployed, this returns a clean 503 (not a dropped conn).
    let vision = state
        .vision
        .as_ref()
        .ok_or_else(|| {
            state.metrics.inc_vision_unavailable();
            AppError::service_unavailable("Photo verification is temporarily unavailable.")
        })?
        .clone();

    let selfie_result = analyze_photo_bytes(vision.clone(), selfie_bytes).await?;
    let selfie_analysis = selfie_result.analysis.ok_or_else(|| AppError::internal("Vision analysis failed"))?;

    let user = fetch_user_by_id(&state.db, user_id)
        .await?
        .ok_or_else(|| AppError::not_found("User not found"))?;

    let photo_paths = extract_photo_paths(&user);
    if photo_paths.is_empty() {
        return Err(AppError::bad_request("Complete your profile first"));
    }

    let mut best_similarity: Option<f32> = None;
    for path in photo_paths {
        // photo_paths are URL paths like "/uploads/photos/123.jpg";
        // strip the leading "/uploads" prefix and resolve against upload_dir
        let relative = path.trim_start_matches("/uploads/");
        let disk_path = format!("{}/{}", state.config.upload_dir, relative);
        let bytes = match fs::read(&disk_path).await {
            Ok(bytes) => bytes,
            Err(_) => continue,
        };
        if bytes.len() > state.config.max_photo_bytes {
            continue;
        }
        let result = match analyze_photo_bytes(vision.clone(), bytes).await {
            Ok(photo) => photo,
            Err(_) => continue,
        };
        if let Some(analysis) = result.analysis {
            if let Some(similarity) =
                cosine_similarity(&selfie_analysis.style_embedding, &analysis.style_embedding)
            {
                if best_similarity.map(|best| similarity > best).unwrap_or(true) {
                    best_similarity = Some(similarity);
                }
            }
        }
    }

    let face_match_score = match best_similarity {
        Some(score) => score,
        None => return Err(AppError::bad_request("No valid photos found")),
    };
    let liveness_score = selfie_analysis.authenticity_score;

    let mut failure_reasons = Vec::new();
    if selfie_analysis.inappropriate_content {
        failure_reasons.push("inappropriate_content");
    }
    if !selfie_analysis.face_detected {
        failure_reasons.push("no_face_detected");
    }
    if face_match_score < state.config.selfie_match_threshold {
        failure_reasons.push("face_mismatch");
    }
    if liveness_score < state.config.selfie_liveness_threshold {
        failure_reasons.push("liveness_low");
    }

    let verified = failure_reasons.is_empty();
    if verified {
        let updated = sqlx::query(
            "UPDATE users SET is_verified = TRUE, verified_at = NOW() WHERE id = $1",
        )
        .bind(user_id)
        .execute(&state.db)
        .await?;
        if updated.rows_affected() == 0 {
            return Err(AppError::not_found("User not found"));
        }
    }

    Ok(Json(json!({
        "verified": verified,
        "confidence": face_match_score,
        "liveness_score": liveness_score,
        "face_match_score": face_match_score,
        "failure_reasons": if verified { Value::Null } else { json!(failure_reasons) },
    })))
}

// ============================================================================
// Admin Stats
// ============================================================================

/// POST /admin/user/{user_id}/override — Admin override for locked identity fields (gender, dob)
/// Only accessible with admin JWT. Logs the change for audit trail.
pub async fn admin_override_identity(
    State(state): State<AppState>,
    _admin: AdminClaims,
    AxumPath(target_user_id): AxumPath<i32>,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let mut changes = Vec::new();

    if let Some(gender) = payload.get("gender").and_then(|v| v.as_str()) {
        sqlx::query("UPDATE users SET gender = $1, updated_at = NOW() WHERE id = $2")
            .bind(gender).bind(target_user_id).execute(&state.db).await?;
        changes.push(format!("gender → {}", gender));
    }

    if let Some(dob) = payload.get("dob").and_then(|v| v.as_str()) {
        let date = chrono::NaiveDate::parse_from_str(dob, "%Y-%m-%d")
            .map_err(|_| AppError::bad_request("dob must be YYYY-MM-DD format"))?;
        sqlx::query("UPDATE users SET dob = $1, updated_at = NOW() WHERE id = $2")
            .bind(date).bind(target_user_id).execute(&state.db).await?;
        changes.push(format!("dob → {}", dob));
    }

    if let Some(name) = payload.get("name").and_then(|v| v.as_str()) {
        sqlx::query("UPDATE users SET name = $1, updated_at = NOW() WHERE id = $2")
            .bind(name).bind(target_user_id).execute(&state.db).await?;
        changes.push(format!("name → {}", name));
    }

    if changes.is_empty() {
        return Err(AppError::bad_request("No fields to update. Send: gender, dob, name"));
    }

    // Audit log
    let reason = payload.get("reason").and_then(|v| v.as_str()).unwrap_or("admin override");
    tracing::info!(target_user_id, reason, changes = ?changes, "Admin identity override");

    let _ = sqlx::query(
        "INSERT INTO interaction_events (user_id, target_user_id, event_type, metadata, source, created_at) VALUES ($1, $2, 'admin_override', $3, 'admin', NOW())"
    )
    .bind(target_user_id)
    .bind(target_user_id)
    .bind(serde_json::json!({ "changes": changes, "reason": reason }).to_string())
    .execute(&state.db)
    .await;

    Ok(Json(json!({
        "updated": true,
        "user_id": target_user_id,
        "changes": changes,
    })))
}

pub async fn admin_stats(
    State(state): State<AppState>,
    _admin: AdminClaims, // Requires admin authorization
) -> Result<Json<AdminStats>, AppError> {
    let read_db = state.read_pool();

    let total_users = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
        .fetch_one(read_db)
        .await
        .unwrap_or(0);

    let verified_users = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE is_verified = TRUE",
    )
    .fetch_one(read_db)
    .await
    .unwrap_or(0);

    let active_users_24h = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE last_active > NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(read_db)
    .await
    .unwrap_or(0);

    let total_matches = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM matches")
        .fetch_one(read_db)
        .await
        .unwrap_or(0);

    let mutual_matches = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM matches WHERE is_mutual_match = TRUE",
    )
    .fetch_one(read_db)
    .await
    .unwrap_or(0);

    let total_messages = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages")
        .fetch_one(read_db)
        .await
        .unwrap_or(0);

    let total_spots = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM spots")
        .fetch_one(read_db)
        .await
        .unwrap_or(0);

    let student_verified_users = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE is_student_verified = TRUE",
    )
    .fetch_one(read_db)
    .await
    .unwrap_or(0);

    let active_subscriptions = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM user_subscriptions WHERE is_active = TRUE AND (end_date IS NULL OR end_date > NOW())",
    )
    .fetch_one(read_db)
    .await
    .unwrap_or(0);

    Ok(Json(AdminStats {
        total_users,
        verified_users,
        active_users_24h,
        total_matches,
        mutual_matches,
        total_messages,
        total_spots,
        student_verified_users,
        active_subscriptions,
    }))
}

// ============================================================================
// WebSocket Handlers
// ============================================================================

pub async fn ws_chat(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let match_id = params.get("match_id").cloned().unwrap_or_default();
    let token = params.get("token").cloned().unwrap_or_default();
    ws.on_upgrade(move |socket| websocket::handle_chat(socket, state, match_id, token))
}

pub async fn ws_call(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let call_id = params.get("call_id").cloned().unwrap_or_default();
    let token = params.get("token").cloned().unwrap_or_default();
    ws.on_upgrade(move |socket| websocket::handle_call(socket, state, call_id, token))
}

/// App-wide user events socket: /ws/events?token=JWT&since=<last_event_id>
/// `since` is optional — when omitted, the server replays the last 7 days of
/// outbox events for this user.
pub async fn ws_events(
    ws: WebSocketUpgrade,
    State(state): State<AppState>,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let token = params.get("token").cloned().unwrap_or_default();
    let since = params.get("since").and_then(|v| v.parse::<i64>().ok());
    ws.on_upgrade(move |socket| websocket::handle_events(socket, state, token, since))
}

// ============================================================================
// Helper Functions
// ============================================================================

struct PhotoInput {
    image: DynamicImage,
    analysis: Option<VisionAnalysis>,
}

async fn read_text_field(
    field: &mut axum::extract::multipart::Field<'_>,
    max_bytes: usize,
) -> Result<String, AppError> {
    let bytes = read_binary_field(field, max_bytes).await?;
    let value = String::from_utf8(bytes).map_err(|_| AppError::bad_request("Invalid text field"))?;
    Ok(value.trim().to_string())
}

async fn read_binary_field(
    field: &mut axum::extract::multipart::Field<'_>,
    max_bytes: usize,
) -> Result<Vec<u8>, AppError> {
    let mut data = Vec::new();
    while let Some(chunk) = field
        .chunk()
        .await
        .map_err(|_| AppError::bad_request("Failed to read upload data"))?
    {
        if data.len() + chunk.len() > max_bytes {
            return Err(AppError::bad_request("Uploaded file is too large"));
        }
        data.extend_from_slice(&chunk);
    }
    if data.is_empty() {
        return Err(AppError::bad_request("Uploaded file is empty"));
    }
    Ok(data)
}

fn calculate_age(dob: NaiveDate) -> i32 {
    let today = Utc::now().date_naive();
    let mut age = today.year() - dob.year();
    if (today.month(), today.day()) < (dob.month(), dob.day()) {
        age -= 1;
    }
    age
}

fn encode_jpeg(image: &DynamicImage) -> Result<Vec<u8>, AppError> {
    let rgb = image.to_rgb8();
    let mut buffer = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut buffer, 90);
    encoder
        .encode(&rgb, rgb.width(), rgb.height(), ColorType::Rgb8.into())
        .map_err(|_| AppError::internal("Failed to encode image"))?;
    Ok(buffer)
}

async fn cleanup_files(paths: &[String]) {
    for path in paths {
        let _ = fs::remove_file(path).await;
    }
}

/// Detect a safe file extension from the first few magic bytes.
/// Used for storing alumni degree docs in their original format.
fn detect_image_ext(bytes: &[u8]) -> &'static str {
    if bytes.len() < 4 { return "bin"; }
    match &bytes[..4] {
        [0xFF, 0xD8, 0xFF, _]       => "jpg",
        [0x89, 0x50, 0x4E, 0x47]    => "png",
        [0x47, 0x49, 0x46, _]       => "gif",
        [0x49, 0x49, 0x2A, 0x00]
        | [0x4D, 0x4D, 0x00, 0x2A]  => "tiff",
        [0x25, 0x50, 0x44, 0x46]    => "pdf",
        [0x52, 0x49, 0x46, 0x46]    => "webp",  // RIFF....WEBP
        _ => {
            // HEIC/HEIF: ftyp box at offset 4
            if bytes.len() >= 12 && &bytes[4..8] == b"ftyp" {
                let brand = &bytes[8..12];
                if brand == b"heic" || brand == b"heis" || brand == b"heix" { return "heic"; }
                if brand == b"heif" || brand == b"mif1"                     { return "heif"; }
                if brand == b"isom" || brand == b"mp41" || brand == b"mp42" { return "mp4";  }
                if brand == b"qt  "                                         { return "mov";  }
            }
            "bin"
        }
    }
}

async fn analyze_photo_bytes(
    vision: std::sync::Arc<tokio::sync::Mutex<crate::vision::VisionAnalyzer>>,
    bytes: Vec<u8>,
) -> Result<PhotoInput, AppError> {
    task::spawn_blocking(move || {
        let image = image::load_from_memory(&bytes)
            .map_err(|_| AppError::bad_request("Invalid image"))?;
        let analysis = {
            let vision = vision.blocking_lock();
            vision
                .analyze_image(&image)
                .map_err(|err| AppError::internal(err.to_string()))?
        };
        Ok(PhotoInput {
            image,
            analysis: Some(analysis),
        })
    })
    .await
    .map_err(|_| AppError::internal("Vision task failed"))?
}

/// When a viewer sees another user's profile, honor the target's privacy toggle.
/// - show_verified_name = TRUE (default)  → return users.name
/// - show_verified_name = FALSE           → return users.display_name if non-empty, else None
/// Returns None to let iOS fall back through publicName: displayName ?? name ?? "User".
fn public_name_for_viewer(
    name: Option<&str>,
    display_name: Option<&str>,
    show_verified_name: Option<bool>,
) -> Option<String> {
    if show_verified_name.unwrap_or(true) {
        name.map(|s| s.to_string())
    } else {
        display_name
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
    }
}

async fn fetch_user_by_id(db: &PgPool, user_id: i32) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"
        SELECT id, phone_number, email, name, display_name, show_verified_name, show_display_name_in_search, dob, gender, bio, location_text,
               interests, languages, looking_for, profession_category, profession_title,
               height_cm, profile_photo_url, profile_photos, profile_photo_1,
               profile_photo_2, profile_photo_3, is_profile_complete, attractiveness_score,
               is_verified, is_student_verified
        FROM users
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .fetch_optional(db)
    .await
}

async fn fetch_user_preferences(
    db: &PgPool,
    user_id: i32,
) -> Result<Option<UserPreferencesRow>, sqlx::Error> {
    sqlx::query_as::<_, UserPreferencesRow>(
        r#"
        SELECT min_age, max_age, preferred_genders, max_distance, only_verified,
               only_students, preferred_locations
        FROM user_preferences
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .fetch_optional(db)
    .await
}

async fn fetch_user_location(
    db: &PgPool,
    user_id: i32,
) -> Result<Option<UserLocationRow>, sqlx::Error> {
    sqlx::query_as::<_, UserLocationRow>(
        r#"
        SELECT latitude, longitude, city, state, country, neighborhood,
               is_fuzzy, show_exact_distance, last_updated
        FROM user_locations
        WHERE user_id = $1
        "#,
    )
    .bind(user_id)
    .fetch_optional(db)
    .await
}

async fn fetch_user_subscriptions(
    db: &PgPool,
    user_id: i32,
) -> Result<Vec<UserSubscriptionRow>, sqlx::Error> {
    let attempt = sqlx::query_as::<_, UserSubscriptionRow>(
        r#"
        SELECT id, subscription_type, pass_type, start_date, end_date, status, is_active
        FROM user_subscriptions
        WHERE user_id = $1
        ORDER BY start_date DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(db)
    .await;

    if let Ok(rows) = attempt {
        return Ok(rows);
    }

    let rows = sqlx::query_as::<_, UserSubscriptionRow>(
        r#"
        SELECT id, subscription_type, NULL::text AS pass_type, start_date, end_date, status, NULL::boolean AS is_active
        FROM user_subscriptions
        WHERE user_id = $1
        ORDER BY start_date DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(db)
    .await?;

    Ok(rows)
}

async fn fetch_user_spots(
    db: &PgPool,
    user_id: i32,
    limit: i32,
) -> Result<Vec<SpotRow>, sqlx::Error> {
    sqlx::query_as::<_, SpotRow>(
        r#"
        SELECT id, title, poster_url, renditions, expires_at, created_at, is_global, city, tags
        FROM spots
        WHERE user_id = $1
        ORDER BY created_at DESC
        LIMIT $2
        "#,
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(db)
    .await
}

async fn get_active_pass(
    db: &PgPool,
    user_id: i32,
) -> Result<Option<UserSubscriptionRow>, sqlx::Error> {
    sqlx::query_as::<_, UserSubscriptionRow>(
        r#"
        SELECT id, subscription_type, start_date, end_date, status
        FROM user_subscriptions
        WHERE user_id = $1
          AND status = 'active'
          AND (end_date IS NULL OR end_date > NOW())
        ORDER BY end_date DESC NULLS FIRST
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(db)
    .await
}

async fn get_student_discount(
    db: &PgPool,
    user_id: i32,
    config: &Config,
) -> Result<StudentStatusResponse, sqlx::Error> {
    let verification = sqlx::query_as::<_, StudentVerificationRow>(
        r#"
        SELECT id, user_id, university_name, university_type, email, student_id,
               status, verification_method, discount_tier, submitted_at, verified_at, expires_at
        FROM student_verifications
        WHERE user_id = $1 AND status = 'approved' AND (expires_at IS NULL OR expires_at > NOW())
        ORDER BY verified_at DESC
        LIMIT 1
        "#,
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?;

    Ok(match verification {
        Some(v) => {
            let tier = v.discount_tier.as_deref().map(StudentTier::from_str).unwrap_or(StudentTier::None);
            StudentStatusResponse {
                is_verified: true,
                university_name: v.university_name,
                discount_tier: Some(tier.as_str().to_string()),
                discount_percent: tier.discount_percent(config),
                expires_at: v.expires_at.map(format_datetime),
            }
        }
        None => StudentStatusResponse {
            is_verified: false,
            university_name: None,
            discount_tier: None,
            discount_percent: 0,
            expires_at: None,
        },
    })
}

async fn log_interaction_event(
    db: &PgPool,
    user_id: i32,
    target_user_id: i32,
    event_type: &str,
    slate_id: Option<&str>,
    rank: Option<i32>,
    surface: Option<&str>,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"
        INSERT INTO interaction_events (user_id, target_user_id, event_type, slate_id, rank, surface, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, NOW())
        "#,
    )
    .bind(user_id)
    .bind(target_user_id)
    .bind(event_type)
    .bind(slate_id)
    .bind(rank)
    .bind(surface)
    .execute(db)
    .await?;
    Ok(())
}

fn compute_profile_completion(user: &UserRow) -> i32 {
    if user.is_profile_complete.unwrap_or(false) {
        return 100;
    }
    let checks = [
        user.name.as_ref().map(|v| !v.is_empty()).unwrap_or(false),
        user.dob.is_some(),
        user.gender.as_ref().map(|v| !v.is_empty()).unwrap_or(false),
        user.bio.as_ref().map(|v| !v.is_empty()).unwrap_or(false),
        user.profile_photo_1
            .as_ref()
            .map(|v| !v.is_empty())
            .unwrap_or(false)
            || user.profile_photo_2
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false)
            || user.profile_photo_3
                .as_ref()
                .map(|v| !v.is_empty())
                .unwrap_or(false),
    ];
    let filled = checks.iter().filter(|c| **c).count();
    let total = checks.len().max(1);
    let percent = ((filled as f32 / total as f32) * 100.0).round() as i32;
    percent.min(100)
}

fn get_user_photos(user: &UserRow) -> Vec<String> {
    if let Some(Value::Array(items)) = &user.profile_photos {
        let photos: Vec<String> = items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        if !photos.is_empty() {
            return photos;
        }
    }

    if let Some(csv) = &user.profile_photo_url {
        let photos: Vec<String> = csv
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        if !photos.is_empty() {
            return photos;
        }
    }

    let mut photos = Vec::new();
    if let Some(value) = &user.profile_photo_1 {
        if !value.is_empty() {
            photos.push(value.clone());
        }
    }
    if let Some(value) = &user.profile_photo_2 {
        if !value.is_empty() {
            photos.push(value.clone());
        }
    }
    if let Some(value) = &user.profile_photo_3 {
        if !value.is_empty() {
            photos.push(value.clone());
        }
    }
    photos
}

/// Build `DiscoverProfile` cards for a set of user ids, reusing the discover
/// feed's shape. Used to enrich the auto-match and agent-matchmaker responses so
/// clients can render the same cards without a second profile fetch. Distance is
/// left None (no viewer location context here).
pub async fn fetch_profile_cards(
    state: &AppState,
    ids: &[i32],
) -> std::collections::HashMap<i32, DiscoverProfile> {
    if ids.is_empty() {
        return std::collections::HashMap::new();
    }
    let ids64: Vec<i64> = ids.iter().map(|x| *x as i64).collect();
    let rows = sqlx::query_as::<_, DiscoverUserRow>(
        "SELECT u.id, u.name, u.display_name, u.show_verified_name, u.dob, u.gender, u.bio, \
                u.profile_photo_url, u.profile_photos, u.profile_photo_1, u.profile_photo_2, u.profile_photo_3, \
                u.is_verified, u.attractiveness_score, u.looking_for, u.profession_title, u.height_cm, \
                l.city, l.latitude, l.longitude \
         FROM users u LEFT JOIN user_locations l ON l.user_id = u.id \
         WHERE u.id = ANY($1)",
    )
    .bind(&ids64)
    .fetch_all(state.read_pool())
    .await
    .unwrap_or_default();

    let uni_map = batch_lookup_university(state.read_pool(), ids).await.unwrap_or_default();

    let mut cards = std::collections::HashMap::new();
    for c in rows {
        let photos = get_photos_from_row(&c);
        let public_name = public_name_for_viewer(
            c.name.as_deref(),
            c.display_name.as_deref(),
            c.show_verified_name,
        );
        let uni = uni_map.get(&c.id);
        cards.insert(
            c.id,
            DiscoverProfile {
                id: c.id,
                name: public_name,
                display_name: c.display_name.clone(),
                age: c.dob.map(calculate_age),
                gender: c.gender.clone(),
                bio: c.bio.clone(),
                photos,
                is_verified: c.is_verified.unwrap_or(false),
                looking_for: c.looking_for.clone(),
                profession_title: c.profession_title.clone(),
                height_cm: c.height_cm,
                distance_km: None,
                distance_text: None,
                city: c.city.clone(),
                compatibility_score: None,
                university: uni.map(|(n, _)| n.clone()),
                university_tier: uni.map(|(_, t)| t.clone()),
                interaction_status: None,
                super_liked_you: None,
            },
        );
    }
    cards
}

fn get_photos_from_row(row: &DiscoverUserRow) -> Vec<String> {
    if let Some(Value::Array(items)) = &row.profile_photos {
        let photos: Vec<String> = items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        if !photos.is_empty() {
            return photos;
        }
    }

    if let Some(csv) = &row.profile_photo_url {
        let photos: Vec<String> = csv
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        if !photos.is_empty() {
            return photos;
        }
    }

    let mut photos = Vec::new();
    if let Some(value) = &row.profile_photo_1 {
        if !value.is_empty() {
            photos.push(value.clone());
        }
    }
    if let Some(value) = &row.profile_photo_2 {
        if !value.is_empty() {
            photos.push(value.clone());
        }
    }
    if let Some(value) = &row.profile_photo_3 {
        if !value.is_empty() {
            photos.push(value.clone());
        }
    }
    photos
}

fn extract_photo_paths(user: &UserRow) -> Vec<String> {
    get_user_photos(user)
}

fn json_array_or_empty(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect(),
        Some(Value::String(single)) if !single.is_empty() => vec![single.clone()],
        _ => Vec::new(),
    }
}

fn is_json_array_nonempty(value: &Value) -> bool {
    match value {
        Value::Array(items) => !items.is_empty(),
        Value::String(value) => !value.is_empty(),
        _ => false,
    }
}

fn format_date(date: NaiveDate) -> String {
    date.format("%Y-%m-%d").to_string()
}

fn format_datetime(dt: NaiveDateTime) -> String {
    dt.format("%Y-%m-%dT%H:%M:%S").to_string()
}

fn cosine_similarity(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 {
        None
    } else {
        Some((dot / denom).clamp(-1.0, 1.0))
    }
}

/// Haversine distance in kilometers
fn haversine_km(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
    let r = 6371.0; // Earth's radius in km
    let d_lat = (lat2 - lat1).to_radians();
    let d_lon = (lon2 - lon1).to_radians();
    let lat1 = lat1.to_radians();
    let lat2 = lat2.to_radians();

    let a = (d_lat / 2.0).sin().powi(2) + lat1.cos() * lat2.cos() * (d_lon / 2.0).sin().powi(2);
    let c = 2.0 * a.sqrt().asin();
    r * c
}

fn format_distance(km: f64) -> String {
    if km < 1.0 {
        format!("{:.0} m", km * 1000.0)
    } else {
        format!("{:.1} km", km)
    }
}

fn fuzzy_distance(km: f64) -> f64 {
    // Round to nearest 5km for privacy
    (km / 5.0).round() * 5.0
}

fn format_fuzzy_distance(km: f64) -> String {
    let fuzzy = fuzzy_distance(km);
    if fuzzy < 5.0 {
        "< 5 km".to_string()
    } else {
        format!("~{:.0} km", fuzzy)
    }
}

fn determine_university_tier(domain: &str, name: Option<&str>) -> (String, StudentTier) {
    // ── Top Private (Ivy League + Global Elite Private) ─────────────────
    let top_private = [
        // US — Ivy League & Elite Private
        ("harvard.edu", "Harvard University"),
        ("stanford.edu", "Stanford University"),
        ("mit.edu", "MIT"),
        ("yale.edu", "Yale University"),
        ("princeton.edu", "Princeton University"),
        ("columbia.edu", "Columbia University"),
        ("upenn.edu", "University of Pennsylvania"),
        ("brown.edu", "Brown University"),
        ("dartmouth.edu", "Dartmouth College"),
        ("cornell.edu", "Cornell University"),
        ("caltech.edu", "Caltech"),
        ("uchicago.edu", "University of Chicago"),
        ("duke.edu", "Duke University"),
        ("northwestern.edu", "Northwestern University"),
        ("jhu.edu", "Johns Hopkins University"),
        ("rice.edu", "Rice University"),
        ("vanderbilt.edu", "Vanderbilt University"),
        ("wustl.edu", "Washington University in St. Louis"),
        ("nd.edu", "University of Notre Dame"),
        ("cmu.edu", "Carnegie Mellon University"),
        ("emory.edu", "Emory University"),
        ("georgetown.edu", "Georgetown University"),
        ("nyu.edu", "New York University"),
        ("usc.edu", "University of Southern California"),
        // India — IITs (Indian Institutes of Technology)
        ("iitb.ac.in", "IIT Bombay"),
        ("iitd.ac.in", "IIT Delhi"),
        ("iitm.ac.in", "IIT Madras"),
        ("iitkgp.ac.in", "IIT Kharagpur"),
        ("iitk.ac.in", "IIT Kanpur"),
        ("iitr.ac.in", "IIT Roorkee"),
        ("iitg.ac.in", "IIT Guwahati"),
        ("iith.ac.in", "IIT Hyderabad"),
        ("iitbbs.ac.in", "IIT Bhubaneswar"),
        ("iitgn.ac.in", "IIT Gandhinagar"),
        ("iiti.ac.in", "IIT Indore"),
        ("iitj.ac.in", "IIT Jodhpur"),
        ("iitp.ac.in", "IIT Patna"),
        ("iitrpr.ac.in", "IIT Ropar"),
        ("iitmandi.ac.in", "IIT Mandi"),
        ("iitbhilai.ac.in", "IIT Bhilai"),
        ("iitgoa.ac.in", "IIT Goa"),
        ("iitjammu.ac.in", "IIT Jammu"),
        ("iitdh.ac.in", "IIT Dharwad"),
        ("iitpkd.ac.in", "IIT Palakkad"),
        ("iittirupati.ac.in", "IIT Tirupati"),
        ("iitism.ac.in", "IIT (ISM) Dhanbad"),
        // India — IIMs (Indian Institutes of Management)
        ("iima.ac.in", "IIM Ahmedabad"),
        ("iimb.ac.in", "IIM Bangalore"),
        ("iimc.ac.in", "IIM Calcutta"),
        ("iiml.ac.in", "IIM Lucknow"),
        ("iimk.ac.in", "IIM Kozhikode"),
        ("iimi.ac.in", "IIM Indore"),
        ("iimshillong.ac.in", "IIM Shillong"),
        ("iimranchi.ac.in", "IIM Ranchi"),
        ("iimrohtak.ac.in", "IIM Rohtak"),
        ("iimkashipur.ac.in", "IIM Kashipur"),
        ("iimtrichy.ac.in", "IIM Tiruchirappalli"),
        ("iimu.ac.in", "IIM Udaipur"),
        ("iimnagpur.ac.in", "IIM Nagpur"),
        ("iimbg.ac.in", "IIM Bodh Gaya"),
        ("iimamritsar.ac.in", "IIM Amritsar"),
        ("iimj.ac.in", "IIM Jammu"),
        // India — BITS, ISB, IISC
        ("bits-pilani.ac.in", "BITS Pilani"),
        ("pilani.bits-pilani.ac.in", "BITS Pilani"),
        ("goa.bits-pilani.ac.in", "BITS Pilani Goa"),
        ("hyderabad.bits-pilani.ac.in", "BITS Pilani Hyderabad"),
        ("isb.edu", "Indian School of Business"),
        ("iisc.ac.in", "Indian Institute of Science"),
        // India — NITs (top ones)
        ("nitk.ac.in", "NIT Karnataka (Surathkal)"),
        ("nitw.ac.in", "NIT Warangal"),
        ("nitt.edu", "NIT Tiruchirappalli"),
        ("nitc.ac.in", "NIT Calicut"),
        ("svnit.ac.in", "NIT Surat"),
        ("mnnit.ac.in", "MNNIT Allahabad"),
        ("vnit.ac.in", "VNIT Nagpur"),
        ("manit.ac.in", "MANIT Bhopal"),
        ("nitdgp.ac.in", "NIT Durgapur"),
        ("nitr.ac.in", "NIT Rourkela"),
        // India — Top Private
        ("vit.ac.in", "VIT Vellore"),
        ("srmist.edu.in", "SRM University"),
        ("manipal.edu", "Manipal University"),
        ("amity.edu", "Amity University"),
        ("lpu.in", "Lovely Professional University"),
        ("christuniversity.in", "Christ University"),
        ("flame.edu.in", "FLAME University"),
        ("ashoka.edu.in", "Ashoka University"),
        ("jgu.edu.in", "Jindal Global University"),
        ("shiv-nadar.org", "Shiv Nadar University"),
        ("plaksha.edu.in", "Plaksha University"),
        // India — Central & State Universities
        ("du.ac.in", "Delhi University"),
        ("jnu.ac.in", "Jawaharlal Nehru University"),
        ("bhu.ac.in", "Banaras Hindu University"),
        ("amu.ac.in", "Aligarh Muslim University"),
        ("uohyd.ac.in", "University of Hyderabad"),
        ("jadavpur.edu", "Jadavpur University"),
        ("annauniv.edu", "Anna University"),
        ("ipu.ac.in", "IP University Delhi"),
        ("mu.ac.in", "Mumbai University"),
        ("unipune.ac.in", "Savitribai Phule Pune University"),
        ("bangaloreuniversity.ac.in", "Bangalore University"),
        ("osmania.ac.in", "Osmania University"),
        // India — Medical
        ("aiims.edu", "AIIMS New Delhi"),
        ("aiimsrishikesh.edu.in", "AIIMS Rishikesh"),
        ("jipmer.edu.in", "JIPMER Puducherry"),
        ("cmc-vellore.edu", "CMC Vellore"),
        // India — Law
        ("nls.ac.in", "National Law School Bangalore"),
        ("nalsar.ac.in", "NALSAR Hyderabad"),
        ("nujs.edu", "NUJS Kolkata"),
        ("nludelhi.ac.in", "NLU Delhi"),
        ("gnlu.ac.in", "GNLU Gujarat"),
        // India — Design & Architecture
        ("nid.edu", "National Institute of Design"),
        ("nift.ac.in", "NIFT"),
        ("spa.ac.in", "School of Planning & Architecture"),
        // UK — Russell Group & Elite
        ("ox.ac.uk", "University of Oxford"),
        ("cam.ac.uk", "University of Cambridge"),
        ("imperial.ac.uk", "Imperial College London"),
        ("lse.ac.uk", "London School of Economics"),
        ("ucl.ac.uk", "University College London"),
        ("kcl.ac.uk", "King's College London"),
        ("ed.ac.uk", "University of Edinburgh"),
        ("manchester.ac.uk", "University of Manchester"),
        ("warwick.ac.uk", "University of Warwick"),
        ("bristol.ac.uk", "University of Bristol"),
        ("st-andrews.ac.uk", "University of St Andrews"),
        ("dur.ac.uk", "Durham University"),
        ("bath.ac.uk", "University of Bath"),
        ("lboro.ac.uk", "Loughborough University"),
        // Canada
        ("utoronto.ca", "University of Toronto"),
        ("ubc.ca", "University of British Columbia"),
        ("mcgill.ca", "McGill University"),
        ("uwaterloo.ca", "University of Waterloo"),
        ("ualberta.ca", "University of Alberta"),
        ("queensu.ca", "Queen's University"),
        ("wlu.ca", "Wilfrid Laurier University"),
        // Australia — Group of Eight
        ("unimelb.edu.au", "University of Melbourne"),
        ("sydney.edu.au", "University of Sydney"),
        ("unsw.edu.au", "UNSW Sydney"),
        ("anu.edu.au", "Australian National University"),
        ("uq.edu.au", "University of Queensland"),
        ("monash.edu", "Monash University"),
        // Singapore
        ("nus.edu.sg", "National University of Singapore"),
        ("ntu.edu.sg", "Nanyang Technological University"),
        ("smu.edu.sg", "Singapore Management University"),
        // Europe
        ("ethz.ch", "ETH Zurich"),
        ("epfl.ch", "EPFL"),
        ("tum.de", "Technical University of Munich"),
        ("lmu.de", "LMU Munich"),
        ("sorbonne-universite.fr", "Sorbonne University"),
        ("polytechnique.fr", "École Polytechnique"),
        ("uva.nl", "University of Amsterdam"),
        ("tudelft.nl", "TU Delft"),
        // Middle East & Africa
        ("kaust.edu.sa", "KAUST"),
        ("aud.edu", "American University Dubai"),
        ("uct.ac.za", "University of Cape Town"),
        // East Asia
        ("u-tokyo.ac.jp", "University of Tokyo"),
        ("kyoto-u.ac.jp", "Kyoto University"),
        ("snu.ac.kr", "Seoul National University"),
        ("kaist.ac.kr", "KAIST"),
        ("tsinghua.edu.cn", "Tsinghua University"),
        ("pku.edu.cn", "Peking University"),
        ("hku.hk", "University of Hong Kong"),
        ("cuhk.edu.hk", "Chinese University of Hong Kong"),
    ];

    // ── Top Public Universities ────────────────────────────────────────
    let top_public = [
        // US
        ("berkeley.edu", "UC Berkeley"),
        ("ucla.edu", "UCLA"),
        ("umich.edu", "University of Michigan"),
        ("virginia.edu", "University of Virginia"),
        ("unc.edu", "UNC Chapel Hill"),
        ("utexas.edu", "UT Austin"),
        ("utdallas.edu", "UT Dallas"),
        ("uta.edu", "UT Arlington"),
        ("utsa.edu", "UT San Antonio"),
        ("gatech.edu", "Georgia Tech"),
        ("wisc.edu", "University of Wisconsin"),
        ("illinois.edu", "UIUC"),
        ("washington.edu", "University of Washington"),
        ("purdue.edu", "Purdue University"),
        ("osu.edu", "Ohio State University"),
        ("psu.edu", "Penn State"),
        ("umd.edu", "University of Maryland"),
        ("umn.edu", "University of Minnesota"),
        ("ufl.edu", "University of Florida"),
        ("tamu.edu", "Texas A&M University"),
        ("ucdavis.edu", "UC Davis"),
        ("ucsd.edu", "UC San Diego"),
        ("ucsb.edu", "UC Santa Barbara"),
        ("uci.edu", "UC Irvine"),
        ("asu.edu", "Arizona State University"),
        ("rutgers.edu", "Rutgers University"),
        ("indiana.edu", "Indiana University"),
        ("uga.edu", "University of Georgia"),
        ("ncsu.edu", "NC State University"),
        ("vt.edu", "Virginia Tech"),
        ("colorado.edu", "University of Colorado Boulder"),
        ("iowa.edu", "University of Iowa"),
        ("msu.edu", "Michigan State University"),
    ];

    for (d, n) in &top_private {
        if domain.ends_with(d) {
            return (n.to_string(), StudentTier::TopPrivate);
        }
    }

    for (d, n) in &top_public {
        if domain.ends_with(d) {
            return (n.to_string(), StudentTier::TopPublic);
        }
    }

    // Also check against the universities table in DB (handled at verification time)
    // The hardcoded lists above are for instant tier classification without DB hit

    // Recognized educational domains (global)
    let edu_domains = [
        ".edu",        // US standard
        ".ac.in",      // India
        ".edu.in",     // India
        ".res.in",     // India research institutes
        ".ac.uk",      // UK
        ".edu.au",     // Australia
        ".ac.nz",      // New Zealand
        ".edu.sg",     // Singapore
        ".edu.my",     // Malaysia
        ".ac.jp",      // Japan
        ".ac.kr",      // South Korea
        ".edu.cn",     // China
        ".edu.hk",     // Hong Kong
        ".edu.tw",     // Taiwan
        ".ac.za",      // South Africa
        ".edu.ng",     // Nigeria
        ".edu.eg",     // Egypt
        ".edu.sa",     // Saudi Arabia
        ".ac.ae",      // UAE
        ".edu.pk",     // Pakistan
        ".edu.bd",     // Bangladesh
        ".edu.lk",     // Sri Lanka
        ".edu.np",     // Nepal
        ".ac.th",      // Thailand
        ".edu.ph",     // Philippines
        ".edu.vn",     // Vietnam
        ".ac.id",      // Indonesia
        ".edu.br",     // Brazil
        ".edu.mx",     // Mexico
        ".edu.co",     // Colombia
        ".edu.ar",     // Argentina
        ".edu.pe",     // Peru
        ".edu.cl",     // Chile
        ".ac.il",      // Israel
        ".edu.tr",     // Turkey
        ".edu.ru",     // Russia
        ".edu.pl",     // Poland
        ".ac.be",      // Belgium
        ".edu.es",     // Spain
        ".edu.it",     // Italy
    ];

    for edu_domain in &edu_domains {
        if domain.ends_with(edu_domain) {
            let uni_name = name
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("University ({})", domain));
            return (uni_name, StudentTier::Regular);
        }
    }

    // European universities often use custom TLDs — check common patterns
    let eu_patterns = [".uni-", ".tu-", ".rwth-", ".kit.", ".fu-berlin", ".hu-berlin"];
    for pattern in &eu_patterns {
        if domain.contains(pattern) && (domain.ends_with(".de") || domain.ends_with(".at") || domain.ends_with(".ch")) {
            let uni_name = name
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("University ({})", domain));
            return (uni_name, StudentTier::Regular);
        }
    }

    (String::new(), StudentTier::None)
}

// ============================================================================
// ML Training Endpoints - Raw SQL (No ORM)
// ============================================================================

/// Store/update user embedding vector for ML training
#[derive(Deserialize)]
pub struct UpdateEmbeddingPayload {
    pub user_id: i32,
    pub embedding: Vec<f64>,
    pub recency_stats: Option<Value>,
}

pub async fn update_user_embedding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdateEmbeddingPayload>,
) -> Result<Json<Value>, AppError> {
    // Admin/ML service auth check
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let embedding_json = serde_json::to_value(&payload.embedding)
        .map_err(|_| AppError::bad_request("Invalid embedding format"))?;

    sqlx::query(
        r#"
        INSERT INTO user_features (user_id, embedding, recency_stats, updated_at)
        VALUES ($1, $2, $3, NOW())
        ON CONFLICT (user_id) DO UPDATE SET
            embedding = $2,
            recency_stats = COALESCE($3, user_features.recency_stats),
            updated_at = NOW()
        "#,
    )
    .bind(payload.user_id)
    .bind(&embedding_json)
    .bind(&payload.recency_stats)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({ "message": "Embedding updated", "user_id": payload.user_id })))
}

/// Get user embedding for inference
pub async fn get_user_embedding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let user_id: i32 = params
        .get("user_id")
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| AppError::bad_request("user_id is required"))?;

    #[derive(sqlx::FromRow)]
    struct FeatureRow {
        embedding: Option<Value>,
        recency_stats: Option<Value>,
        updated_at: Option<NaiveDateTime>,
    }

    let row = sqlx::query_as::<_, FeatureRow>(
        "SELECT embedding, recency_stats, updated_at FROM user_features WHERE user_id = $1",
    )
    .bind(user_id)
    .fetch_optional(state.read_pool())
    .await?;

    match row {
        Some(r) => Ok(Json(json!({
            "user_id": user_id,
            "embedding": r.embedding,
            "recency_stats": r.recency_stats,
            "updated_at": r.updated_at.map(|t| t.format("%Y-%m-%dT%H:%M:%S").to_string()),
        }))),
        None => Err(AppError::not_found("User embedding not found")),
    }
}

/// Batch get embeddings for multiple users (efficient for ML training)
#[derive(Deserialize)]
pub struct BatchEmbeddingsRequest {
    pub user_ids: Vec<i64>,
}

pub async fn get_batch_embeddings(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<BatchEmbeddingsRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    if payload.user_ids.is_empty() || payload.user_ids.len() > 1000 {
        return Err(AppError::bad_request("user_ids must have 1-1000 items"));
    }

    #[derive(sqlx::FromRow, Serialize)]
    struct EmbeddingResult {
        user_id: i32,
        embedding: Option<Value>,
        recency_stats: Option<Value>,
    }

    // Build parameterized query for batch fetch
    let placeholders: Vec<String> = (1..=payload.user_ids.len())
        .map(|i| format!("${}", i))
        .collect();
    let query = format!(
        "SELECT user_id, embedding, recency_stats FROM user_features WHERE user_id IN ({})",
        placeholders.join(", ")
    );

    let mut q = sqlx::query_as::<_, EmbeddingResult>(&query);
    for id in &payload.user_ids {
        q = q.bind(*id);
    }

    let results = q.fetch_all(state.read_pool()).await?;

    Ok(Json(json!({
        "embeddings": results,
        "count": results.len(),
    })))
}

/// Update contextual bandit arm statistics
#[derive(Deserialize)]
pub struct UpdateBanditArmPayload {
    pub arm_id: String,
    pub arm_type: Option<String>, // "global", "user", "context"
    pub user_id: Option<i64>,
    pub a_matrix: Option<Value>,  // Context matrix for LinUCB
    pub b_vector: Option<Value>,  // Reward vector
    pub theta_vector: Option<Value>, // Learned weights
    pub num_pulls: Option<i32>,
    pub total_reward: Option<f64>,
}

pub async fn update_bandit_arm(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UpdateBanditArmPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let arm_type = payload.arm_type.as_deref().unwrap_or("global");

    sqlx::query(
        r#"
        INSERT INTO bandit_arm_stats (arm_id, arm_type, user_id, a_matrix, b_vector, theta_vector, num_pulls, total_reward, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, COALESCE($7, 0), COALESCE($8, 0), NOW())
        ON CONFLICT (arm_id, arm_type) WHERE user_id IS NULL
        DO UPDATE SET
            a_matrix = COALESCE($4, bandit_arm_stats.a_matrix),
            b_vector = COALESCE($5, bandit_arm_stats.b_vector),
            theta_vector = COALESCE($6, bandit_arm_stats.theta_vector),
            num_pulls = COALESCE($7, bandit_arm_stats.num_pulls),
            total_reward = COALESCE($8, bandit_arm_stats.total_reward),
            updated_at = NOW()
        "#,
    )
    .bind(&payload.arm_id)
    .bind(arm_type)
    .bind(payload.user_id)
    .bind(&payload.a_matrix)
    .bind(&payload.b_vector)
    .bind(&payload.theta_vector)
    .bind(payload.num_pulls)
    .bind(payload.total_reward)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({ "message": "Bandit arm updated", "arm_id": payload.arm_id })))
}

/// Get bandit arm stats for inference
pub async fn get_bandit_arm(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let arm_id = params
        .get("arm_id")
        .ok_or_else(|| AppError::bad_request("arm_id is required"))?;

    #[derive(sqlx::FromRow, Serialize)]
    struct BanditRow {
        id: i64,
        arm_id: String,
        arm_type: Option<String>,
        user_id: Option<i64>,
        a_matrix: Option<Value>,
        b_vector: Option<Value>,
        theta_vector: Option<Value>,
        num_pulls: Option<i32>,
        total_reward: Option<f64>,
        updated_at: Option<NaiveDateTime>,
    }

    let row = sqlx::query_as::<_, BanditRow>(
        "SELECT id, arm_id, arm_type, user_id, a_matrix, b_vector, theta_vector, num_pulls, total_reward, updated_at FROM bandit_arm_stats WHERE arm_id = $1",
    )
    .bind(arm_id)
    .fetch_optional(state.read_pool())
    .await?;

    match row {
        Some(r) => Ok(Json(json!(r))),
        None => Err(AppError::not_found("Bandit arm not found")),
    }
}

/// Log reward for RL training (converts user actions to rewards)
#[derive(Deserialize)]
pub struct LogRewardPayload {
    pub user_id: i32,
    pub target_user_id: i32,
    pub event_type: String,  // "like", "pass", "message", "match", "unmatch", "block"
    pub slate_id: Option<String>,
    pub rank: Option<i32>,
    pub surface: Option<String>,
    pub reward: Option<f64>,  // Custom reward, otherwise computed from event_type
    pub delay_ms: Option<i32>,
    pub metadata: Option<Value>,
}

pub async fn log_reward(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LogRewardPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    // Compute reward based on event type if not provided
    let reward = payload.reward.unwrap_or_else(|| {
        match payload.event_type.as_str() {
            "like" => 1.0,
            "match" => 5.0,
            "message" => 3.0,
            "voice_message" => 4.0,
            "call_answered" => 6.0,
            "pass" => -0.5,
            "unmatch" => -2.0,
            "block" => -5.0,
            "report" => -10.0,
            "impression" => 0.0,
            _ => 0.0,
        }
    });

    sqlx::query(
        r#"
        INSERT INTO interaction_events (user_id, target_user_id, event_type, slate_id, rank, surface, reward, delay_ms, event_metadata, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW())
        "#,
    )
    .bind(payload.user_id)
    .bind(payload.target_user_id)
    .bind(&payload.event_type)
    .bind(&payload.slate_id)
    .bind(payload.rank)
    .bind(&payload.surface)
    .bind(reward)
    .bind(payload.delay_ms)
    .bind(&payload.metadata)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "message": "Reward logged",
        "event_type": payload.event_type,
        "reward": reward
    })))
}

/// Get interaction events for ML training (batch export)
#[derive(Deserialize)]
pub struct GetEventsParams {
    pub since: Option<String>,  // ISO timestamp
    pub event_types: Option<String>,  // Comma-separated
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

pub async fn get_training_events(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<GetEventsParams>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let limit = params.limit.unwrap_or(1000).min(10000);
    let offset = params.offset.unwrap_or(0);

    #[derive(sqlx::FromRow, Serialize)]
    struct EventRow {
        id: i64,
        user_id: i32,
        target_user_id: i32,
        event_type: String,
        slate_id: Option<String>,
        rank: Option<i32>,
        surface: Option<String>,
        reward: Option<f64>,
        delay_ms: Option<i32>,
        event_metadata: Option<Value>,
        created_at: Option<NaiveDateTime>,
    }

    let read_db = state.read_pool();
    let events = if let Some(since) = &params.since {
        sqlx::query_as::<_, EventRow>(
            r#"
            SELECT id, user_id, target_user_id, event_type, slate_id, rank, surface, reward, delay_ms, event_metadata, created_at
            FROM interaction_events
            WHERE created_at >= $1::timestamp
            ORDER BY created_at ASC
            LIMIT $2 OFFSET $3
            "#,
        )
        .bind(since)
        .bind(limit)
        .bind(offset)
        .fetch_all(read_db)
        .await?
    } else {
        sqlx::query_as::<_, EventRow>(
            r#"
            SELECT id, user_id, target_user_id, event_type, slate_id, rank, surface, reward, delay_ms, event_metadata, created_at
            FROM interaction_events
            ORDER BY created_at DESC
            LIMIT $1 OFFSET $2
            "#,
        )
        .bind(limit)
        .bind(offset)
        .fetch_all(read_db)
        .await?
    };

    Ok(Json(json!({
        "events": events,
        "count": events.len(),
        "limit": limit,
        "offset": offset,
    })))
}

/// Get user interaction history for personalization
pub async fn get_user_interactions(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let requesting_user = decode_access_token(&token, &state.config.secret_key)?;

    let user_id: i32 = params
        .get("user_id")
        .and_then(|v| v.parse().ok())
        .unwrap_or(requesting_user);

    let limit: i32 = params
        .get("limit")
        .and_then(|v| v.parse().ok())
        .unwrap_or(100)
        .min(1000);

    #[derive(sqlx::FromRow, Serialize)]
    struct InteractionSummary {
        target_user_id: i32,
        event_type: String,
        count: Option<i64>,
        total_reward: Option<f64>,
        last_event: Option<NaiveDateTime>,
    }

    let interactions = sqlx::query_as::<_, InteractionSummary>(
        r#"
        SELECT target_user_id, event_type, COUNT(*) as count, SUM(reward) as total_reward, MAX(created_at) as last_event
        FROM interaction_events
        WHERE user_id = $1
        GROUP BY target_user_id, event_type
        ORDER BY last_event DESC
        LIMIT $2
        "#,
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(state.read_pool())
    .await?;

    Ok(Json(json!({
        "user_id": user_id,
        "interactions": interactions,
    })))
}

/// Bulk update attractiveness scores from ML model
#[derive(Deserialize)]
pub struct BulkScoreUpdate {
    pub scores: Vec<UserScoreUpdate>,
}

#[derive(Deserialize)]
pub struct UserScoreUpdate {
    pub user_id: i32,
    pub attractiveness_score: Option<f64>,
    pub ai_embedding: Option<Vec<f64>>,
}

pub async fn bulk_update_scores(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<BulkScoreUpdate>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    if payload.scores.is_empty() || payload.scores.len() > 1000 {
        return Err(AppError::bad_request("scores must have 1-1000 items"));
    }

    let mut updated = 0;
    for score in &payload.scores {
        let embedding_json = score.ai_embedding.as_ref().map(|e| serde_json::to_value(e).ok()).flatten();

        let result = sqlx::query(
            r#"
            UPDATE users SET
                attractiveness_score = COALESCE($2, attractiveness_score),
                ai_embedding = COALESCE($3, ai_embedding),
                updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(score.user_id)
        .bind(score.attractiveness_score)
        .bind(&embedding_json)
        .execute(&state.db)
        .await;

        if result.is_ok() {
            updated += 1;
        }
    }

    Ok(Json(json!({
        "message": "Scores updated",
        "updated": updated,
        "total": payload.scores.len(),
    })))
}

/// Store spot embedding for content-based recommendations
#[derive(Deserialize)]
pub struct SpotEmbeddingPayload {
    pub spot_id: i64,
    pub embedding: Vec<f64>,
}

pub async fn update_spot_embedding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SpotEmbeddingPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let embedding_json = serde_json::to_value(&payload.embedding)
        .map_err(|_| AppError::bad_request("Invalid embedding format"))?;

    sqlx::query(
        r#"
        INSERT INTO spot_embeddings (spot_id, embedding, created_at)
        VALUES ($1, $2, NOW())
        ON CONFLICT (spot_id) DO UPDATE SET
            embedding = $2,
            created_at = NOW()
        "#,
    )
    .bind(payload.spot_id)
    .bind(&embedding_json)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({ "message": "Spot embedding updated", "spot_id": payload.spot_id })))
}

// ============================================================================
// REEL-BASED DATING SYSTEM (Private Messages Only - No Public Comments)
// ML learns: interest patterns, effort levels, what gets responses
// ============================================================================

/// Create a new reel
#[derive(Deserialize)]
pub struct CreateReelPayload {
    pub video_url: String,
    pub thumbnail_url: Option<String>,
    pub duration_sec: Option<i32>,
    pub caption: Option<String>,
    pub audio_track: Option<String>,
    pub tags: Option<Vec<String>>,
    pub category: Option<String>,
    pub location_tag: Option<String>,
}

/// POST /reels/upload-video — accepts multipart video binary, streams to disk, returns video_url.
/// Called immediately when the user picks a video (pre-upload before tapping Post).
/// Streams chunks directly to disk — never buffers the full video in RAM.
pub async fn upload_reel_video(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let mut mime_type: Option<String> = None;
    let mut disk_path: Option<String> = None;

    while let Some(mut field) = multipart.next_field().await.map_err(|_| AppError::bad_request("Invalid multipart"))? {
        let name = field.name().unwrap_or("").to_string();
        if name == "video" {
            let ct = field.content_type().map(|v| v.to_string()).unwrap_or_default();
            if !ct.starts_with("video/") {
                return Err(AppError::bad_request("Field 'video' must be a video file"));
            }
            mime_type = Some(ct.clone());

            let ext = if ct.contains("quicktime") || ct.contains("mov") {
                "mov"
            } else if ct.contains("webm") {
                "webm"
            } else {
                "mp4"
            };

            let upload_dir = &state.config.upload_dir;
            fs::create_dir_all(format!("{}/reels", upload_dir))
                .await
                .map_err(|_| AppError::internal("Failed to create reels directory"))?;

            let filename = format!("reels/{}_{}_{}.{}", user_id, Utc::now().timestamp(), Uuid::new_v4(), ext);
            let path = format!("{}/{}", upload_dir, filename);

            // Stream chunks directly to disk — no full RAM buffer
            let mut file = fs::File::create(&path).await
                .map_err(|_| AppError::internal("Failed to create video file"))?;
            let mut total_bytes: usize = 0;
            let max_bytes = state.config.max_video_bytes;

            use tokio::io::AsyncWriteExt;
            while let Some(chunk) = field.chunk().await.map_err(|_| AppError::bad_request("Read error"))? {
                total_bytes += chunk.len();
                if total_bytes > max_bytes {
                    drop(file);
                    let _ = fs::remove_file(&path).await;
                    return Err(AppError::bad_request("Video file too large"));
                }
                file.write_all(&chunk).await
                    .map_err(|_| AppError::internal("Failed to write video chunk"))?;
            }
            file.flush().await.map_err(|_| AppError::internal("Failed to flush video"))?;

            disk_path = Some(filename);
        } else {
            while field.chunk().await.map_err(|_| AppError::bad_request("Read error"))?.is_some() {}
        }
    }

    let filename = disk_path.ok_or_else(|| AppError::bad_request("Missing 'video' field"))?;
    let video_url = format!("/uploads/{}", filename);
    Ok(Json(json!({ "video_url": video_url })))
}

pub async fn create_reel(
    State(state): State<AppState>,
    headers: HeaderMap,
    mut multipart: Multipart,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let mut video_bytes: Option<Vec<u8>> = None;
    let mut video_mime: Option<String> = None;
    let mut caption: Option<String> = None;
    let mut category: Option<String> = None;
    let mut tags_str: Option<String> = None;
    let mut music_id: Option<String> = None;
    let mut music_title: Option<String> = None;
    let mut music_artist: Option<String> = None;
    let mut music_artwork_url: Option<String> = None;
    let mut music_preview_url: Option<String> = None;
    let mut music_duration_ms: Option<i32> = None;
    let mut music_start_ms: Option<i32> = None;
    let mut music_genre: Option<String> = None;
    let mut location: Option<String> = None;
    let mut latitude: Option<f64> = None;
    let mut longitude: Option<f64> = None;

    while let Some(mut field) = multipart
        .next_field()
        .await
        .map_err(|_| AppError::bad_request("Invalid multipart data"))?
    {
        match field.name().unwrap_or("") {
            "video" => {
                video_mime = Some(field.content_type().unwrap_or("video/mp4").to_string());
                video_bytes = Some(read_binary_field(&mut field, state.config.max_video_bytes).await?);
            }
            "caption" => { caption = field.text().await.ok(); }
            "category" => { category = field.text().await.ok(); }
            "tags" => { tags_str = field.text().await.ok(); }
            "music_id" => { music_id = field.text().await.ok(); }
            "music_title" => { music_title = field.text().await.ok(); }
            "music_artist" => { music_artist = field.text().await.ok(); }
            "music_artwork_url" => { music_artwork_url = field.text().await.ok(); }
            "music_preview_url" => { music_preview_url = field.text().await.ok(); }
            "music_duration_ms" => { music_duration_ms = field.text().await.ok().and_then(|v| v.parse().ok()); }
            "music_start_ms" => { music_start_ms = field.text().await.ok().and_then(|v| v.parse().ok()); }
            "music_genre" => { music_genre = field.text().await.ok(); }
            "location" => { location = field.text().await.ok(); }
            "latitude" => { latitude = field.text().await.ok().and_then(|v| v.parse().ok()); }
            "longitude" => { longitude = field.text().await.ok().and_then(|v| v.parse().ok()); }
            _ => {
                while field.chunk().await.map_err(|_| AppError::bad_request("Read error"))?.is_some() {}
            }
        }
    }

    let video_data = video_bytes.ok_or_else(|| AppError::bad_request("Missing 'video' field"))?;
    tracing::info!("Reel upload received: {:.1}MB from user {}", video_data.len() as f64 / 1_048_576.0, user_id);
    let mime = video_mime.unwrap_or_else(|| "video/mp4".to_string());
    let ext = if mime.contains("quicktime") || mime.contains("mov") {
        "mov"
    } else if mime.contains("m4v") {
        "m4v"
    } else {
        "mp4"
    };

    // Save video to disk
    let upload_dir = &state.config.upload_dir;
    fs::create_dir_all(format!("{}/reels", upload_dir))
        .await
        .map_err(|_| AppError::internal("Failed to create reels directory"))?;

    let filename = format!("reels/{}_{}_{}.{}", user_id, Utc::now().timestamp(), Uuid::new_v4(), ext);
    let disk_path = format!("{}/{}", upload_dir, filename);
    fs::write(&disk_path, &video_data)
        .await
        .map_err(|_| AppError::internal("Failed to save video"))?;

    // Free the in-memory video buffer immediately after writing to disk
    drop(video_data);

    let video_url = format!("/uploads/{}", filename);

    let tags_json = tags_str
        .as_deref()
        .and_then(|s| serde_json::from_str::<Value>(s).ok());

    let reel_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO reels (user_id, video_url, caption, category, tags,
                           music_id, music_title, music_artist, music_artwork_url,
                           music_preview_url, music_duration_ms, music_start_ms, music_genre,
                           location_tag, latitude, longitude, creator_city,
                           created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14, $15, $16, $17, NOW())
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(&video_url)
    .bind(&caption)
    .bind(&category)
    .bind(&tags_json)
    .bind(&music_id)
    .bind(&music_title)
    .bind(&music_artist)
    .bind(&music_artwork_url)
    .bind(&music_preview_url)
    .bind(&music_duration_ms)
    .bind(&music_start_ms)
    .bind(&music_genre)
    .bind(&location)
    .bind(&latitude)
    .bind(&longitude)
    .bind(&location) // creator_city = location name
    .fetch_one(&state.db)
    .await?;

    // Graph: user created reel
    {
        let db = state.db.clone();
        let uid = user_id.to_string();
        let rid = reel_id.to_string();
        tokio::spawn(async move {
            let _ = sqlx::query(
                "INSERT INTO graph_nodes (node_type, node_id, properties) VALUES ('user', $1, '{}') ON CONFLICT DO NOTHING"
            ).bind(&uid).execute(&db).await;
            let _ = sqlx::query(
                "INSERT INTO graph_nodes (node_type, node_id, properties) VALUES ('reel', $1, '{}') ON CONFLICT DO NOTHING"
            ).bind(&rid).execute(&db).await;
            let _ = sqlx::query(
                "INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id) VALUES ('user', $1, 'created', 'reel', $2) ON CONFLICT DO NOTHING"
            ).bind(&uid).bind(&rid).execute(&db).await;
            let _ = sqlx::query(
                "INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id) VALUES ('reel', $2, 'created', 'user', $1) ON CONFLICT DO NOTHING"
            ).bind(&uid).bind(&rid).execute(&db).await;
        });
    }

    // Log location content with weather/time context (iOS sends weather data in multipart)
    if latitude.is_some() || location.is_some() {
        let db2 = state.db.clone();
        let loc = location.clone();
        let lat2 = latitude;
        let lng2 = longitude;
        let now = chrono::Utc::now();
        tokio::spawn(async move {
            let _ = sqlx::query(
                r#"INSERT INTO location_content_log (user_id, content_type, content_id, latitude, longitude,
                      location_name, hour_of_day, day_of_week, month, season, posted_at)
                   VALUES ($1, 'reel', $2, $3, $4, $5, $6, $7, $8, $9, NOW())"#,
            )
            .bind(user_id).bind(reel_id).bind(lat2).bind(lng2).bind(&loc)
            .bind(now.format("%H").to_string().parse::<i32>().unwrap_or(0))
            .bind(now.format("%u").to_string().parse::<i32>().unwrap_or(1))
            .bind(now.format("%m").to_string().parse::<i32>().unwrap_or(1))
            .bind(match now.format("%m").to_string().parse::<i32>().unwrap_or(1) {
                1..=2 | 12 => "winter", 3..=5 => "spring", 6..=9 => "monsoon", _ => "autumn"
            })
            .execute(&db2).await;
        });
    }

    // Spawn HLS transcoding in the background — doesn't block the upload response
    // Uses single-pass pipeline: probe → skip normalize if short H.264 → direct HLS output
    {
        let db_clone = state.db.clone();
        let upload_dir_clone = upload_dir.clone();
        let disk_path_clone = disk_path.clone();
        tokio::spawn(async move {
            let _ = sqlx::query("UPDATE reels SET hls_state = 'processing' WHERE id = $1")
                .bind(reel_id)
                .execute(&db_clone)
                .await;

            let start = std::time::Instant::now();

            // Probe video to decide pipeline
            let probe = crate::hls::probe_video(&disk_path_clone).await;
            let needs_normalize = match &probe {
                Ok(p) => {
                    // Skip separate normalize if ≤30s and already H.264 — single-pass handles it
                    let dominated = p.duration_secs <= 31.0 && p.codec == "h264";
                    tracing::info!(
                        "Reel {} probe: {:.1}s, codec={}, {}px wide, skip_normalize={}",
                        reel_id, p.duration_secs, p.codec, p.width, dominated
                    );
                    !dominated
                }
                Err(e) => {
                    tracing::warn!("ffprobe failed for reel {}: {} — will normalize", reel_id, e);
                    true
                }
            };

            if needs_normalize {
                // Fallback two-step: normalize first, then HLS
                if let Err(e) = crate::hls::normalize_video(&disk_path_clone).await {
                    tracing::warn!("Normalization failed for reel {} (proceeding with original): {}", reel_id, e);
                }
                match crate::hls::transcode_to_hls(reel_id, &disk_path_clone, &upload_dir_clone, "reels").await {
                    Ok(hls_url) => {
                        let _ = sqlx::query(
                            "UPDATE reels SET hls_url = $1, hls_state = 'ready' WHERE id = $2",
                        )
                        .bind(&hls_url)
                        .bind(reel_id)
                        .execute(&db_clone)
                        .await;
                        tracing::info!("HLS ready for reel {} in {:.1}s (two-pass): {}", reel_id, start.elapsed().as_secs_f64(), hls_url);
                    }
                    Err(e) => {
                        let _ = sqlx::query("UPDATE reels SET hls_state = 'failed' WHERE id = $1")
                            .bind(reel_id)
                            .execute(&db_clone)
                            .await;
                        tracing::warn!("HLS failed for reel {}: {}", reel_id, e);
                    }
                }
            } else {
                // Fast path: single-pass normalize + HLS (no double encode)
                match crate::hls::normalize_and_hls(reel_id, &disk_path_clone, &upload_dir_clone, "reels").await {
                    Ok(hls_url) => {
                        let _ = sqlx::query(
                            "UPDATE reels SET hls_url = $1, hls_state = 'ready' WHERE id = $2",
                        )
                        .bind(&hls_url)
                        .bind(reel_id)
                        .execute(&db_clone)
                        .await;
                        tracing::info!("HLS ready for reel {} in {:.1}s (single-pass): {}", reel_id, start.elapsed().as_secs_f64(), hls_url);
                    }
                    Err(e) => {
                        let _ = sqlx::query("UPDATE reels SET hls_state = 'failed' WHERE id = $1")
                            .bind(reel_id)
                            .execute(&db_clone)
                            .await;
                        tracing::warn!("HLS failed for reel {}: {}", reel_id, e);
                    }
                }
            }
        });
    }

    Ok(Json(json!({
        "reel_id": reel_id,
        "hls_state": "processing",
        "message": "Reel created successfully"
    })))
}

/// GET /reels/trending-music — songs most used in reels (for music picker)
pub async fn get_trending_music(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _user_id = decode_access_token(&token, &state.config.secret_key)?;

    let limit: i64 = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(20).min(50);

    let tracks = sqlx::query_as::<_, (String, Option<String>, Option<String>, Option<String>, Option<String>, i64)>(
        r#"
        SELECT music_id, music_title, music_artist, music_artwork_url, music_preview_url,
               COUNT(*) as use_count
        FROM reels
        WHERE music_id IS NOT NULL AND is_active = TRUE
        GROUP BY music_id, music_title, music_artist, music_artwork_url, music_preview_url
        ORDER BY use_count DESC, MAX(created_at) DESC
        LIMIT $1
        "#,
    )
    .bind(limit)
    .fetch_all(state.read_pool())
    .await?;

    let items: Vec<Value> = tracks.iter().map(|(id, title, artist, artwork, preview, count)| {
        json!({
            "id": id,
            "title": title,
            "artist": artist,
            "artwork_url": artwork,
            "preview_url": preview,
            "use_count": count
        })
    }).collect();

    Ok(Json(json!({ "trending": items })))
}

/// Get personalized reel feed
pub async fn get_reel_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Feed controls
    // limit: 5-50, default 10 (initial), 15 (prefetch)
    // prefetch: true when loading next batch while user is still scrolling
    // session_id: carry across prefetches to group analytics
    let is_prefetch = params.get("prefetch").map(|v| v == "true" || v == "1").unwrap_or(false);
    let default_limit = if is_prefetch { 15 } else { 10 };
    let limit: i32 = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(default_limit).clamp(5, 50);
    let session_id = params.get("session_id").cloned().unwrap_or_else(|| Uuid::new_v4().to_string());

    let read_db = state.read_pool();

    // ── Step 1: Load user preference signals ──
    let category_prefs: std::collections::HashMap<String, f64> = sqlx::query_as::<_, (Option<Value>,)>(
        "SELECT preferred_categories FROM user_content_preferences WHERE user_id = $1"
    )
    .bind(user_id)
    .fetch_optional(read_db)
    .await?
    .and_then(|(v,)| v)
    .and_then(|v| v.as_object().map(|obj| {
        obj.iter().filter_map(|(k, v)| v.as_f64().map(|score| (k.clone(), score))).collect()
    }))
    .unwrap_or_default();

    let exploration_rate: f64 = sqlx::query_scalar::<_, f64>(
        "SELECT COALESCE(reel_exploration_rate, 0.3) FROM user_interaction_model WHERE user_id = $1"
    )
    .bind(user_id as i64)
    .fetch_optional(read_db)
    .await?
    .unwrap_or(0.3); // New users get 30% exploration

    // ── Step 2a: Load viewer's city for location boosting ──
    let viewer_city: Option<String> = sqlx::query_scalar("SELECT city FROM user_locations WHERE user_id = $1")
        .bind(user_id)
        .fetch_optional(read_db)
        .await?
        .flatten();

    // ── Step 2b: Candidate generation — 5x limit, exclude seen reels ──
    let candidate_pool = (limit * 5).min(200);

    #[derive(sqlx::FromRow)]
    struct ReelCandidate {
        id: i64,
        user_id: i32,
        video_url: String,
        hls_url: Option<String>,
        hls_state: Option<String>,
        thumbnail_url: Option<String>,
        duration_sec: Option<i32>,
        caption: Option<String>,
        tags: Option<Value>,
        category: Option<String>,
        engagement_score: Option<f64>,
        avg_watch_percent: Option<f64>,
        view_count: Option<i32>,
        like_count: Option<i32>,
        created_at: Option<NaiveDateTime>,
        creator_name: Option<String>,
        creator_photo: Option<String>,
        creator_verified: Option<bool>,
        creator_attractiveness: Option<f64>,
        creator_city: Option<String>,
        music_id: Option<String>,
        music_title: Option<String>,
        music_artist: Option<String>,
        music_artwork_url: Option<String>,
        music_preview_url: Option<String>,
        music_start_ms: Option<i32>,
    }

    let candidates = sqlx::query_as::<_, ReelCandidate>(
        r#"
        SELECT r.id, r.user_id::int4 as user_id, r.video_url, r.hls_url, r.hls_state,
               r.thumbnail_url, r.duration_sec,
               r.caption, r.tags, r.category, r.engagement_score, r.avg_watch_percent,
               r.view_count, r.like_count, r.created_at,
               u.name as creator_name, u.profile_photo_1 as creator_photo,
               u.is_verified as creator_verified, u.attractiveness_score as creator_attractiveness,
               COALESCE(r.creator_city, ul.city) as creator_city,
               r.music_id, r.music_title, r.music_artist, r.music_artwork_url,
               r.music_preview_url, r.music_start_ms
        FROM reels r
        JOIN users u ON u.id = r.user_id
        LEFT JOIN user_locations ul ON ul.user_id = r.user_id
        WHERE r.is_active = TRUE
          AND r.user_id != $1
          AND NOT EXISTS (
              SELECT 1 FROM reel_views rv
              WHERE rv.reel_id = r.id AND rv.viewer_id = $1
          )
          AND NOT EXISTS (
              SELECT 1 FROM matches m
              WHERE ((m.user1_id = $1 AND m.user2_id = r.user_id) OR (m.user2_id = $1 AND m.user1_id = r.user_id))
              AND m.status = 'blocked'
          )
        ORDER BY r.created_at DESC
        LIMIT $2
        "#,
    )
    .bind(user_id as i64)
    .bind(candidate_pool)
    .fetch_all(read_db)
    .await?;

    // ── Step 3: Personalized scoring ──
    let now = chrono::Utc::now().naive_utc();
    let mut rng_seed = (user_id as u64).wrapping_mul(now.and_utc().timestamp() as u64);

    // ── Graph signals: which creators did users-like-me also watch? ──
    let uid_str = (user_id as i64).to_string();
    let creator_id_strs: Vec<String> = candidates.iter().map(|r| (r.user_id as i64).to_string()).collect();
    let graph_creator_scores: std::collections::HashMap<i32, f64> = if !creator_id_strs.is_empty() {
        sqlx::query_as::<_, (String, i64)>(
            r#"SELECT f2.from_id, COUNT(*) as shared
               FROM graph_edge_links_fwd f1
               JOIN graph_edge_links_fwd f2 ON f1.to_id = f2.to_id AND f1.edge_type = f2.edge_type
               WHERE f1.from_type = 'user' AND f1.from_id = $1
                 AND f1.edge_type = 'liked' AND f2.from_type = 'user'
                 AND f2.from_id = ANY($2) AND f2.from_id != $1
               GROUP BY f2.from_id"#,
        )
        .bind(&uid_str).bind(&creator_id_strs)
        .fetch_all(read_db).await.unwrap_or_default()
        .into_iter()
        .filter_map(|(id, count)| id.parse::<i32>().ok().map(|i| (i, (count as f64 / 5.0).min(1.0))))
        .collect()
    } else {
        std::collections::HashMap::new()
    };

    // ── Music signals: does reel creator share music taste? ──
    let creator_i64s: Vec<i64> = candidates.iter().map(|r| r.user_id as i64).collect();
    let music_creator_scores: std::collections::HashMap<i32, f64> = if !creator_i64s.is_empty() {
        sqlx::query_as::<_, (i64, i64)>(
            r#"SELECT b.user_id, COUNT(*) as shared
               FROM user_genre_profile a
               JOIN user_genre_profile b ON a.genre = b.genre
               WHERE a.user_id = $1 AND b.user_id = ANY($2)
               GROUP BY b.user_id"#,
        )
        .bind(user_id as i64).bind(&creator_i64s)
        .fetch_all(read_db).await.unwrap_or_default()
        .into_iter()
        .map(|(cid, shared)| (cid as i32, (shared as f64 / 5.0).min(1.0)))
        .collect()
    } else {
        std::collections::HashMap::new()
    };

    let mut scored: Vec<(f64, usize)> = candidates.iter().enumerate().map(|(idx, r)| {
        // ── Category affinity (25%) ──
        let cat_score = r.category.as_ref()
            .and_then(|c| category_prefs.get(c))
            .copied()
            .unwrap_or(0.0)
            .min(1.0);

        // ── Engagement quality (15%) — normalized watch% + like rate ──
        let watch_quality = r.avg_watch_percent.unwrap_or(0.0) / 100.0;
        let like_rate = if r.view_count.unwrap_or(0) > 0 {
            (r.like_count.unwrap_or(0) as f64) / (r.view_count.unwrap_or(1) as f64)
        } else {
            0.0
        };
        let engagement = (watch_quality * 0.6 + like_rate.min(1.0) * 0.4).min(1.0);

        // ── Freshness (10%) — exponential decay, half-life = 24h ──
        let age_hours = r.created_at
            .map(|t| (now - t).num_hours() as f64)
            .unwrap_or(168.0);
        let freshness = (-0.693 * age_hours / 24.0).exp();

        // ── Graph: creator liked by users-like-me (15%) ──
        let graph_score = graph_creator_scores.get(&r.user_id).copied().unwrap_or(0.0);

        // ── Music taste match with creator (10%) ──
        let music_score = music_creator_scores.get(&r.user_id).copied().unwrap_or(0.0);

        // ── Creator compatibility (5%) ──
        let attractiveness = r.creator_attractiveness.unwrap_or(0.0).min(1.0);

        // ── Location boost (10%) — same city = strong boost ──
        let location_score = match (&viewer_city, &r.creator_city) {
            (Some(vc), Some(cc)) if !vc.is_empty() && vc.to_lowercase() == cc.to_lowercase() => 1.0,
            _ => 0.0,
        };

        // ── Exploration (5%) — pseudo-random for diversity ──
        rng_seed = rng_seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        let random_val = (rng_seed >> 33) as f64 / (u32::MAX as f64);
        let explore = if random_val < exploration_rate { 0.5 + random_val } else { 0.0 };

        // ── Weighted combination ──
        let score = cat_score * 0.25
            + engagement * 0.15
            + freshness * 0.10
            + graph_score * 0.15
            + music_score * 0.10
            + attractiveness * 0.05
            + location_score * 0.10
            + explore * 0.05
            + random_val * 0.05;

        (score, idx)
    }).collect();

    // Sort by personalized score descending
    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

    // ── Step 4: Diversity filter — max 2 reels per creator in final feed ──
    let mut creator_counts: std::collections::HashMap<i32, usize> = std::collections::HashMap::new();
    let mut category_streak = 0u32;
    let mut last_category: Option<String> = None;
    let mut final_indices: Vec<usize> = Vec::with_capacity(limit as usize);

    for (_, idx) in &scored {
        if final_indices.len() >= limit as usize { break; }
        let r = &candidates[*idx];

        // Max 2 reels per creator
        let count = creator_counts.entry(r.user_id).or_insert(0);
        if *count >= 2 { continue; }

        // Max 3 consecutive same-category reels
        if let Some(ref cat) = r.category {
            if last_category.as_ref() == Some(cat) {
                category_streak += 1;
                if category_streak >= 3 { continue; }
            } else {
                category_streak = 0;
                last_category = Some(cat.clone());
            }
        }

        *count += 1;
        final_indices.push(*idx);
    }

    // ── Step 5: Build response with university + interaction data ──
    let final_candidates: Vec<&ReelCandidate> = final_indices.iter().map(|i| &candidates[*i]).collect();
    let creator_ids: Vec<i32> = final_candidates.iter().map(|r| r.user_id).collect();
    let uni_map = batch_lookup_university(read_db, &creator_ids).await?;
    let interaction_map = batch_lookup_interactions(read_db, user_id, &creator_ids).await?;

    let reels: Vec<Value> = final_indices.iter().zip(scored.iter()).map(|(idx, (score, _))| {
        let r = &candidates[*idx];
        let uni_info = uni_map.get(&r.user_id);
        let interaction = interaction_map.get(&r.user_id).cloned().unwrap_or_else(|| "none".to_string());
        json!({
            "id": r.id,
            "user_id": r.user_id,
            "video_url": r.video_url,
            "hls_url": r.hls_url,
            "hls_state": r.hls_state,
            "thumbnail_url": r.thumbnail_url,
            "duration_sec": r.duration_sec,
            "caption": r.caption,
            "tags": r.tags,
            "category": r.category,
            "engagement_score": r.engagement_score,
            "view_count": r.view_count,
            "like_count": r.like_count,
            "created_at": r.created_at,
            "creator_name": r.creator_name,
            "creator_photo": r.creator_photo,
            "creator_verified": r.creator_verified,
            "creator_university": uni_info.map(|(name, _)| name.clone()),
            "creator_university_tier": uni_info.map(|(_, tier)| format_tier(tier)),
            "interaction_status": interaction,
            "can_like": interaction == "none",
            "personalization_score": (score * 100.0).round() / 100.0,
            "music": if r.music_id.is_some() { Some(json!({
                "id": r.music_id,
                "title": r.music_title,
                "artist": r.music_artist,
                "artwork_url": r.music_artwork_url,
                "preview_url": r.music_preview_url,
                "start_ms": r.music_start_ms
            })) } else { None }
        })
    }).collect();

    // ── Step 6: Decay exploration rate as user watches more ──
    // (async fire-and-forget — don't block the response)
    let db_clone = state.db.clone();
    let uid = user_id;
    tokio::spawn(async move {
        let _ = sqlx::query(
            r#"UPDATE user_interaction_model
               SET reel_exploration_rate = GREATEST(0.05, reel_exploration_rate * 0.995),
                   total_reel_interactions = total_reel_interactions + 1,
                   updated_at = NOW()
               WHERE user_id = $1"#
        ).bind(uid as i64).execute(&db_clone).await;
    });

    // has_more = we had more candidates than we returned
    let has_more = candidates.len() > final_indices.len();

    Ok(Json(json!({
        "reels": reels,
        "session_id": session_id,
        "count": reels.len(),
        "has_more": has_more,
        "prefetch_at": (reels.len() as f64 * 0.7).ceil() as usize,
        "algorithm": "personalized_v1",
        "exploration_rate": (exploration_rate * 100.0).round() / 100.0
    })))
}

/// Track reel view - ML learns interest patterns
#[derive(Deserialize)]
pub struct TrackReelViewPayload {
    pub reel_id: i32,
    pub watch_duration_sec: i32,
    /// Precise watch duration in milliseconds (preferred over watch_duration_sec)
    pub watch_duration_ms: Option<i64>,
    pub watch_percent: f64,
    pub rewatched: Option<bool>,
    pub source: Option<String>,
    pub session_id: Option<String>,
    pub scroll_velocity: Option<f64>,
    pub position_in_feed: Option<i32>,
    /// Number of times user skipped forward (negative interest signal)
    pub seek_forward_count: Option<i32>,
    /// Number of times user rewound (positive — wanted to re-see something)
    pub seek_backward_count: Option<i32>,
    /// Number of times user paused (positive — stopped to look/read)
    pub pause_count: Option<i32>,
}

pub async fn track_reel_view(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<TrackReelViewPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let session_id = payload.session_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());

    // Fetch reel owner + duration + creator city in one query
    #[derive(sqlx::FromRow)]
    struct ReelMeta { user_id: i64, duration_sec: Option<i32>, creator_city: Option<String> }
    let meta = sqlx::query_as::<_, ReelMeta>(
        "SELECT r.user_id, r.duration_sec, ul.city as creator_city FROM reels r JOIN users u ON u.id = r.user_id LEFT JOIN user_locations ul ON ul.user_id = r.user_id WHERE r.id = $1"
    )
    .bind(payload.reel_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("Reel not found"))?;
    let reel_owner = meta.user_id as i32;

    // Viewer city for same-city signal
    let viewer_city: Option<String> = sqlx::query_scalar("SELECT city FROM users WHERE id = $1")
        .bind(user_id).fetch_optional(&state.db).await?.flatten();
    let same_city = match (&viewer_city, &meta.creator_city) {
        (Some(vc), Some(cc)) if !vc.is_empty() => vc.to_lowercase() == cc.to_lowercase(),
        _ => false,
    };

    let seek_fwd = payload.seek_forward_count.unwrap_or(0);
    let seek_bwd = payload.seek_backward_count.unwrap_or(0);
    let pauses   = payload.pause_count.unwrap_or(0);
    let rewatched = payload.rewatched.unwrap_or(false);

    // ── Precise reward formula v2 ──────────────────────────────────────────
    // d_norm: fraction of reel actually watched (capped 0–1)
    let d_norm = if let Some(dur) = meta.duration_sec.filter(|&d| d > 0) {
        (payload.watch_duration_sec as f64 / dur as f64).min(1.0)
    } else {
        (payload.watch_percent / 100.0).min(1.0)
    };

    let mut reward: f64 = 0.0;
    reward += 0.35 * (payload.watch_percent / 100.0).min(1.0); // 0–0.35
    reward += 0.15 * d_norm;                                    // 0–0.15
    if rewatched { reward += 0.15; }                            // +0.15
    reward += (seek_bwd as f64 * 0.05).min(0.15);              // rewind: 0–+0.15
    reward -= (seek_fwd as f64 * 0.04).min(0.15);              // skip:   0–-0.15
    reward += (pauses as f64 * 0.03).min(0.10);                // pause:  0–+0.10

    // Same-city multiplier (config-tunable: 1.0–1.20, default 1.10)
    if same_city {
        reward *= state.config.reel_same_city_multiplier;
    }

    // Hard cap: max 4.0, floor -0.5 (like/message bonuses stack on top at call site)
    reward = reward.clamp(-0.5, 4.0);

    // Shadow cohort: deterministic by user_id (same user always in same bucket).
    // user_id % 100 < fraction*100 → v2; everyone else → v1 (control).
    let cohort_threshold = (state.config.reel_shadow_cohort_fraction * 100.0).round() as i32;
    let reward_version = if (user_id % 100) < cohort_threshold {
        state.config.reel_reward_version.as_str() // "v2"
    } else {
        "v1" // control — reward formula identical, tag differs for Prometheus split
    };

    let watch_duration_ms = payload.watch_duration_ms
        .unwrap_or_else(|| payload.watch_duration_sec as i64 * 1000);

    // Interest score for preference model
    let interest_score = calc_interest_score(
        payload.watch_percent, payload.watch_duration_sec,
        rewatched, payload.scroll_velocity,
        seek_fwd, seek_bwd, pauses,
    );

    sqlx::query(
        r#"
        INSERT INTO reel_views (reel_id, viewer_id, watch_duration_sec, watch_percent, rewatched,
                                seek_forward_count, seek_backward_count, pause_count,
                                source, session_id, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, NOW())
        ON CONFLICT (reel_id, viewer_id, session_id) DO UPDATE SET
            watch_duration_sec  = GREATEST(reel_views.watch_duration_sec, $3),
            watch_percent       = GREATEST(reel_views.watch_percent, $4),
            rewatched           = $5 OR reel_views.rewatched,
            rewatch_count       = CASE WHEN $5 THEN reel_views.rewatch_count + 1 ELSE reel_views.rewatch_count END,
            seek_forward_count  = reel_views.seek_forward_count  + $6,
            seek_backward_count = reel_views.seek_backward_count + $7,
            pause_count         = reel_views.pause_count         + $8
        "#,
    )
    .bind(payload.reel_id).bind(user_id).bind(payload.watch_duration_sec).bind(payload.watch_percent)
    .bind(rewatched).bind(seek_fwd).bind(seek_bwd).bind(pauses)
    .bind(&payload.source).bind(&session_id)
    .execute(&state.db).await?;

    // Update reel aggregate stats
    sqlx::query("UPDATE reels SET view_count = view_count + 1, avg_watch_percent = (avg_watch_percent * view_count + $2) / (view_count + 1), updated_at = NOW() WHERE id = $1")
        .bind(payload.reel_id).bind(payload.watch_percent).execute(&state.db).await?;

    log_reel_event(&state.db, user_id, payload.reel_id, reel_owner, "view",
        payload.watch_percent, watch_duration_ms, payload.scroll_velocity,
        payload.source.as_deref(), payload.position_in_feed,
        seek_fwd, seek_bwd, pauses,
        same_city, false, false, 0,
        reward, reward_version).await?;

    if payload.watch_percent > 50.0 {
        update_content_prefs(&state.db, user_id, payload.reel_id, interest_score).await?;
    }

    // Graph: user viewed reel + log location interaction
    {
        let db = state.db.clone();
        let uid = user_id.to_string();
        let rid = payload.reel_id.to_string();
        let reel_owner_id = meta.user_id;
        let reel_id = payload.reel_id;
        tokio::spawn(async move {
            let _ = sqlx::query(
                "INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id) VALUES ('user', $1, 'viewed', 'reel', $2) ON CONFLICT DO NOTHING"
            ).bind(&uid).bind(&rid).execute(&db).await;
            let _ = sqlx::query(
                "INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id) VALUES ('reel', $2, 'viewed', 'user', $1) ON CONFLICT DO NOTHING"
            ).bind(&uid).bind(&rid).execute(&db).await;

            // Log interaction with reel creator at reel's location
            let _ = sqlx::query(
                r#"INSERT INTO location_interactions (user_id, target_user_id, interaction_type, content_type, content_id, latitude, longitude, location_name)
                   SELECT $1, $2, 'viewed', 'reel', $3, r.latitude, r.longitude, r.location_tag
                   FROM reels r WHERE r.id = $3 AND r.latitude IS NOT NULL"#,
            ).bind(user_id).bind(reel_owner_id).bind(reel_id as i64).execute(&db).await;
        });
    }

    Ok(Json(json!({
        "tracked": true,
        "interest_score": interest_score,
        "reward": reward,
        "reward_version": reward_version,
        "same_city": same_city
    })))
}

/// Like reel - strong interest signal
#[derive(Deserialize)]
pub struct ReelIdPayload {
    pub reel_id: i32,
}

pub async fn like_reel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReelIdPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let reel_owner = sqlx::query_scalar::<_, i64>("SELECT user_id FROM reels WHERE id = $1")
        .bind(payload.reel_id).fetch_optional(&state.db).await?.ok_or_else(|| AppError::not_found("Reel not found"))? as i32;

    if reel_owner == user_id { return Err(AppError::bad_request("Cannot like your own reel")); }

    let result = sqlx::query("INSERT INTO reel_likes (reel_id, user_id, created_at) VALUES ($1, $2, NOW()) ON CONFLICT DO NOTHING")
        .bind(payload.reel_id).bind(user_id).execute(&state.db).await?;

    if result.rows_affected() > 0 {
        sqlx::query("UPDATE reels SET like_count = like_count + 1, updated_at = NOW() WHERE id = $1").bind(payload.reel_id).execute(&state.db).await?;
        log_reel_event(&state.db, user_id, payload.reel_id, reel_owner, "like", 100.0, 0, None, None, None, 0, 0, 0, false, true, false, 0, 2.0, "v2").await?;
        update_content_prefs(&state.db, user_id, payload.reel_id, 1.0).await?;
    }

    Ok(Json(json!({ "liked": true })))
}

/// Unlike reel
pub async fn unlike_reel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReelIdPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let result = sqlx::query("DELETE FROM reel_likes WHERE reel_id = $1 AND user_id = $2").bind(payload.reel_id).bind(user_id).execute(&state.db).await?;
    if result.rows_affected() > 0 {
        sqlx::query("UPDATE reels SET like_count = GREATEST(0, like_count - 1) WHERE id = $1").bind(payload.reel_id).execute(&state.db).await?;
    }

    Ok(Json(json!({ "unliked": true })))
}

/// Send PRIVATE message on reel (only reel owner sees it) - highest effort signal
#[derive(Deserialize)]
pub struct SendReelMessagePayload {
    pub reel_id: i32,
    pub content: String,
    pub message_type: Option<String>,
    pub reaction_emoji: Option<String>,
}

pub async fn send_reel_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SendReelMessagePayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let sender_id = decode_access_token(&token, &state.config.secret_key)?;

    let receiver_id_i64 = sqlx::query_scalar::<_, i64>("SELECT user_id FROM reels WHERE id = $1")
        .bind(payload.reel_id).fetch_optional(&state.db).await?.ok_or_else(|| AppError::not_found("Reel not found"))?;
    let receiver_id = receiver_id_i64 as i32;

    if receiver_id == sender_id { return Err(AppError::bad_request("Cannot message yourself")); }

    // Calculate effort: length, has question, thoughtfulness
    let effort_score = calc_message_effort(&payload.content, payload.reaction_emoji.is_some());
    let msg_type = payload.message_type.as_deref().unwrap_or("text");

    let message_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO reel_messages (reel_id, sender_id, receiver_id, content, message_type, reaction_emoji, created_at) VALUES ($1, $2, $3, $4, $5, $6, NOW()) RETURNING id",
    )
    .bind(payload.reel_id).bind(sender_id).bind(receiver_id).bind(&payload.content).bind(msg_type).bind(&payload.reaction_emoji)
    .fetch_one(&state.db).await?;

    // Auto-queue reel message for LLM labeling
    auto_queue_for_labeling(state.db.clone(), state.config.llm_enabled, "reel_message", message_id, 4);

    sqlx::query("UPDATE reels SET message_count = message_count + 1, updated_at = NOW() WHERE id = $1").bind(payload.reel_id).execute(&state.db).await?;

    // Update conversation thread
    let (user_a, user_b) = if sender_id < receiver_id { (sender_id, receiver_id) } else { (receiver_id, sender_id) };
    let is_sender_a = sender_id == user_a;

    sqlx::query(
        r#"
        INSERT INTO reel_conversations (reel_id, user_a, user_b, a_message_count, b_message_count, total_messages, a_initiated, last_message_by, last_message_at, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, 1, $6, $7, NOW(), NOW(), NOW())
        ON CONFLICT (reel_id, user_a, user_b) DO UPDATE SET
            a_message_count = reel_conversations.a_message_count + $4, b_message_count = reel_conversations.b_message_count + $5,
            total_messages = reel_conversations.total_messages + 1, last_message_by = $7, last_message_at = NOW(), updated_at = NOW()
        "#,
    )
    .bind(payload.reel_id).bind(user_a).bind(user_b).bind(if is_sender_a { 1 } else { 0 }).bind(if is_sender_a { 0 } else { 1 }).bind(is_sender_a).bind(sender_id)
    .execute(&state.db).await?;

    // Log for ML - messages are highest value
    // Effort bonus: +0.1 per 20 chars, capped at +0.5
    let effort_bonus = ((payload.content.len() as f64 / 20.0) * 0.1).min(0.5);
    let msg_reward = (3.0 + effort_bonus).min(4.0);
    log_reel_event(&state.db, sender_id, payload.reel_id, receiver_id, "message", 100.0, 0, None, None, None, 0, 0, 0, false, false, true, payload.content.len() as i32, msg_reward, "v2").await?;
    update_content_prefs(&state.db, sender_id, payload.reel_id, effort_score).await?;

    // Record for response tracking
    let msg_features = serde_json::json!({ "length": payload.content.len(), "has_question": payload.content.contains('?'), "effort": effort_score });
    sqlx::query("INSERT INTO response_training_data (sender_id, receiver_id, interaction_source, reel_id, message_features, got_response, created_at) VALUES ($1, $2, 'reel_message', $3, $4, FALSE, NOW())")
        .bind(sender_id).bind(receiver_id).bind(payload.reel_id).bind(&msg_features).execute(&state.db).await?;

    // Publish notification event
    let preview = if payload.content.len() > 60 { format!("{}...", &payload.content[..57]) } else { payload.content.clone() };
    state.event_bus.publish("reel_handler", crate::modules::events::DomainEvent::ReelMessage {
        reel_id: payload.reel_id, sender_id, receiver_id, content_preview: preview,
    });

    // Real-time inbox badge update for the receiver (if their /ws/events is connected).
    publish_reel_inbox_update(&state, receiver_id).await;

    Ok(Json(json!({ "message_id": message_id, "effort_score": effort_score, "sent": true })))
}

/// Get inbox - messages received on user's reels
pub async fn get_reel_inbox(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let limit: i32 = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50);
    let unread_only = params.get("unread_only").map(|v| v == "true").unwrap_or(false);
    let since = params.get("since").and_then(|s|
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok()
            .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok()));

    #[derive(sqlx::FromRow, Serialize)]
    struct InboxMsg {
        id: i64, reel_id: i32, sender_id: i32, content: String, message_type: Option<String>,
        reaction_emoji: Option<String>, is_read: Option<bool>, created_at: Option<NaiveDateTime>,
        sender_name: Option<String>, sender_photo: Option<String>, reel_thumbnail: Option<String>,
    }

    let read_db = state.read_pool();

    let messages = if let Some(since_ts) = since {
        // Delta sync: only messages after the given timestamp
        let query = if unread_only {
            r#"SELECT rm.id, rm.reel_id, rm.sender_id, rm.content, rm.message_type, rm.reaction_emoji, rm.is_read, rm.created_at,
               u.name as sender_name, u.profile_photo_1 as sender_photo, r.thumbnail_url as reel_thumbnail
               FROM reel_messages rm JOIN users u ON u.id = rm.sender_id JOIN reels r ON r.id = rm.reel_id
               WHERE rm.receiver_id = $1 AND rm.is_read = FALSE AND rm.created_at > $3 ORDER BY rm.created_at DESC LIMIT $2"#
        } else {
            r#"SELECT rm.id, rm.reel_id, rm.sender_id, rm.content, rm.message_type, rm.reaction_emoji, rm.is_read, rm.created_at,
               u.name as sender_name, u.profile_photo_1 as sender_photo, r.thumbnail_url as reel_thumbnail
               FROM reel_messages rm JOIN users u ON u.id = rm.sender_id JOIN reels r ON r.id = rm.reel_id
               WHERE rm.receiver_id = $1 AND rm.created_at > $3 ORDER BY rm.created_at DESC LIMIT $2"#
        };
        sqlx::query_as::<_, InboxMsg>(query).bind(user_id).bind(limit).bind(since_ts).fetch_all(read_db).await?
    } else {
        let query = if unread_only {
            r#"SELECT rm.id, rm.reel_id, rm.sender_id, rm.content, rm.message_type, rm.reaction_emoji, rm.is_read, rm.created_at,
               u.name as sender_name, u.profile_photo_1 as sender_photo, r.thumbnail_url as reel_thumbnail
               FROM reel_messages rm JOIN users u ON u.id = rm.sender_id JOIN reels r ON r.id = rm.reel_id
               WHERE rm.receiver_id = $1 AND rm.is_read = FALSE ORDER BY rm.created_at DESC LIMIT $2"#
        } else {
            r#"SELECT rm.id, rm.reel_id, rm.sender_id, rm.content, rm.message_type, rm.reaction_emoji, rm.is_read, rm.created_at,
               u.name as sender_name, u.profile_photo_1 as sender_photo, r.thumbnail_url as reel_thumbnail
               FROM reel_messages rm JOIN users u ON u.id = rm.sender_id JOIN reels r ON r.id = rm.reel_id
               WHERE rm.receiver_id = $1 ORDER BY rm.created_at DESC LIMIT $2"#
        };
        sqlx::query_as::<_, InboxMsg>(query).bind(user_id).bind(limit).fetch_all(read_db).await?
    };

    let unread_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM reel_messages WHERE receiver_id = $1 AND is_read = FALSE").bind(user_id).fetch_one(read_db).await.unwrap_or(0);

    Ok(Json(json!({ "messages": messages, "unread_count": unread_count })))
}

/// Reply to reel message - ML LEARNS what messages get responses!
#[derive(Deserialize)]
pub struct ReplyReelMessagePayload {
    pub original_message_id: i64,
    pub content: String,
}

pub async fn reply_reel_message(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReplyReelMessagePayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    #[derive(sqlx::FromRow)]
    struct OrigMsg { reel_id: i64, sender_id: i64, receiver_id: i64, created_at: Option<NaiveDateTime> }

    let orig = sqlx::query_as::<_, OrigMsg>("SELECT reel_id, sender_id, receiver_id, created_at FROM reel_messages WHERE id = $1")
        .bind(payload.original_message_id).fetch_optional(&state.db).await?.ok_or_else(|| AppError::not_found("Message not found"))?;

    if orig.receiver_id as i32 != user_id { return Err(AppError::forbidden("Not authorized")); }

    let response_time_sec = orig.created_at.map(|t| (Utc::now().naive_utc() - t).num_seconds() as i32);

    let orig_reel_id = orig.reel_id as i32;
    let orig_sender_id = orig.sender_id as i32;

    let reply_id = sqlx::query_scalar::<_, i64>("INSERT INTO reel_messages (reel_id, sender_id, receiver_id, content, message_type, created_at) VALUES ($1, $2, $3, $4, 'text', NOW()) RETURNING id")
        .bind(orig_reel_id).bind(user_id).bind(orig_sender_id).bind(&payload.content).fetch_one(&state.db).await?;

    // Auto-queue reply for LLM labeling
    auto_queue_for_labeling(state.db.clone(), state.config.llm_enabled, "reel_message", reply_id, 4);

    // Mark original as replied
    sqlx::query("UPDATE reel_messages SET replied = TRUE, reply_delay_sec = $2 WHERE id = $1").bind(payload.original_message_id).bind(response_time_sec).execute(&state.db).await?;

    // Update conversation
    let (user_a, user_b) = if user_id < orig_sender_id { (user_id, orig_sender_id) } else { (orig_sender_id, user_id) };
    let is_replier_a = user_id == user_a;

    let conv = sqlx::query_as::<_, (i32, i32)>("SELECT a_message_count, b_message_count FROM reel_conversations WHERE reel_id = $1 AND user_a = $2 AND user_b = $3")
        .bind(orig_reel_id).bind(user_a).bind(user_b).fetch_optional(&state.db).await?;
    let conversation_continued = conv.map(|(a, b)| a + b >= 2).unwrap_or(false);

    sqlx::query(r#"UPDATE reel_conversations SET a_message_count = a_message_count + $4, b_message_count = b_message_count + $5, total_messages = total_messages + 1, last_message_by = $6, last_message_at = NOW(), updated_at = NOW() WHERE reel_id = $1 AND user_a = $2 AND user_b = $3"#)
        .bind(orig_reel_id).bind(user_a).bind(user_b).bind(if is_replier_a { 1 } else { 0 }).bind(if is_replier_a { 0 } else { 1 }).bind(user_id).execute(&state.db).await?;

    // KEY ML LEARNING: Original sender got a response - their approach worked!
    let reward = 3.0 + if conversation_continued { 2.0 } else { 0.0 };
    sqlx::query(r#"UPDATE response_training_data SET got_response = TRUE, response_time_sec = $4, conversation_continued = $5, reward = $6 WHERE sender_id = $1 AND receiver_id = $2 AND reel_id = $3 AND got_response = FALSE"#)
        .bind(orig_sender_id).bind(user_id).bind(orig_reel_id).bind(response_time_sec).bind(conversation_continued).bind(reward).execute(&state.db).await?;

    // Update sender's response patterns
    sqlx::query(r#"INSERT INTO user_response_patterns (user_id, total_responses_received, conversations_continued, updated_at) VALUES ($1, 1, $2, NOW())
        ON CONFLICT (user_id) DO UPDATE SET total_responses_received = user_response_patterns.total_responses_received + 1, conversations_continued = user_response_patterns.conversations_continued + $2, response_rate = (user_response_patterns.total_responses_received + 1)::float / GREATEST(user_response_patterns.total_messages_sent, 1), updated_at = NOW()"#)
        .bind(orig_sender_id).bind(if conversation_continued { 1 } else { 0 }).execute(&state.db).await?;

    log_reel_event(&state.db, user_id, orig_reel_id, orig_sender_id, "reply", 100.0, 0, None, None, None, 0, 0, 0, false, false, true, 0, 4.0, "v2").await?;

    // Check match eligibility
    check_reel_match_eligibility(&state.db, orig_reel_id, user_a, user_b).await?;

    // Publish notification events
    let preview = if payload.content.len() > 60 { format!("{}...", &payload.content[..57]) } else { payload.content.clone() };
    state.event_bus.publish("reel_handler", crate::modules::events::DomainEvent::ReelReply {
        reel_id: orig_reel_id, replier_id: user_id, original_sender_id: orig_sender_id, content_preview: preview,
    });

    // Check if this reply triggered match eligibility — notify both users
    let eligible: Option<bool> = sqlx::query_scalar("SELECT eligible_for_match FROM reel_conversations WHERE reel_id = $1 AND user_a = $2 AND user_b = $3")
        .bind(orig_reel_id).bind(user_a).bind(user_b).fetch_optional(&state.db).await?.flatten();
    if eligible == Some(true) {
        state.event_bus.publish("reel_handler", crate::modules::events::DomainEvent::ReelMatchEligible {
            reel_id: orig_reel_id, user_a, user_b,
        });
    }

    // Real-time badge update for the original sender (the recipient of this reply).
    publish_reel_inbox_update(&state, orig_sender_id).await;

    Ok(Json(json!({ "reply_id": reply_id, "conversation_continued": conversation_continued, "response_time_sec": response_time_sec })))
}

/// Mark message as read
#[derive(Deserialize)]
pub struct MsgIdPayload { pub message_id: i64 }

pub async fn mark_reel_message_read(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<MsgIdPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    sqlx::query("UPDATE reel_messages SET is_read = TRUE, read_at = NOW() WHERE id = $1 AND receiver_id = $2").bind(payload.message_id).bind(user_id).execute(&state.db).await?;
    publish_reel_inbox_update(&state, user_id).await;
    Ok(Json(json!({ "marked_read": true })))
}

/// Publish a durable user event: writes to user_event_outbox AND broadcasts
/// to any live /ws/events subscribers. On reconnect, the client replays
/// outbox rows it hasn't seen (see websocket::handle_events).
///
/// The returned event_id is embedded in the JSON so clients can track
/// last-seen-id for since-based replay on the next connect.
///
/// Fire-and-forget — never fails the caller.
pub async fn publish_user_event(state: &AppState, user_id: i32, event_type: &str, mut payload: Value) {
    // INSERT first so the event is durable even if broadcast drops it.
    let event_id = sqlx::query_scalar::<_, i64>(
        "INSERT INTO user_event_outbox (user_id, event_type, payload) VALUES ($1, $2, $3) RETURNING id"
    )
    .bind(user_id)
    .bind(event_type)
    .bind(&payload)
    .fetch_one(&state.db)
    .await
    .ok();

    // Embed event_id + type into the outgoing JSON envelope so the client
    // can drive since-id replay and type-discriminated handling.
    if let Some(id) = event_id {
        if let Some(obj) = payload.as_object_mut() {
            obj.insert("event_id".to_string(), json!(id));
            obj.entry("type").or_insert_with(|| json!(event_type));
        }
    }

    let msg = payload.to_string();
    // Fan out to other pods first (borrows msg), then deliver to this pod's
    // local subscribers (consumes msg). A user's devices may be connected to
    // different instances, so both paths are needed.
    crate::realtime::publish_user_event(&state, user_id, &msg).await;
    state.user_events.read().await.publish(user_id, msg);
}

/// Recompute unread reel message count for a user and publish it as a
/// durable event. Still self-healing: clients can fall back to the fresh
/// state we send on /ws/events connect if they miss everything.
pub async fn publish_reel_inbox_update(state: &AppState, user_id: i32) {
    let unread: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM reel_messages WHERE receiver_id = $1 AND is_read = FALSE"
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    publish_user_event(
        state,
        user_id,
        "reel_inbox_update",
        json!({ "unread_count": unread }),
    ).await;
}

/// Get conversation thread
pub async fn get_reel_conversation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let reel_id: i32 = params.get("reel_id").and_then(|v| v.parse().ok()).ok_or_else(|| AppError::bad_request("reel_id required"))?;
    let other_user: i32 = params.get("other_user_id").and_then(|v| v.parse().ok()).ok_or_else(|| AppError::bad_request("other_user_id required"))?;
    let since = params.get("since").and_then(|s|
        NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S%.f").ok()
            .or_else(|| NaiveDateTime::parse_from_str(s, "%Y-%m-%dT%H:%M:%S").ok()));

    #[derive(sqlx::FromRow, Serialize)]
    struct ConvMsg { id: i32, sender_id: i32, content: String, message_type: Option<String>, is_read: Option<bool>, created_at: Option<NaiveDateTime> }

    let read_db = state.read_pool();
    let messages = if let Some(since_ts) = since {
        sqlx::query_as::<_, ConvMsg>(
            "SELECT id, sender_id, content, message_type, is_read, created_at FROM reel_messages WHERE reel_id = $1 AND ((sender_id = $2 AND receiver_id = $3) OR (sender_id = $3 AND receiver_id = $2)) AND created_at > $4 ORDER BY created_at ASC"
        ).bind(reel_id).bind(user_id).bind(other_user).bind(since_ts).fetch_all(read_db).await?
    } else {
        sqlx::query_as::<_, ConvMsg>(
            "SELECT id, sender_id, content, message_type, is_read, created_at FROM reel_messages WHERE reel_id = $1 AND ((sender_id = $2 AND receiver_id = $3) OR (sender_id = $3 AND receiver_id = $2)) ORDER BY created_at ASC"
        ).bind(reel_id).bind(user_id).bind(other_user).fetch_all(read_db).await?
    };

    let (user_a, user_b) = if user_id < other_user { (user_id, other_user) } else { (other_user, user_id) };

    #[derive(sqlx::FromRow)]
    struct ConvStats {
        total_messages: Option<i32>,
        eligible_for_match: Option<bool>,
        match_suggested: Option<bool>,
        match_accepted_a: Option<bool>,
        match_accepted_b: Option<bool>,
        match_id: Option<String>,
    }

    let stats = sqlx::query_as::<_, ConvStats>(
        "SELECT total_messages, eligible_for_match, match_suggested, match_accepted_a, match_accepted_b, match_id FROM reel_conversations WHERE reel_id = $1 AND user_a = $2 AND user_b = $3"
    ).bind(reel_id).bind(user_a).bind(user_b).fetch_optional(read_db).await?;

    let is_user_a = user_id == user_a;
    let (can_request_match, match_status) = if let Some(ref s) = stats {
        let already_matched = s.match_id.is_some();
        let i_accepted = if is_user_a { s.match_accepted_a } else { s.match_accepted_b };
        let they_accepted = if is_user_a { s.match_accepted_b } else { s.match_accepted_a };

        let status = if already_matched {
            "matched"
        } else if i_accepted == Some(true) && they_accepted == Some(true) {
            "matched"
        } else if i_accepted == Some(true) {
            "request_sent"
        } else if they_accepted == Some(true) {
            "request_received"
        } else if s.eligible_for_match.unwrap_or(false) {
            "eligible"
        } else {
            "chatting"
        };

        let can_request = s.eligible_for_match.unwrap_or(false)
            && !already_matched
            && i_accepted.is_none();

        (can_request, status)
    } else {
        (false, "no_conversation")
    };

    Ok(Json(json!({
        "messages": messages,
        "stats": {
            "total_messages": stats.as_ref().and_then(|s| s.total_messages),
            "eligible_for_match": stats.as_ref().and_then(|s| s.eligible_for_match).unwrap_or(false),
            "match_status": match_status,
            "can_request_match": can_request_match,
            "match_id": stats.as_ref().and_then(|s| s.match_id.clone())
        }
    })))
}

// ============================================================================
// Reel Match Request / Accept — Private conversation → Match
// ============================================================================

/// POST /reels/match-request — Request to match after a reel conversation
#[derive(Deserialize)]
pub struct ReelMatchRequestPayload {
    pub reel_id: i32,
    pub other_user_id: i32,
}

pub async fn request_reel_match(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReelMatchRequestPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    if user_id == payload.other_user_id {
        return Err(AppError::bad_request("Cannot match with yourself"));
    }

    let (user_a, user_b) = if user_id < payload.other_user_id {
        (user_id, payload.other_user_id)
    } else {
        (payload.other_user_id, user_id)
    };
    let is_user_a = user_id == user_a;

    // Check conversation exists and is eligible
    #[derive(sqlx::FromRow)]
    struct ConvCheck {
        eligible_for_match: Option<bool>,
        match_accepted_a: Option<bool>,
        match_accepted_b: Option<bool>,
        match_id: Option<String>,
    }

    let conv = sqlx::query_as::<_, ConvCheck>(
        "SELECT eligible_for_match, match_accepted_a, match_accepted_b, match_id FROM reel_conversations WHERE reel_id = $1 AND user_a = $2 AND user_b = $3"
    )
    .bind(payload.reel_id).bind(user_a).bind(user_b)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("No conversation found"))?;

    if conv.match_id.is_some() {
        return Err(AppError::bad_request("Already matched"));
    }

    if !conv.eligible_for_match.unwrap_or(false) {
        return Err(AppError::bad_request("Not eligible yet — keep chatting (both need 2+ messages)"));
    }

    let my_accepted = if is_user_a { conv.match_accepted_a } else { conv.match_accepted_b };
    if my_accepted == Some(true) {
        return Err(AppError::bad_request("Match request already sent"));
    }

    // Set my acceptance
    let col = if is_user_a { "match_accepted_a" } else { "match_accepted_b" };
    sqlx::query(&format!(
        "UPDATE reel_conversations SET {} = TRUE, match_suggested = TRUE, updated_at = NOW() WHERE reel_id = $1 AND user_a = $2 AND user_b = $3",
        col
    ))
    .bind(payload.reel_id).bind(user_a).bind(user_b)
    .execute(&state.db).await?;

    // Check if both accepted → auto-create match
    let other_accepted = if is_user_a { conv.match_accepted_b } else { conv.match_accepted_a };
    let is_match = other_accepted == Some(true);

    let match_id = if is_match {
        // Both accepted — create the real match!
        let mid = create_match_from_reel(&state.db, user_a, user_b, payload.reel_id).await?;

        // Update conversation with match_id
        sqlx::query("UPDATE reel_conversations SET match_id = $4, updated_at = NOW() WHERE reel_id = $1 AND user_a = $2 AND user_b = $3")
            .bind(payload.reel_id).bind(user_a).bind(user_b).bind(&mid)
            .execute(&state.db).await?;

        // Update ML stats
        let _ = sqlx::query(
            "UPDATE user_interaction_model SET total_matches_from_reels = COALESCE(total_matches_from_reels, 0) + 1, updated_at = NOW() WHERE user_id = $1"
        ).bind(user_id as i64).execute(&state.db).await;
        let _ = sqlx::query(
            "UPDATE user_interaction_model SET total_matches_from_reels = COALESCE(total_matches_from_reels, 0) + 1, updated_at = NOW() WHERE user_id = $1"
        ).bind(payload.other_user_id as i64).execute(&state.db).await;

        Some(mid)
    } else {
        None
    };

    // Publish notification events
    if is_match {
        if let Some(ref mid) = match_id {
            state.event_bus.publish("reel_handler", crate::modules::events::DomainEvent::ReelMatchAccepted {
                reel_id: payload.reel_id, match_id: mid.clone(), user1_id: user_a, user2_id: user_b,
            });
        }
    } else {
        state.event_bus.publish("reel_handler", crate::modules::events::DomainEvent::ReelMatchRequested {
            reel_id: payload.reel_id, requester_id: user_id, target_id: payload.other_user_id,
        });
    }

    Ok(Json(json!({
        "request_sent": true,
        "is_match": is_match,
        "match_id": match_id,
        "status": if is_match { "matched" } else { "request_sent" }
    })))
}

/// POST /reels/match-accept — Accept or decline a match request
#[derive(Deserialize)]
pub struct ReelMatchAcceptPayload {
    pub reel_id: i32,
    pub other_user_id: i32,
    pub accept: bool,
}

pub async fn accept_reel_match(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReelMatchAcceptPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let (user_a, user_b) = if user_id < payload.other_user_id {
        (user_id, payload.other_user_id)
    } else {
        (payload.other_user_id, user_id)
    };
    let is_user_a = user_id == user_a;

    // Verify the other person already sent a request
    #[derive(sqlx::FromRow)]
    struct ConvCheck2 {
        match_accepted_a: Option<bool>,
        match_accepted_b: Option<bool>,
        match_id: Option<String>,
    }

    let conv = sqlx::query_as::<_, ConvCheck2>(
        "SELECT match_accepted_a, match_accepted_b, match_id FROM reel_conversations WHERE reel_id = $1 AND user_a = $2 AND user_b = $3"
    )
    .bind(payload.reel_id).bind(user_a).bind(user_b)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("No conversation found"))?;

    if conv.match_id.is_some() {
        return Err(AppError::bad_request("Already matched"));
    }

    let other_accepted = if is_user_a { conv.match_accepted_b } else { conv.match_accepted_a };
    if other_accepted != Some(true) {
        return Err(AppError::bad_request("No pending match request from this user"));
    }

    if !payload.accept {
        // Decline — reset their request
        let other_col = if is_user_a { "match_accepted_b" } else { "match_accepted_a" };
        sqlx::query(&format!(
            "UPDATE reel_conversations SET {} = NULL, updated_at = NOW() WHERE reel_id = $1 AND user_a = $2 AND user_b = $3",
            other_col
        ))
        .bind(payload.reel_id).bind(user_a).bind(user_b)
        .execute(&state.db).await?;

        return Ok(Json(json!({
            "accepted": false,
            "status": "declined",
            "message": "Match request declined. They can request again later."
        })));
    }

    // Accept — set my acceptance and create match
    let col = if is_user_a { "match_accepted_a" } else { "match_accepted_b" };
    sqlx::query(&format!(
        "UPDATE reel_conversations SET {} = TRUE, updated_at = NOW() WHERE reel_id = $1 AND user_a = $2 AND user_b = $3",
        col
    ))
    .bind(payload.reel_id).bind(user_a).bind(user_b)
    .execute(&state.db).await?;

    // Create the match
    let match_id = create_match_from_reel(&state.db, user_a, user_b, payload.reel_id).await?;

    sqlx::query("UPDATE reel_conversations SET match_id = $4, updated_at = NOW() WHERE reel_id = $1 AND user_a = $2 AND user_b = $3")
        .bind(payload.reel_id).bind(user_a).bind(user_b).bind(&match_id)
        .execute(&state.db).await?;

    // Update ML stats for both users
    for uid in [user_a, user_b] {
        let _ = sqlx::query(
            "UPDATE user_interaction_model SET total_matches_from_reels = COALESCE(total_matches_from_reels, 0) + 1, updated_at = NOW() WHERE user_id = $1"
        ).bind(uid as i64).execute(&state.db).await;
    }

    // Notify both users about the match
    state.event_bus.publish("reel_handler", crate::modules::events::DomainEvent::ReelMatchAccepted {
        reel_id: payload.reel_id, match_id: match_id.clone(), user1_id: user_a, user2_id: user_b,
    });

    Ok(Json(json!({
        "accepted": true,
        "is_match": true,
        "match_id": match_id,
        "status": "matched",
        "message": "It's a match! You can now chat freely."
    })))
}

/// Create a real match record from a reel conversation
async fn create_match_from_reel(db: &PgPool, user1: i32, user2: i32, _reel_id: i32) -> Result<String, AppError> {
    // Check if match already exists
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT id FROM matches WHERE user1_id = $1 AND user2_id = $2"
    )
    .bind(user1).bind(user2)
    .fetch_optional(db).await?;

    if let Some(mid) = existing {
        // Update existing to mutual match
        sqlx::query(
            "UPDATE matches SET is_mutual_match = TRUE, user1_liked = TRUE, user2_liked = TRUE, status = 'accepted' WHERE id = $1"
        ).bind(&mid).execute(db).await?;
        return Ok(mid);
    }

    // Create new mutual match
    let new_match_id = Uuid::new_v4().to_string();
    sqlx::query(
        r#"
        INSERT INTO matches (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, status, match_reason, created_at)
        VALUES ($1, $2, $3, TRUE, TRUE, TRUE, 'accepted', 'reel_conversation', NOW())
        "#,
    )
    .bind(&new_match_id).bind(user1).bind(user2)
    .execute(db).await?;
    let match_id = new_match_id;

    // Record swipes for both (for consistency with discover flow)
    let _ = sqlx::query(
        "INSERT INTO swipes (from_user_id, to_user_id, action, source) VALUES ($1, $2, 'like', 'reel') ON CONFLICT DO NOTHING"
    ).bind(user1 as i64).bind(user2 as i64).execute(db).await;
    let _ = sqlx::query(
        "INSERT INTO swipes (from_user_id, to_user_id, action, source) VALUES ($1, $2, 'like', 'reel') ON CONFLICT DO NOTHING"
    ).bind(user2 as i64).bind(user1 as i64).execute(db).await;

    Ok(match_id)
}

/// Get user's learned patterns (what ML learned about them)
pub async fn get_my_learned_patterns(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    #[derive(sqlx::FromRow, Serialize)]
    struct ContentPrefs { preferred_categories: Option<Value>, preferred_tags: Option<Value>, completion_rate: Option<f64>, like_rate: Option<f64>, message_rate: Option<f64>, response_rate: Option<f64> }

    #[derive(sqlx::FromRow, Serialize)]
    struct RespPatterns { successful_categories: Option<Value>, successful_opener_types: Option<Value>, response_rate: Option<f64>, conversations_continued: Option<i32>, matches_from_reels: Option<i32> }

    #[derive(sqlx::FromRow, Serialize)]
    struct IntStats { total_swipes: Option<i32>, total_matches_from_swipes: Option<i32>, swipe_success_rate: Option<f64>, total_reel_interactions: Option<i32>, total_matches_from_reels: Option<i32>, reel_success_rate: Option<f64>, best_interaction_mode: Option<String> }

    let read_db = state.read_pool();
    let content = sqlx::query_as::<_, ContentPrefs>("SELECT preferred_categories, preferred_tags, completion_rate, like_rate, message_rate, response_rate FROM user_content_preferences WHERE user_id = $1").bind(user_id).fetch_optional(read_db).await?;
    let response = sqlx::query_as::<_, RespPatterns>("SELECT successful_categories, successful_opener_types, response_rate, conversations_continued, matches_from_reels FROM user_response_patterns WHERE user_id = $1").bind(user_id).fetch_optional(read_db).await?;
    let interaction = sqlx::query_as::<_, IntStats>("SELECT total_swipes, total_matches_from_swipes, swipe_success_rate, total_reel_interactions, total_matches_from_reels, reel_success_rate, best_interaction_mode FROM user_interaction_model WHERE user_id = $1").bind(user_id).fetch_optional(read_db).await?;

    Ok(Json(json!({ "content_preferences": content, "response_patterns": response, "interaction_stats": interaction })))
}

// ============================================================================
// Helper functions for reel ML
// ============================================================================

fn calc_interest_score(
    watch_pct: f64, duration: i32, rewatched: bool,
    scroll_vel: Option<f64>, seek_fwd: i32, seek_bwd: i32, pauses: i32,
) -> f64 {
    let mut score = (watch_pct / 100.0) * 0.35;
    if rewatched { score += 0.15; }
    score += ((duration as f64) / 30.0).min(1.0) * 0.15;
    if let Some(v) = scroll_vel { score += (1.0 - (v / 100.0).min(1.0)) * 0.10; }
    // Seek backward = positive: user rewound to re-watch something (high interest)
    score += (seek_bwd as f64 * 0.05).min(0.15);
    // Seek forward = negative: user skipped ahead (low interest in that section)
    score -= (seek_fwd as f64 * 0.04).min(0.15);
    // Pauses = positive: user stopped to read caption / look at something
    score += (pauses as f64 * 0.03).min(0.10);
    score.clamp(0.0, 1.0)
}

fn calc_message_effort(content: &str, has_reaction: bool) -> f64 {
    let mut score = (content.len() as f64 / 200.0).min(1.0) * 0.3;
    if content.contains('?') { score += 0.2; }
    if !has_reaction && content.len() > 10 { score += 0.2; }
    if content.matches('.').count() + content.matches('!').count() + content.matches('?').count() >= 2 { score += 0.2; }
    if content.len() < 5 { score = 0.1; }
    score.min(1.0)
}

#[allow(clippy::too_many_arguments)]
async fn log_reel_event(
    db: &PgPool,
    user_id: i32, reel_id: i32, owner_id: i32, event_type: &str,
    watch_pct: f64, duration_ms: i64, scroll_vel: Option<f64>,
    source: Option<&str>, position: Option<i32>,
    seek_fwd: i32, seek_bwd: i32, pauses: i32,
    same_city: bool, liked: bool, messaged: bool, message_length: i32,
    reward: f64, reward_version: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        r#"INSERT INTO reel_engagement_events
            (user_id, reel_id, reel_owner_id, event_type,
             watch_percent, time_on_reel_sec, watch_duration_ms, scroll_velocity,
             source, position_in_feed,
             seek_forward_count, seek_backward_count, pause_count,
             same_city, liked, messaged, message_length,
             reward, reward_version, created_at)
           VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,NOW())"#,
    )
    .bind(user_id).bind(reel_id).bind(owner_id).bind(event_type)
    .bind(watch_pct).bind((duration_ms / 1000) as i32).bind(duration_ms).bind(scroll_vel)
    .bind(source).bind(position)
    .bind(seek_fwd).bind(seek_bwd).bind(pauses)
    .bind(same_city).bind(liked).bind(messaged).bind(message_length)
    .bind(reward).bind(reward_version)
    .execute(db).await?;
    Ok(())
}

async fn update_content_prefs(db: &PgPool, user_id: i32, reel_id: i32, score: f64) -> Result<(), sqlx::Error> {
    let cat: Option<String> = sqlx::query_scalar("SELECT category FROM reels WHERE id = $1").bind(reel_id).fetch_optional(db).await?;
    if let Some(c) = cat {
        sqlx::query(r#"INSERT INTO user_content_preferences (user_id, preferred_categories, updated_at) VALUES ($1, jsonb_build_object($2, $3), NOW())
            ON CONFLICT (user_id) DO UPDATE SET preferred_categories = jsonb_set(COALESCE(user_content_preferences.preferred_categories, '{}'::jsonb), ARRAY[$2], to_jsonb(COALESCE((user_content_preferences.preferred_categories->>$2)::float, 0) * 0.9 + $3 * 0.1)), updated_at = NOW()"#)
            .bind(user_id).bind(&c).bind(score).execute(db).await?;
    }
    Ok(())
}

async fn check_reel_match_eligibility(db: &PgPool, reel_id: i32, user_a: i32, user_b: i32) -> Result<(), sqlx::Error> {
    let conv: Option<(i32, i32)> = sqlx::query_as("SELECT a_message_count, b_message_count FROM reel_conversations WHERE reel_id = $1 AND user_a = $2 AND user_b = $3")
        .bind(reel_id).bind(user_a).bind(user_b).fetch_optional(db).await?;
    if let Some((a, b)) = conv {
        if a >= 2 && b >= 2 {
            sqlx::query("UPDATE reel_conversations SET eligible_for_match = TRUE, updated_at = NOW() WHERE reel_id = $1 AND user_a = $2 AND user_b = $3").bind(reel_id).bind(user_a).bind(user_b).execute(db).await?;
        }
    }
    Ok(())
}

// ============================================================================
// LLM AUTO-LABELING SYSTEM
// Labels reels, messages, and user interactions for ML training
// ============================================================================

/// Auto-queue content for LLM labeling (fire-and-forget, never fails the caller)
pub fn auto_queue_for_labeling(db: sqlx::PgPool, config_enabled: bool, content_type: &str, content_id: i64, priority: i32) {
    if !config_enabled { return; }
    let ct = content_type.to_string();
    tokio::spawn(async move {
        let _ = sqlx::query(
            r#"INSERT INTO llm_labeling_queue (content_type, content_id, priority, status, created_at)
               VALUES ($1, $2, $3, 'pending', NOW())
               ON CONFLICT (content_type, content_id) WHERE status = 'pending'
               DO UPDATE SET priority = LEAST(llm_labeling_queue.priority, $3)"#,
        )
        .bind(&ct)
        .bind(content_id)
        .bind(priority)
        .execute(&db)
        .await;
    });
}

/// Write a user→user graph edge (fire-and-forget, never fails the caller).
/// Writes to both forward and reverse indexes for bidirectional traversal.
pub fn write_user_edge(db: sqlx::PgPool, from_id: i32, to_id: i32, edge_type: &'static str) {
    let from_s = from_id.to_string();
    let to_s = to_id.to_string();
    tokio::spawn(async move {
        let _ = sqlx::query(
            "INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id) VALUES ('user', $1, $2, 'user', $3) ON CONFLICT DO NOTHING"
        ).bind(&from_s).bind(edge_type).bind(&to_s).execute(&db).await;
        let _ = sqlx::query(
            "INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id) VALUES ('user', $3, $2, 'user', $1) ON CONFLICT DO NOTHING"
        ).bind(&from_s).bind(edge_type).bind(&to_s).execute(&db).await;
    });
}

/// Queue a reel for LLM labeling
pub async fn queue_reel_labeling(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<QueueLabelingPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let queue_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO llm_labeling_queue (content_type, content_id, priority, status, created_at)
        VALUES ($1, $2, $3, 'pending', NOW())
        ON CONFLICT (content_type, content_id) WHERE status = 'pending'
        DO UPDATE SET priority = LEAST(llm_labeling_queue.priority, $3)
        RETURNING id
        "#,
    )
    .bind(&payload.content_type)
    .bind(payload.content_id)
    .bind(payload.priority.unwrap_or(5))
    .fetch_one(&state.db)
    .await?;

    Ok(Json(json!({ "queue_id": queue_id, "status": "queued" })))
}

#[derive(Deserialize)]
pub struct QueueLabelingPayload {
    pub content_type: String,  // reel, message, user
    pub content_id: i64,
    pub priority: Option<i32>,
}

/// Get next batch of items to label (for LLM worker)
pub async fn get_labeling_batch(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let batch_size: i32 = params.get("batch_size").and_then(|v| v.parse().ok()).unwrap_or(10);
    let content_type = params.get("content_type").cloned();

    #[derive(sqlx::FromRow, Serialize)]
    struct QueueItem {
        id: i64,
        content_type: String,
        content_id: i64,
        priority: Option<i32>,
        retry_count: Option<i32>,
    }

    let items = if let Some(ct) = content_type {
        sqlx::query_as::<_, QueueItem>(
            r#"
            UPDATE llm_labeling_queue SET status = 'processing', started_at = NOW()
            WHERE id IN (
                SELECT id FROM llm_labeling_queue
                WHERE status = 'pending' AND content_type = $1
                ORDER BY priority ASC, created_at ASC
                LIMIT $2
                FOR UPDATE SKIP LOCKED
            )
            RETURNING id, content_type, content_id, priority, retry_count
            "#,
        )
        .bind(&ct)
        .bind(batch_size)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, QueueItem>(
            r#"
            UPDATE llm_labeling_queue SET status = 'processing', started_at = NOW()
            WHERE id IN (
                SELECT id FROM llm_labeling_queue
                WHERE status = 'pending'
                ORDER BY priority ASC, created_at ASC
                LIMIT $1
                FOR UPDATE SKIP LOCKED
            )
            RETURNING id, content_type, content_id, priority, retry_count
            "#,
        )
        .bind(batch_size)
        .fetch_all(&state.db)
        .await?
    };

    // Fetch content for each item
    let mut enriched_items = Vec::new();
    for item in &items {
        let content = match item.content_type.as_str() {
            "reel" => {
                let reel = sqlx::query_as::<_, ReelContent>(
                    "SELECT id, video_url, caption, tags, category, audio_track FROM reels WHERE id = $1"
                ).bind(item.content_id).fetch_optional(&state.db).await?.map(|r| serde_json::to_value(r).ok()).flatten();
                reel
            }
            "message" => {
                let msg = sqlx::query_as::<_, MessageContent>(
                    "SELECT id, content, message_type, reel_id FROM reel_messages WHERE id = $1"
                ).bind(item.content_id).fetch_optional(&state.db).await?.map(|m| serde_json::to_value(m).ok()).flatten();
                msg
            }
            "user" => {
                let user = sqlx::query_as::<_, UserContent>(
                    "SELECT id, bio FROM users WHERE id = $1"
                ).bind(item.content_id).fetch_optional(&state.db).await?.map(|u| serde_json::to_value(u).ok()).flatten();
                user
            }
            _ => None
        };
        enriched_items.push(json!({
            "queue_id": item.id,
            "content_type": item.content_type,
            "content_id": item.content_id,
            "content": content
        }));
    }

    Ok(Json(json!({ "items": enriched_items, "count": items.len() })))
}

#[derive(sqlx::FromRow, Serialize)]
struct ReelContent { id: i64, video_url: String, caption: Option<String>, tags: Option<Value>, category: Option<String>, audio_track: Option<String> }

#[derive(sqlx::FromRow, Serialize)]
struct MessageContent { id: i64, content: String, message_type: Option<String>, reel_id: i32 }

#[derive(sqlx::FromRow, Serialize)]
struct UserContent { id: i64, bio: Option<String> }

/// Submit LLM labels for a reel
#[derive(Deserialize)]
pub struct ReelLabelsPayload {
    pub queue_id: i64,
    pub reel_id: i32,
    pub content_summary: Option<String>,
    pub detected_topics: Option<Vec<String>>,
    pub detected_mood: Option<String>,
    pub detected_intent: Option<String>,
    pub detected_setting: Option<String>,
    pub detected_activity: Option<String>,
    pub production_quality: Option<f64>,
    pub creativity_score: Option<f64>,
    pub engagement_potential: Option<f64>,
    pub authenticity_score: Option<f64>,
    pub dating_appeal_score: Option<f64>,
    pub personality_traits: Option<Value>,
    pub conversation_starters: Option<Vec<String>>,
    pub nsfw_score: Option<f64>,
    pub spam_score: Option<f64>,
    pub catfish_risk: Option<f64>,
    pub content_embedding: Option<Vec<f64>>,
    pub llm_model: String,
    pub confidence: f64,
    pub processing_time_ms: i32,
}

pub async fn submit_reel_labels(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReelLabelsPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let topics_json = payload.detected_topics.as_ref().and_then(|t| serde_json::to_value(t).ok());
    let starters_json = payload.conversation_starters.as_ref().and_then(|s| serde_json::to_value(s).ok());
    let embedding_json = payload.content_embedding.as_ref().and_then(|e| serde_json::to_value(e).ok());

    sqlx::query(
        r#"
        INSERT INTO reel_llm_labels (
            reel_id, content_summary, detected_topics, detected_mood, detected_intent,
            detected_setting, detected_activity, production_quality, creativity_score,
            engagement_potential, authenticity_score, dating_appeal_score, personality_traits,
            conversation_starters, nsfw_score, spam_score, catfish_risk, content_embedding,
            llm_model, confidence, processing_time_ms, labeled_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,NOW())
        ON CONFLICT (reel_id) DO UPDATE SET
            content_summary = $2, detected_topics = $3, detected_mood = $4, detected_intent = $5,
            detected_setting = $6, detected_activity = $7, production_quality = $8, creativity_score = $9,
            engagement_potential = $10, authenticity_score = $11, dating_appeal_score = $12,
            personality_traits = $13, conversation_starters = $14, nsfw_score = $15, spam_score = $16,
            catfish_risk = $17, content_embedding = $18, llm_model = $19, confidence = $20,
            processing_time_ms = $21, labeled_at = NOW()
        "#,
    )
    .bind(payload.reel_id)
    .bind(&payload.content_summary)
    .bind(&topics_json)
    .bind(&payload.detected_mood)
    .bind(&payload.detected_intent)
    .bind(&payload.detected_setting)
    .bind(&payload.detected_activity)
    .bind(payload.production_quality)
    .bind(payload.creativity_score)
    .bind(payload.engagement_potential)
    .bind(payload.authenticity_score)
    .bind(payload.dating_appeal_score)
    .bind(&payload.personality_traits)
    .bind(&starters_json)
    .bind(payload.nsfw_score)
    .bind(payload.spam_score)
    .bind(payload.catfish_risk)
    .bind(&embedding_json)
    .bind(&payload.llm_model)
    .bind(payload.confidence)
    .bind(payload.processing_time_ms)
    .execute(&state.db)
    .await?;

    // Update reel with engagement potential for ranking
    if let Some(engagement) = payload.engagement_potential {
        sqlx::query("UPDATE reels SET engagement_score = $2, content_embedding = $3, updated_at = NOW() WHERE id = $1")
            .bind(payload.reel_id)
            .bind(engagement)
            .bind(&embedding_json)
            .execute(&state.db)
            .await?;
    }

    // Mark queue item complete
    sqlx::query("UPDATE llm_labeling_queue SET status = 'completed', completed_at = NOW() WHERE id = $1")
        .bind(payload.queue_id)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({ "labeled": true, "reel_id": payload.reel_id })))
}

/// Submit LLM labels for a message
#[derive(Deserialize)]
pub struct MessageLabelsPayload {
    pub queue_id: i64,
    pub message_id: i64,
    pub message_type: Option<String>,
    pub sentiment: Option<String>,
    pub sentiment_score: Option<f64>,
    pub intent: Option<String>,
    pub effort_score: Option<f64>,
    pub personalization_score: Option<f64>,
    pub creativity_score: Option<f64>,
    pub conversation_value: Option<f64>,
    pub has_question: bool,
    pub has_compliment: bool,
    pub has_humor: bool,
    pub has_emoji: bool,
    pub references_reel: bool,
    pub references_profile: bool,
    pub word_count: i32,
    pub predicted_response_prob: Option<f64>,
    pub predicted_response_quality: Option<String>,
    pub spam_score: Option<f64>,
    pub creepy_score: Option<f64>,
    pub generic_score: Option<f64>,
    pub message_embedding: Option<Vec<f64>>,
    pub llm_model: String,
    pub confidence: f64,
}

pub async fn submit_message_labels(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<MessageLabelsPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let embedding_json = payload.message_embedding.as_ref().and_then(|e| serde_json::to_value(e).ok());

    sqlx::query(
        r#"
        INSERT INTO message_llm_labels (
            message_id, message_type, sentiment, sentiment_score, intent, effort_score,
            personalization_score, creativity_score, conversation_value, has_question,
            has_compliment, has_humor, has_emoji, references_reel, references_profile,
            word_count, predicted_response_prob, predicted_response_quality, spam_score,
            creepy_score, generic_score, message_embedding, llm_model, confidence, labeled_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,$24,NOW())
        ON CONFLICT (message_id) DO UPDATE SET
            message_type = $2, sentiment = $3, sentiment_score = $4, intent = $5, effort_score = $6,
            personalization_score = $7, creativity_score = $8, conversation_value = $9,
            has_question = $10, has_compliment = $11, has_humor = $12, has_emoji = $13,
            references_reel = $14, references_profile = $15, word_count = $16,
            predicted_response_prob = $17, predicted_response_quality = $18, spam_score = $19,
            creepy_score = $20, generic_score = $21, message_embedding = $22, llm_model = $23,
            confidence = $24, labeled_at = NOW()
        "#,
    )
    .bind(payload.message_id)
    .bind(&payload.message_type)
    .bind(&payload.sentiment)
    .bind(payload.sentiment_score)
    .bind(&payload.intent)
    .bind(payload.effort_score)
    .bind(payload.personalization_score)
    .bind(payload.creativity_score)
    .bind(payload.conversation_value)
    .bind(payload.has_question)
    .bind(payload.has_compliment)
    .bind(payload.has_humor)
    .bind(payload.has_emoji)
    .bind(payload.references_reel)
    .bind(payload.references_profile)
    .bind(payload.word_count)
    .bind(payload.predicted_response_prob)
    .bind(&payload.predicted_response_quality)
    .bind(payload.spam_score)
    .bind(payload.creepy_score)
    .bind(payload.generic_score)
    .bind(&embedding_json)
    .bind(&payload.llm_model)
    .bind(payload.confidence)
    .execute(&state.db)
    .await?;

    sqlx::query("UPDATE llm_labeling_queue SET status = 'completed', completed_at = NOW() WHERE id = $1")
        .bind(payload.queue_id)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({ "labeled": true, "message_id": payload.message_id })))
}

/// Submit LLM labels for a user profile
#[derive(Deserialize)]
pub struct UserLabelsPayload {
    pub queue_id: i64,
    pub user_id: i32,
    pub bio_quality_score: Option<f64>,
    pub bio_authenticity: Option<f64>,
    pub photo_consistency: Option<f64>,
    pub profile_completeness: Option<f64>,
    pub personality_summary: Option<String>,
    pub big_five: Option<Value>,
    pub communication_style: Option<String>,
    pub humor_type: Option<String>,
    pub dating_style: Option<String>,
    pub relationship_goals: Option<Value>,
    pub dealbreakers: Option<Value>,
    pub opener_style: Option<String>,
    pub avg_message_quality: Option<f64>,
    pub response_pattern: Option<String>,
    pub conversation_depth: Option<String>,
    pub catfish_probability: Option<f64>,
    pub bot_probability: Option<f64>,
    pub toxic_probability: Option<f64>,
    pub personality_embedding: Option<Vec<f64>>,
    pub preference_embedding: Option<Vec<f64>>,
    pub samples_analyzed: i32,
    pub llm_model: String,
}

pub async fn submit_user_labels(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<UserLabelsPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let personality_emb = payload.personality_embedding.as_ref().and_then(|e| serde_json::to_value(e).ok());
    let preference_emb = payload.preference_embedding.as_ref().and_then(|e| serde_json::to_value(e).ok());

    sqlx::query(
        r#"
        INSERT INTO user_llm_labels (
            user_id, bio_quality_score, bio_authenticity, photo_consistency, profile_completeness,
            personality_summary, big_five, communication_style, humor_type, dating_style,
            relationship_goals, dealbreakers, opener_style, avg_message_quality, response_pattern,
            conversation_depth, catfish_probability, bot_probability, toxic_probability,
            personality_embedding, preference_embedding, samples_analyzed, llm_model, last_analyzed_at, updated_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20,$21,$22,$23,NOW(),NOW())
        ON CONFLICT (user_id) DO UPDATE SET
            bio_quality_score = $2, bio_authenticity = $3, photo_consistency = $4, profile_completeness = $5,
            personality_summary = $6, big_five = $7, communication_style = $8, humor_type = $9,
            dating_style = $10, relationship_goals = $11, dealbreakers = $12, opener_style = $13,
            avg_message_quality = $14, response_pattern = $15, conversation_depth = $16,
            catfish_probability = $17, bot_probability = $18, toxic_probability = $19,
            personality_embedding = $20, preference_embedding = $21, samples_analyzed = $22,
            llm_model = $23, last_analyzed_at = NOW(), model_version = user_llm_labels.model_version + 1, updated_at = NOW()
        "#,
    )
    .bind(payload.user_id)
    .bind(payload.bio_quality_score)
    .bind(payload.bio_authenticity)
    .bind(payload.photo_consistency)
    .bind(payload.profile_completeness)
    .bind(&payload.personality_summary)
    .bind(&payload.big_five)
    .bind(&payload.communication_style)
    .bind(&payload.humor_type)
    .bind(&payload.dating_style)
    .bind(&payload.relationship_goals)
    .bind(&payload.dealbreakers)
    .bind(&payload.opener_style)
    .bind(payload.avg_message_quality)
    .bind(&payload.response_pattern)
    .bind(&payload.conversation_depth)
    .bind(payload.catfish_probability)
    .bind(payload.bot_probability)
    .bind(payload.toxic_probability)
    .bind(&personality_emb)
    .bind(&preference_emb)
    .bind(payload.samples_analyzed)
    .bind(&payload.llm_model)
    .execute(&state.db)
    .await?;

    sqlx::query("UPDATE llm_labeling_queue SET status = 'completed', completed_at = NOW() WHERE id = $1")
        .bind(payload.queue_id)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({ "labeled": true, "user_id": payload.user_id })))
}

/// Mark labeling as failed (for retry)
#[derive(Deserialize)]
pub struct LabelingFailedPayload {
    pub queue_id: i64,
    pub error_message: String,
}

pub async fn mark_labeling_failed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LabelingFailedPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let result = sqlx::query_as::<_, (i32, i32)>(
        "SELECT retry_count, max_retries FROM llm_labeling_queue WHERE id = $1"
    )
    .bind(payload.queue_id)
    .fetch_optional(&state.db)
    .await?;

    if let Some((retry_count, max_retries)) = result {
        let new_status = if retry_count + 1 >= max_retries { "failed" } else { "pending" };
        sqlx::query(
            "UPDATE llm_labeling_queue SET status = $2, retry_count = retry_count + 1, error_message = $3, started_at = NULL WHERE id = $1"
        )
        .bind(payload.queue_id)
        .bind(new_status)
        .bind(&payload.error_message)
        .execute(&state.db)
        .await?;
    }

    Ok(Json(json!({ "marked_failed": true })))
}

/// Get LLM labels for a reel
pub async fn get_reel_labels(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let reel_id: i32 = params.get("reel_id").and_then(|v| v.parse().ok()).ok_or_else(|| AppError::bad_request("reel_id required"))?;

    #[derive(sqlx::FromRow, Serialize)]
    struct ReelLabels {
        content_summary: Option<String>,
        detected_topics: Option<Value>,
        detected_mood: Option<String>,
        detected_intent: Option<String>,
        detected_setting: Option<String>,
        detected_activity: Option<String>,
        production_quality: Option<f64>,
        creativity_score: Option<f64>,
        engagement_potential: Option<f64>,
        dating_appeal_score: Option<f64>,
        personality_traits: Option<Value>,
        conversation_starters: Option<Value>,
        nsfw_score: Option<f64>,
        confidence: Option<f64>,
        labeled_at: Option<NaiveDateTime>,
    }

    let labels = sqlx::query_as::<_, ReelLabels>(
        "SELECT content_summary, detected_topics, detected_mood, detected_intent, detected_setting, detected_activity, production_quality, creativity_score, engagement_potential, dating_appeal_score, personality_traits, conversation_starters, nsfw_score, confidence, labeled_at FROM reel_llm_labels WHERE reel_id = $1"
    )
    .bind(reel_id)
    .fetch_optional(&state.db)
    .await?;

    Ok(Json(json!({ "labels": labels })))
}

// ============================================================================
// FEDERATED LEARNING SYSTEM
// Privacy-preserving distributed model training
// ============================================================================

/// Register a client device for federated learning
#[derive(Deserialize)]
pub struct RegisterFLClientPayload {
    pub device_id: String,
    pub device_type: Option<String>,
    pub device_model: Option<String>,
    pub os_version: Option<String>,
    pub app_version: Option<String>,
    pub compute_tier: Option<String>,
    pub battery_threshold: Option<i32>,
    pub wifi_only: Option<bool>,
}

pub async fn register_fl_client(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<RegisterFLClientPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let client_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO fl_clients (user_id, device_id, device_type, device_model, os_version, app_version, compute_tier, battery_threshold, wifi_only, created_at, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW(), NOW())
        ON CONFLICT (user_id, device_id) DO UPDATE SET
            device_type = $3, device_model = $4, os_version = $5, app_version = $6,
            compute_tier = $7, battery_threshold = $8, wifi_only = $9, is_active = TRUE, updated_at = NOW()
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(&payload.device_id)
    .bind(&payload.device_type)
    .bind(&payload.device_model)
    .bind(&payload.os_version)
    .bind(&payload.app_version)
    .bind(&payload.compute_tier)
    .bind(payload.battery_threshold.unwrap_or(50))
    .bind(payload.wifi_only.unwrap_or(true))
    .fetch_one(&state.db)
    .await?;

    Ok(Json(json!({ "client_id": client_id, "registered": true })))
}

/// Get current FL round for participation
pub async fn get_fl_round(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let model_type = params.get("model_type").cloned().unwrap_or_else(|| "recommendation".to_string());
    let device_id = params.get("device_id").ok_or_else(|| AppError::bad_request("device_id required"))?;

    // Check if client is registered and eligible
    let client = sqlx::query_as::<_, (i64, bool, bool)>(
        "SELECT id, is_active, opted_in FROM fl_clients WHERE user_id = $1 AND device_id = $2"
    )
    .bind(user_id)
    .bind(device_id)
    .fetch_optional(&state.db)
    .await?;

    let (client_id, is_active, opted_in) = client.ok_or_else(|| AppError::bad_request("Client not registered"))?;
    if !is_active || !opted_in {
        return Ok(Json(json!({ "eligible": false, "reason": "Client not active or not opted in" })));
    }

    // Get current active round
    #[derive(sqlx::FromRow, Serialize)]
    struct FLRound {
        id: i64,
        round_number: i32,
        model_type: String,
        local_epochs: Option<i32>,
        batch_size: Option<i32>,
        learning_rate: Option<f64>,
        global_weights: Option<Value>,
        model_version: Option<i32>,
        differential_privacy: Option<bool>,
        noise_multiplier: Option<f64>,
        clip_norm: Option<f64>,
    }

    let round = sqlx::query_as::<_, FLRound>(
        r#"
        SELECT id, round_number, model_type, local_epochs, batch_size, learning_rate,
               global_weights, model_version, differential_privacy, noise_multiplier, clip_norm
        FROM fl_rounds
        WHERE model_type = $1 AND status = 'in_progress'
        ORDER BY round_number DESC LIMIT 1
        "#,
    )
    .bind(&model_type)
    .fetch_optional(&state.db)
    .await?;

    if let Some(r) = round {
        // Check if client already participated in this round
        let already_participated = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM fl_client_updates WHERE round_id = $1 AND client_id = $2"
        )
        .bind(r.id)
        .bind(client_id)
        .fetch_one(&state.db)
        .await? > 0;

        if already_participated {
            return Ok(Json(json!({ "eligible": false, "reason": "Already participated in this round" })));
        }

        // Include feature schema so the client knows what the model dimensions represent
        let schema = crate::ml::features::feature_schema();

        Ok(Json(json!({
            "eligible": true,
            "round": r,
            "client_id": client_id,
            "feature_schema": schema
        })))
    } else {
        Ok(Json(json!({ "eligible": false, "reason": "No active round" })))
    }
}

/// Submit local model update from client
#[derive(Deserialize)]
pub struct SubmitFLUpdatePayload {
    pub round_id: i64,
    pub client_id: i64,
    pub local_weights: Value,        // DP-noised local weights
    pub weight_delta: Option<Value>, // Optional: difference from global
    pub num_samples: i32,
    pub local_loss: f64,
    pub local_accuracy: Option<f64>,
    pub training_time_ms: i32,
    pub dp_epsilon: Option<f64>,
    pub dp_delta: Option<f64>,
    pub checksum: String,
    /// Summary of features used in local training (profile, swipe, engagement stats)
    pub feature_summary: Option<Value>,
}

pub async fn submit_fl_update(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<SubmitFLUpdatePayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Verify client ownership
    let client_user = sqlx::query_scalar::<_, i32>("SELECT user_id FROM fl_clients WHERE id = $1")
        .bind(payload.client_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("Client not found"))?;

    if client_user != user_id {
        return Err(AppError::forbidden("Not your client"));
    }

    // Verify round is still accepting updates
    let round_status = sqlx::query_scalar::<_, String>("SELECT status FROM fl_rounds WHERE id = $1")
        .bind(payload.round_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("Round not found"))?;

    if round_status != "in_progress" {
        return Err(AppError::bad_request("Round is not accepting updates"));
    }

    let update_id = sqlx::query_scalar::<_, i32>(
        r#"
        INSERT INTO fl_client_updates (
            round_id, client_id, local_weights, weight_delta, num_samples, local_loss,
            local_accuracy, training_time_ms, dp_epsilon, dp_delta, noise_added, checksum,
            status, received_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,TRUE,$11,'received',NOW())
        RETURNING id
        "#,
    )
    .bind(payload.round_id)
    .bind(payload.client_id)
    .bind(&payload.local_weights)
    .bind(&payload.weight_delta)
    .bind(payload.num_samples)
    .bind(payload.local_loss)
    .bind(payload.local_accuracy)
    .bind(payload.training_time_ms)
    .bind(payload.dp_epsilon)
    .bind(payload.dp_delta)
    .bind(&payload.checksum)
    .fetch_one(&state.db)
    .await?;

    // Update client stats
    sqlx::query(
        r#"
        UPDATE fl_clients SET
            total_rounds_participated = total_rounds_participated + 1,
            last_participation = NOW(),
            avg_training_time_ms = (COALESCE(avg_training_time_ms, 0) * total_rounds_participated + $2) / (total_rounds_participated + 1),
            updated_at = NOW()
        WHERE id = $1
        "#,
    )
    .bind(payload.client_id)
    .bind(payload.training_time_ms)
    .execute(&state.db)
    .await?;

    // Update round participation count
    sqlx::query("UPDATE fl_rounds SET clients_participated = clients_participated + 1 WHERE id = $1")
        .bind(payload.round_id)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({ "update_id": update_id, "accepted": true })))
}

/// Start a new FL round (admin)
#[derive(Deserialize)]
pub struct StartFLRoundPayload {
    pub model_type: String,
    pub target_clients: i32,
    pub min_clients: i32,
    pub client_fraction: Option<f64>,
    pub local_epochs: Option<i32>,
    pub batch_size: Option<i32>,
    pub learning_rate: Option<f64>,
    pub aggregation_method: Option<String>,
    pub differential_privacy: Option<bool>,
    pub noise_multiplier: Option<f64>,
    pub clip_norm: Option<f64>,
}

pub async fn start_fl_round(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<StartFLRoundPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    // Get latest round number and global weights
    let latest = sqlx::query_as::<_, (i32, Option<Value>, Option<i32>)>(
        "SELECT round_number, global_weights, model_version FROM fl_rounds WHERE model_type = $1 ORDER BY round_number DESC LIMIT 1"
    )
    .bind(&payload.model_type)
    .fetch_optional(&state.db)
    .await?;

    let (next_round, global_weights, model_version) = latest
        .map(|(n, w, v)| (n + 1, w, v.unwrap_or(1)))
        .unwrap_or((1, None, 1));

    let round_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO fl_rounds (
            round_number, model_type, target_clients, min_clients, client_fraction,
            local_epochs, batch_size, learning_rate, global_weights, model_version,
            aggregation_method, differential_privacy, noise_multiplier, clip_norm,
            status, started_at, created_at
        ) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,'in_progress',NOW(),NOW())
        RETURNING id
        "#,
    )
    .bind(next_round)
    .bind(&payload.model_type)
    .bind(payload.target_clients)
    .bind(payload.min_clients)
    .bind(payload.client_fraction.unwrap_or(0.1))
    .bind(payload.local_epochs.unwrap_or(1))
    .bind(payload.batch_size.unwrap_or(32))
    .bind(payload.learning_rate.unwrap_or(0.01))
    .bind(&global_weights)
    .bind(model_version)
    .bind(payload.aggregation_method.as_deref().unwrap_or("fedavg"))
    .bind(payload.differential_privacy.unwrap_or(true))
    .bind(payload.noise_multiplier.unwrap_or(1.0))
    .bind(payload.clip_norm.unwrap_or(1.0))
    .fetch_one(&state.db)
    .await?;

    let schema = crate::ml::features::feature_schema();

    Ok(Json(json!({
        "round_id": round_id,
        "round_number": next_round,
        "status": "in_progress",
        "feature_schema": schema
    })))
}

/// Aggregate client updates (FedAvg)
pub async fn aggregate_fl_round(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<AggregateRoundPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    // Get round info
    let round = sqlx::query_as::<_, (String, i32, Option<i32>)>(
        "SELECT status, min_clients, model_version FROM fl_rounds WHERE id = $1"
    )
    .bind(payload.round_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("Round not found"))?;

    if round.0 != "in_progress" {
        return Err(AppError::bad_request("Round already aggregated or failed"));
    }

    // Get all valid updates
    #[derive(sqlx::FromRow)]
    struct ClientUpdate {
        local_weights: Value,
        num_samples: i32,
        local_loss: f64,
        local_accuracy: Option<f64>,
    }

    let updates = sqlx::query_as::<_, ClientUpdate>(
        "SELECT local_weights, num_samples, local_loss, local_accuracy FROM fl_client_updates WHERE round_id = $1 AND status = 'received'"
    )
    .bind(payload.round_id)
    .fetch_all(&state.db)
    .await?;

    if (updates.len() as i32) < round.1 {
        return Err(AppError::bad_request(format!(
            "Not enough clients: {} / {} required",
            updates.len(),
            round.1
        )));
    }

    // Real FedAvg: weighted averaging with differential privacy
    let ml = state.ml.read().await;
    let client_data: Vec<(Value, i32, f64, Option<f64>)> = updates
        .iter()
        .map(|u| (u.local_weights.clone(), u.num_samples, u.local_loss, u.local_accuracy))
        .collect();

    let aggregation = ml.federated.aggregate(&client_data)
        .map_err(|e| AppError::internal(&format!("FedAvg aggregation failed: {e}")))?;

    let avg_loss = aggregation.avg_loss;
    let avg_accuracy = aggregation.avg_accuracy;
    let total_samples = aggregation.total_samples;
    let new_version = round.2.unwrap_or(1) + 1;

    // Store aggregated weights in fl_models
    let _ = sqlx::query(
        "INSERT INTO fl_models (model_type, version, weights, created_at) VALUES ($1, $2, $3, NOW()) ON CONFLICT (model_type, version) DO UPDATE SET weights = $3"
    )
    .bind(&payload.model_type)
    .bind(new_version)
    .bind(&aggregation.aggregated_weights)
    .execute(&state.db)
    .await;

    sqlx::query(
        r#"
        UPDATE fl_rounds SET
            status = 'completed',
            completed_at = NOW(),
            avg_loss = $2,
            avg_accuracy = $3,
            model_version = $4
        WHERE id = $1
        "#,
    )
    .bind(payload.round_id)
    .bind(avg_loss)
    .bind(avg_accuracy)
    .bind(new_version)
    .execute(&state.db)
    .await?;

    // Update all client updates as accepted
    sqlx::query("UPDATE fl_client_updates SET status = 'accepted', processed_at = NOW() WHERE round_id = $1")
        .bind(payload.round_id)
        .execute(&state.db)
        .await?;

    // Log performance
    sqlx::query(
        "INSERT INTO model_performance_log (model_type, model_version, metric_name, metric_value, eval_dataset, sample_count, recorded_at) VALUES ($1, $2, 'loss', $3, 'federated', $4, NOW()), ($1, $2, 'accuracy', $5, 'federated', $4, NOW())"
    )
    .bind(&payload.model_type)
    .bind(new_version.to_string())
    .bind(avg_loss)
    .bind(total_samples)
    .bind(avg_accuracy)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "aggregated": true,
        "clients": updates.len(),
        "total_samples": total_samples,
        "avg_loss": avg_loss,
        "avg_accuracy": avg_accuracy,
        "new_model_version": new_version
    })))
}

#[derive(Deserialize)]
pub struct AggregateRoundPayload {
    pub round_id: i64,
    pub model_type: String,
}

/// Get active FL model for deployment
pub async fn get_active_fl_model(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let model_type = params.get("model_type").ok_or_else(|| AppError::bad_request("model_type required"))?;

    #[derive(sqlx::FromRow, Serialize)]
    struct FLModel {
        id: i64,
        model_type: String,
        version: i32,
        architecture: Option<Value>,
        weights: Option<Value>,
        weights_url: Option<String>,
        total_rounds: Option<i32>,
        total_samples: Option<i32>,
        validation_loss: Option<f64>,
        validation_accuracy: Option<f64>,
        deployed_at: Option<NaiveDateTime>,
    }

    let model = sqlx::query_as::<_, FLModel>(
        "SELECT id, model_type, version, architecture, weights, weights_url, total_rounds, total_samples, validation_loss, validation_accuracy, deployed_at FROM fl_models WHERE model_type = $1 AND is_active = TRUE"
    )
    .bind(model_type)
    .fetch_optional(&state.db)
    .await?;

    Ok(Json(json!({ "model": model })))
}

/// Report local data stats for FL eligibility
#[derive(Deserialize)]
pub struct ReportLocalDataPayload {
    pub data_type: String,
    pub sample_count: i32,
    pub feature_stats: Option<Value>,
    pub label_distribution: Option<Value>,
    pub quality_score: Option<f64>,
}

pub async fn report_local_data(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ReportLocalDataPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let min_samples_met = payload.sample_count >= 50; // Minimum samples for training

    sqlx::query(
        r#"
        INSERT INTO fl_local_data (user_id, data_type, sample_count, feature_stats, label_distribution, quality_score, min_samples_met, updated_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
        ON CONFLICT (user_id, data_type) DO UPDATE SET
            sample_count = $3, feature_stats = $4, label_distribution = $5, quality_score = $6, min_samples_met = $7, updated_at = NOW()
        "#,
    )
    .bind(user_id)
    .bind(&payload.data_type)
    .bind(payload.sample_count)
    .bind(&payload.feature_stats)
    .bind(&payload.label_distribution)
    .bind(payload.quality_score)
    .bind(min_samples_met)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({ "reported": true, "eligible_for_training": min_samples_met })))
}

/// Get FL training data for on-device model training.
/// Serves the user's profile features + their swipe history as labeled training
/// pairs so the FL client can train a local recommendation model using real
/// user data (age, profession, height, languages, interests, gender, swipe
/// behavior, engagement, and match outcomes).
pub async fn get_fl_training_data(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let max_pairs: i64 = params
        .get("max_pairs")
        .and_then(|v| v.parse().ok())
        .unwrap_or(200);

    let device_id = params
        .get("device_id")
        .ok_or_else(|| AppError::bad_request("device_id required"))?;

    // Verify client is registered and opted in
    let client = sqlx::query_as::<_, (i64, bool, bool)>(
        "SELECT id, is_active, opted_in FROM fl_clients WHERE user_id = $1 AND device_id = $2"
    )
    .bind(user_id)
    .bind(device_id)
    .fetch_optional(&state.db)
    .await?;

    let (_, is_active, opted_in) = client
        .ok_or_else(|| AppError::bad_request("Client not registered for FL"))?;
    if !is_active || !opted_in {
        return Err(AppError::bad_request("Client not active or not opted in"));
    }

    let training_data = crate::ml::features::FLTrainingData::build(
        &state.db,
        user_id,
        max_pairs,
    )
    .await
    .map_err(|e| AppError::internal(&format!("Failed to build FL training data: {e}")))?;

    Ok(Json(json!({
        "user_id": user_id,
        "user_features": training_data.user_features,
        "training_pairs_count": training_data.training_pairs.len(),
        "training_pairs": training_data.training_pairs,
        "feature_schema": training_data.feature_schema,
        "eligible_for_training": training_data.training_pairs.len() >= 50,
    })))
}

/// Export training data snapshot for LLM fine-tuning
#[derive(Deserialize)]
pub struct ExportTrainingDataPayload {
    pub snapshot_type: String,
    pub date_from: Option<String>,
    pub date_to: Option<String>,
    pub sample_limit: Option<i32>,
    pub anonymize: Option<bool>,
}

pub async fn export_training_snapshot(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<ExportTrainingDataPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let snapshot_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO llm_training_snapshots (snapshot_type, date_from, date_to, anonymized, pii_removed, status, created_at)
        VALUES ($1, $2::timestamp, $3::timestamp, $4, TRUE, 'pending', NOW())
        RETURNING id
        "#,
    )
    .bind(&payload.snapshot_type)
    .bind(&payload.date_from)
    .bind(&payload.date_to)
    .bind(payload.anonymize.unwrap_or(true))
    .fetch_one(&state.db)
    .await?;

    // In production, this would trigger an async job to export data
    // For now, we'll do a simple count and mark as ready

    let sample_count = match payload.snapshot_type.as_str() {
        "reel_labels" => {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM reel_llm_labels")
                .fetch_one(&state.db)
                .await?
        }
        "message_quality" => {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM message_llm_labels")
                .fetch_one(&state.db)
                .await?
        }
        "response_prediction" => {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM response_training_data")
                .fetch_one(&state.db)
                .await?
        }
        "conversation_labels" => {
            sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM llm_conversation_labels")
                .fetch_one(&state.db)
                .await?
        }
        _ => 0
    };

    sqlx::query("UPDATE llm_training_snapshots SET sample_count = $2, status = 'completed', completed_at = NOW() WHERE id = $1")
        .bind(snapshot_id)
        .bind(sample_count as i32)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({
        "snapshot_id": snapshot_id,
        "snapshot_type": payload.snapshot_type,
        "sample_count": sample_count,
        "status": "completed"
    })))
}

/// Get FL and LLM system stats (admin)
pub async fn get_ml_system_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    // LLM Labeling stats
    let labeling_stats = sqlx::query_as::<_, (String, i64)>(
        "SELECT status, COUNT(*) FROM llm_labeling_queue GROUP BY status"
    )
    .fetch_all(&state.db)
    .await?;

    let labeled_reels = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM reel_llm_labels").fetch_one(&state.db).await?;
    let labeled_messages = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM message_llm_labels").fetch_one(&state.db).await?;
    let labeled_users = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM user_llm_labels").fetch_one(&state.db).await?;

    // FL stats
    let active_clients = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM fl_clients WHERE is_active = TRUE AND opted_in = TRUE").fetch_one(&state.db).await?;
    let total_rounds = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM fl_rounds WHERE status = 'completed'").fetch_one(&state.db).await?;
    let total_updates = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM fl_client_updates WHERE status = 'accepted'").fetch_one(&state.db).await?;

    // Active FL rounds
    let active_rounds = sqlx::query_as::<_, (i64, String, i32, i32)>(
        "SELECT id, model_type, round_number, clients_participated FROM fl_rounds WHERE status = 'in_progress'"
    )
    .fetch_all(&state.db)
    .await?;

    // ML computation stats (RL, LinUCB)
    let ml_computation = {
        let ml = state.ml.read().await;
        ml.ml_stats()
    };

    Ok(Json(json!({
        "llm_labeling": {
            "queue_stats": labeling_stats.into_iter().collect::<HashMap<_, _>>(),
            "labeled_reels": labeled_reels,
            "labeled_messages": labeled_messages,
            "labeled_users": labeled_users
        },
        "federated_learning": {
            "active_clients": active_clients,
            "total_completed_rounds": total_rounds,
            "total_client_updates": total_updates,
            "active_rounds": active_rounds.iter().map(|(id, mt, rn, cp)| json!({
                "round_id": id,
                "model_type": mt,
                "round_number": rn,
                "clients_participated": cp
            })).collect::<Vec<_>>()
        },
        "ml_computation": ml_computation
    })))
}

// ============================================================================
// University Discovery System
// ============================================================================

/// University row from database
#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct UniversityRow {
    pub id: i64,
    pub name: String,
    pub short_name: Option<String>,
    pub domain: String,
    pub country: String,
    pub country_code: String,
    pub state_province: Option<String>,
    pub city: Option<String>,
    pub tier: Option<String>,
}

/// Search universities by name/short_name
#[derive(Debug, Deserialize)]
pub struct UniversitySearchQuery {
    pub q: String,
    pub country: Option<String>,
    pub limit: Option<i32>,
}

pub async fn search_universities(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<UniversitySearchQuery>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _user_id = decode_access_token(&token, &state.config.secret_key)?;
    // Auth required but no student verification needed — university search is used during onboarding

    let q = params.q.trim().to_lowercase();
    let prefix = format!("{}%", q);
    let contains = format!("%{}%", q);
    let limit = params.limit.unwrap_or(30).min(100);

    // Normalize ISO alpha-2 codes (from iOS CLPlacemark.isoCountryCode) to
    // alpha-3 codes as stored in the DB. Also uppercases everything.
    let country: Option<String> = params.country.as_deref().map(|c| {
        match c.to_uppercase().as_str() {
            "US"       => "USA".into(),
            "GB" | "UK"=> "GBR".into(),
            "CA"       => "CAN".into(),
            "AU"       => "AUS".into(),
            "IN"       => "IND".into(),
            "DE"       => "DEU".into(),
            "SG"       => "SGP".into(),
            "JP"       => "JPN".into(),
            "FR"       => "FRA".into(),
            "NZ"       => "NZL".into(),
            "AE"       => "ARE".into(),
            "NL"       => "NLD".into(),
            other      => other.to_string(),  // already alpha-3 or unknown — pass through
        }
    });

    // Empty query: return top universities by tier (filtered by country if provided)
    if q.is_empty() {
        let universities = if let Some(country) = &country {
            sqlx::query_as::<_, UniversityRow>(
                r#"
                SELECT id, name, short_name, domain, country, country_code, state_province, city, tier
                FROM universities
                WHERE is_active = TRUE AND country_code = $1
                ORDER BY CASE tier WHEN 'tier1' THEN 1 WHEN 'tier2' THEN 2 ELSE 3 END, name ASC
                LIMIT $2
                "#
            )
            .bind(country)
            .bind(limit)
            .fetch_all(&state.db)
            .await?
        } else {
            sqlx::query_as::<_, UniversityRow>(
                r#"
                SELECT id, name, short_name, domain, country, country_code, state_province, city, tier
                FROM universities
                WHERE is_active = TRUE
                ORDER BY CASE tier WHEN 'tier1' THEN 1 WHEN 'tier2' THEN 2 ELSE 3 END, name ASC
                LIMIT $1
                "#
            )
            .bind(limit)
            .fetch_all(&state.db)
            .await?
        };
        let result: Vec<Value> = universities.into_iter().map(|u| serde_json::json!({
            "id": u.id, "name": u.name, "short_name": u.short_name,
            "domain": u.domain, "country": u.country, "country_code": u.country_code,
            "state_province": u.state_province, "city": u.city, "tier": u.tier
        })).collect();
        return Ok(Json(serde_json::json!({ "universities": result, "total": result.len() })));
    }

    // tsquery: prefix match on each word using :* for partial FTS
    let tsquery = q.split_whitespace()
        .map(|w| format!("{}:*", w))
        .collect::<Vec<_>>()
        .join(" & ");

    // Hybrid search: trigram similarity (O(log n) via GIN) + FTS ts_rank + exact/prefix boost
    let universities = if let Some(country) = &country {
        sqlx::query_as::<_, UniversityRow>(
            r#"
            SELECT id, name, short_name, domain, country, country_code, state_province, city, tier
            FROM universities
            WHERE is_active = TRUE
              AND country_code = $1
              AND (
                LOWER(name) LIKE $3
                OR LOWER(short_name) LIKE $3
                OR LOWER(city) LIKE $3
                OR LOWER(state_province) LIKE $3
                OR (search_vector IS NOT NULL AND search_vector @@ to_tsquery('simple', $5))
                OR similarity(LOWER(name), $2) > 0.15
                OR similarity(LOWER(short_name), $2) > 0.2
              )
            ORDER BY
              CASE
                WHEN LOWER(short_name) = $2        THEN 1000
                WHEN LOWER(name) = $2              THEN 900
                WHEN LOWER(short_name) LIKE $4     THEN 800
                WHEN LOWER(name) LIKE $4           THEN 700
                WHEN LOWER(short_name) LIKE $3     THEN 600
                WHEN LOWER(name) LIKE $3           THEN 500
                WHEN LOWER(city) LIKE $3           THEN 300
                WHEN LOWER(state_province) LIKE $3 THEN 200
                ELSE 0
              END
              + CASE WHEN search_vector IS NOT NULL
                  THEN COALESCE(ts_rank(search_vector, to_tsquery('simple', $5))::numeric * 100, 0)
                  ELSE 0 END
              + similarity(LOWER(name), $2) * 80
              + CASE tier WHEN 'tier1' THEN 50 WHEN 'tier2' THEN 30 ELSE 10 END
              DESC,
              name ASC
            LIMIT $6
            "#
        )
        .bind(country)
        .bind(&q)
        .bind(&contains)
        .bind(&prefix)
        .bind(&tsquery)
        .bind(limit)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, UniversityRow>(
            r#"
            SELECT id, name, short_name, domain, country, country_code, state_province, city, tier
            FROM universities
            WHERE is_active = TRUE
              AND (
                LOWER(name) LIKE $2
                OR LOWER(short_name) LIKE $2
                OR LOWER(city) LIKE $2
                OR LOWER(state_province) LIKE $2
                OR (search_vector IS NOT NULL AND search_vector @@ to_tsquery('simple', $4))
                OR similarity(LOWER(name), $1) > 0.15
                OR similarity(LOWER(short_name), $1) > 0.2
              )
            ORDER BY
              CASE
                WHEN LOWER(short_name) = $1        THEN 1000
                WHEN LOWER(name) = $1              THEN 900
                WHEN LOWER(short_name) LIKE $3     THEN 800
                WHEN LOWER(name) LIKE $3           THEN 700
                WHEN LOWER(short_name) LIKE $2     THEN 600
                WHEN LOWER(name) LIKE $2           THEN 500
                WHEN LOWER(city) LIKE $2           THEN 300
                WHEN LOWER(state_province) LIKE $2 THEN 200
                ELSE 0
              END
              + CASE WHEN search_vector IS NOT NULL
                  THEN COALESCE(ts_rank(search_vector, to_tsquery('simple', $4))::numeric * 100, 0)
                  ELSE 0 END
              + similarity(LOWER(name), $1) * 80
              + CASE tier WHEN 'tier1' THEN 50 WHEN 'tier2' THEN 30 ELSE 10 END
              DESC,
              name ASC
            LIMIT $5
            "#
        )
        .bind(&q)
        .bind(&contains)
        .bind(&prefix)
        .bind(&tsquery)
        .bind(limit)
        .fetch_all(&state.db)
        .await?
    };

    // Get student count per university
    let university_ids: Vec<i64> = universities.iter().map(|u| u.id).collect();
    let student_counts: HashMap<i64, i64> = if !university_ids.is_empty() {
        sqlx::query_as::<_, (i64, i64)>(
            r#"
            SELECT sv.university_id, COUNT(*) as count
            FROM student_verifications sv
            WHERE sv.university_id = ANY($1) AND sv.status = 'approved'
            GROUP BY sv.university_id
            "#
        )
        .bind(&university_ids)
        .fetch_all(&state.db)
        .await?
        .into_iter()
        .collect()
    } else {
        HashMap::new()
    };

    let results: Vec<Value> = universities.iter().map(|u| {
        json!({
            "id": u.id,
            "name": u.name,
            "short_name": u.short_name,
            "domain": u.domain,
            "country": u.country,
            "country_code": u.country_code,
            "state_province": u.state_province,
            "city": u.city,
            "tier": u.tier,
            "student_count": student_counts.get(&u.id).unwrap_or(&0)
        })
    }).collect();

    Ok(Json(json!({
        "universities": results,
        "count": results.len()
    })))
}

/// Get list of countries with universities
pub async fn get_university_countries(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let countries = sqlx::query_as::<_, (String, String, i64)>(
        r#"
        SELECT country, country_code, COUNT(*) as university_count
        FROM universities
        WHERE is_active = TRUE
        GROUP BY country, country_code
        ORDER BY university_count DESC
        "#
    )
    .fetch_all(&state.db)
    .await?;

    let results: Vec<Value> = countries.iter().map(|(name, code, count)| {
        json!({
            "country": name,
            "country_code": code,
            "university_count": count
        })
    }).collect();

    Ok(Json(json!({ "countries": results })))
}

/// Query params for university discovery
#[derive(Debug, Deserialize)]
pub struct UniversityDiscoverQuery {
    pub university_id: i64,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

/// Check if user has access to discover from a university
async fn check_university_access(
    db: &sqlx::PgPool,
    user_id: i32,
    target_university_id: i64,
) -> Result<(bool, String), AppError> {
    // Get user's own university
    let user_uni = sqlx::query_as::<_, (Option<i64>, Option<String>)>(
        r#"
        SELECT university_id, university_country_code
        FROM student_verifications
        WHERE user_id = $1 AND status = 'approved'
        "#
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?;

    let (user_university_id, user_country) = match user_uni {
        Some((uid, cc)) => (uid, cc),
        None => return Ok((false, "Student verification required".to_string())),
    };

    // Same university - always allowed
    if user_university_id == Some(target_university_id) {
        return Ok((true, "own_university".to_string()));
    }

    // Get target university's country
    let target_country = sqlx::query_scalar::<_, String>(
        "SELECT country_code FROM universities WHERE id = $1"
    )
    .bind(target_university_id)
    .fetch_optional(db)
    .await?;

    let target_country = match target_country {
        Some(c) => c,
        None => return Ok((false, "University not found".to_string())),
    };

    // Check for active passes
    let active_pass = sqlx::query_as::<_, (String, Option<String>)>(
        r#"
        SELECT pass_type, country_code
        FROM university_passes
        WHERE user_id = $1
          AND status = 'active'
          AND (end_date IS NULL OR end_date > NOW())
        ORDER BY
          CASE pass_type WHEN 'global' THEN 1 WHEN 'country' THEN 2 END
        LIMIT 1
        "#
    )
    .bind(user_id)
    .fetch_optional(db)
    .await?;

    if let Some((pass_type, pass_country)) = active_pass {
        match pass_type.as_str() {
            "global" => return Ok((true, "global_pass".to_string())),
            "country" => {
                if pass_country.as_deref() == Some(&target_country) {
                    return Ok((true, "country_pass".to_string()));
                }
                // Check if same country as user
                if user_country.as_deref() == Some(&target_country) {
                    return Ok((true, "same_country".to_string()));
                }
            }
            _ => {}
        }
    }

    // Same country without pass - allowed for free
    if user_country.as_deref() == Some(&target_country) {
        return Ok((true, "same_country_free".to_string()));
    }

    Ok((false, format!("Pass required for {} universities", target_country)))
}

/// Discover profiles from a specific university
pub async fn discover_university_profiles(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<UniversityDiscoverQuery>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Check access (read-replica safe)
    let read_db = state.read_pool();
    let (has_access, access_type) = check_university_access(read_db, user_id, params.university_id).await?;
    if !has_access {
        return Err(AppError::forbidden(&access_type));
    }

    let limit = params.limit.unwrap_or(20).min(50);
    let offset = params.offset.unwrap_or(0);

    // Get university info
    let university = sqlx::query_as::<_, UniversityRow>(
        "SELECT id, name, short_name, domain, country, country_code, state_province, city, tier FROM universities WHERE id = $1"
    )
    .bind(params.university_id)
    .fetch_optional(read_db)
    .await?
    .ok_or_else(|| AppError::not_found("University not found"))?;

    // Get verified students from this university
    let profiles = sqlx::query_as::<_, DiscoverUserRow>(
        r#"
        SELECT u.id, u.name, u.display_name, u.show_verified_name,
               u.dob, u.gender, u.bio, u.profile_photo_url, u.profile_photos,
               u.profile_photo_1, u.profile_photo_2, u.profile_photo_3,
               u.is_verified, u.attractiveness_score, u.looking_for, u.profession_title,
               u.height_cm, l.city, l.latitude, l.longitude
        FROM users u
        INNER JOIN student_verifications sv ON sv.user_id = u.id
        LEFT JOIN user_locations l ON l.user_id = u.id
        WHERE sv.university_id = $1
          AND sv.status = 'approved'
          AND u.id != $2
          AND u.is_active = TRUE
          AND u.is_profile_complete = TRUE
          AND NOT EXISTS (
              SELECT 1 FROM matches m
              WHERE ((m.user1_id = $2 AND m.user2_id = u.id) OR (m.user1_id = u.id AND m.user2_id = $2))
              AND (m.user1_liked = TRUE OR m.user2_liked = TRUE)
          )
        ORDER BY u.attractiveness_score DESC NULLS LAST, u.created_at DESC
        LIMIT $3 OFFSET $4
        "#
    )
    .bind(params.university_id)
    .bind(user_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(read_db)
    .await?;

    let results: Vec<DiscoverProfile> = profiles.iter().map(|row| {
        let mut photos = Vec::new();
        if let Some(url) = &row.profile_photo_url { photos.push(url.clone()); }
        if let Some(url) = &row.profile_photo_1 { photos.push(url.clone()); }
        if let Some(url) = &row.profile_photo_2 { photos.push(url.clone()); }
        if let Some(url) = &row.profile_photo_3 { photos.push(url.clone()); }

        let age = row.dob.map(|dob| {
            let today = chrono::Utc::now().date_naive();
            today.years_since(dob).unwrap_or(0) as i32
        });

        let public_name = public_name_for_viewer(
            row.name.as_deref(), row.display_name.as_deref(), row.show_verified_name,
        );
        DiscoverProfile {
            id: row.id,
            name: public_name,
            display_name: row.display_name.clone(),
            age,
            gender: row.gender.clone(),
            bio: row.bio.clone(),
            photos,
            is_verified: row.is_verified.unwrap_or(false),
            looking_for: row.looking_for.clone(),
            profession_title: row.profession_title.clone(),
            height_cm: row.height_cm,
            distance_km: None,
            distance_text: None,
            city: row.city.clone(),
            compatibility_score: row.attractiveness_score,
            university: Some(university.name.clone()),
            university_tier: university.tier.as_ref().map(|t| format_tier(t)),
            interaction_status: None,
            super_liked_you: None,
        }
    }).collect();

    Ok(Json(json!({
        "university": {
            "id": university.id,
            "name": university.name,
            "short_name": university.short_name,
            "country": university.country
        },
        "access_type": access_type,
        "profiles": results,
        "count": results.len(),
        "offset": offset,
        "limit": limit
    })))
}

/// Get reels from a specific university
#[derive(Debug, Deserialize)]
pub struct UniversityReelsQuery {
    pub university_id: i64,
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

pub async fn get_university_reels(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<UniversityReelsQuery>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Check access
    let (has_access, access_type) = check_university_access(&state.db, user_id, params.university_id).await?;
    if !has_access {
        return Err(AppError::forbidden(&access_type));
    }

    let limit = params.limit.unwrap_or(20).min(50);
    let offset = params.offset.unwrap_or(0);

    // Get university info
    let university = sqlx::query_as::<_, (i64, String, Option<String>, String)>(
        "SELECT id, name, short_name, country FROM universities WHERE id = $1"
    )
    .bind(params.university_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("University not found"))?;

    // Get reels from verified students at this university
    let reels = sqlx::query_as::<_, (i64, i64, Option<String>, Option<String>, Option<Value>, Option<NaiveDateTime>, Option<String>, Option<String>, Option<bool>)>(
        r#"
        SELECT s.id, s.user_id, s.title, s.poster_url, s.renditions, s.created_at,
               u.name as user_name, u.profile_photo_url, u.is_verified
        FROM spots s
        INNER JOIN users u ON u.id = s.user_id
        INNER JOIN student_verifications sv ON sv.user_id = s.user_id
        WHERE sv.university_id = $1
          AND sv.status = 'approved'
          AND s.user_id != $2
          AND (s.expires_at IS NULL OR s.expires_at > NOW())
        ORDER BY s.created_at DESC
        LIMIT $3 OFFSET $4
        "#
    )
    .bind(params.university_id)
    .bind(user_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(&state.db)
    .await?;

    let results: Vec<Value> = reels.iter().map(|(id, uid, title, poster, renditions, created, name, photo, verified)| {
        json!({
            "id": id,
            "user_id": uid,
            "title": title,
            "poster_url": poster,
            "renditions": renditions,
            "created_at": created,
            "user": {
                "name": name,
                "profile_photo_url": photo,
                "is_verified": verified.unwrap_or(false)
            }
        })
    }).collect();

    Ok(Json(json!({
        "university": {
            "id": university.0,
            "name": university.1,
            "short_name": university.2,
            "country": university.3
        },
        "access_type": access_type,
        "reels": results,
        "count": results.len(),
        "offset": offset,
        "limit": limit
    })))
}

/// GET /reels/user/:user_id — fetch a user's public reels (paginated)
/// Used by ProfileView "My Reels" tab and UserReelsView.
pub async fn get_user_reels(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(target_user_id): AxumPath<i64>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _viewer_id = decode_access_token(&token, &state.config.secret_key)?;

    let limit: i64 = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(30).min(50);
    let offset: i64 = params.get("offset").and_then(|v| v.parse().ok()).unwrap_or(0);

    #[derive(sqlx::FromRow)]
    struct UserReel {
        id: i64,
        video_url: String,
        thumbnail_url: Option<String>,
        caption: Option<String>,
        view_count: Option<i32>,
        like_count: Option<i32>,
        duration_sec: Option<i32>,
        created_at: Option<NaiveDateTime>,
        tags: Option<Value>,
        category: Option<String>,
        hls_url: Option<String>,
        hls_state: Option<String>,
        music_id: Option<String>,
        music_title: Option<String>,
        music_artist: Option<String>,
        music_artwork_url: Option<String>,
        music_preview_url: Option<String>,
        music_start_ms: Option<i32>,
    }

    let reels = sqlx::query_as::<_, UserReel>(
        r#"
        SELECT r.id, r.video_url, r.thumbnail_url, r.caption,
               r.view_count, r.like_count, r.duration_sec, r.created_at,
               r.tags, r.category, r.hls_url, r.hls_state,
               r.music_id, r.music_title, r.music_artist, r.music_artwork_url,
               r.music_preview_url, r.music_start_ms
        FROM reels r
        WHERE r.user_id = $1
          AND r.is_active = TRUE
        ORDER BY r.created_at DESC
        LIMIT $2 OFFSET $3
        "#,
    )
    .bind(target_user_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(state.read_pool())
    .await?;

    let total: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM reels WHERE user_id = $1 AND is_active = TRUE"
    )
    .bind(target_user_id)
    .fetch_one(state.read_pool())
    .await
    .unwrap_or(0);

    let items: Vec<Value> = reels.iter().map(|r| {
        json!({
            "id": r.id,
            "video_url": r.video_url,
            "hls_url": r.hls_url,
            "hls_state": r.hls_state,
            "thumbnail_url": r.thumbnail_url,
            "caption": r.caption,
            "view_count": r.view_count.unwrap_or(0),
            "like_count": r.like_count.unwrap_or(0),
            "duration_sec": r.duration_sec,
            "created_at": r.created_at,
            "tags": r.tags,
            "category": r.category,
            "music": if r.music_id.is_some() { Some(json!({
                "id": r.music_id,
                "title": r.music_title,
                "artist": r.music_artist,
                "artwork_url": r.music_artwork_url,
                "preview_url": r.music_preview_url,
                "start_ms": r.music_start_ms
            })) } else { None },
        })
    }).collect();

    Ok(Json(json!({
        "reels": items,
        "total": total,
        "count": items.len(),
        "offset": offset,
        "limit": limit,
        "has_more": offset + limit < total,
    })))
}

/// Purchase university pass request
#[derive(Debug, Deserialize)]
pub struct PurchaseUniversityPassRequest {
    pub pass_type: String,           // "country" or "global"
    pub country_code: Option<String>, // Required for country pass
    pub duration_days: Option<i32>,   // NULL for lifetime
    pub payment_id: Option<String>,
    /// Client-generated idempotency key to prevent duplicate purchases
    pub idempotency_key: Option<String>,
}

pub async fn purchase_university_pass(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<PurchaseUniversityPassRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Verify user is a verified student
    let is_verified = sqlx::query_scalar::<_, bool>(
        "SELECT COALESCE(is_student_verified, FALSE) FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    if !is_verified {
        return Err(AppError::forbidden("Student verification required to purchase passes"));
    }

    // Validate pass type
    if payload.pass_type != "country" && payload.pass_type != "global" {
        return Err(AppError::bad_request("Invalid pass type. Must be 'country' or 'global'"));
    }

    // Country pass requires country_code
    if payload.pass_type == "country" && payload.country_code.is_none() {
        return Err(AppError::bad_request("country_code required for country pass"));
    }

    // Idempotency check: If idempotency_key provided, check if already processed
    if let Some(ref idempotency_key) = payload.idempotency_key {
        let existing: Option<(i64, String)> = sqlx::query_as(
            r#"
            SELECT id, status
            FROM university_passes
            WHERE user_id = $1 AND idempotency_key = $2
            LIMIT 1
            "#,
        )
        .bind(user_id)
        .bind(idempotency_key)
        .fetch_optional(&state.db)
        .await?;

        if let Some((pass_id, status)) = existing {
            // Return the existing pass info (idempotent response)
            return Ok(Json(json!({
                "message": "Pass already purchased (idempotent)",
                "pass": {
                    "id": pass_id,
                    "type": payload.pass_type,
                    "status": status,
                },
                "idempotent": true,
            })));
        }
    }

    // Calculate pricing
    let (price, currency) = match payload.pass_type.as_str() {
        "country" => {
            match payload.duration_days {
                Some(7) => (4.99, "USD"),
                Some(30) => (14.99, "USD"),
                Some(90) => (34.99, "USD"),
                _ => (14.99, "USD"), // Default 30 days
            }
        }
        "global" => {
            match payload.duration_days {
                Some(7) => (9.99, "USD"),
                Some(30) => (29.99, "USD"),
                Some(90) => (69.99, "USD"),
                Some(365) => (199.99, "USD"),
                _ => (29.99, "USD"), // Default 30 days
            }
        }
        _ => return Err(AppError::bad_request("Invalid pass type")),
    };

    let duration = payload.duration_days.unwrap_or(30);
    let end_date = Utc::now().naive_utc() + chrono::Duration::days(duration as i64);

    // Check for existing active pass of same type
    let existing = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT id FROM university_passes
        WHERE user_id = $1 AND pass_type = $2 AND status = 'active'
          AND (end_date IS NULL OR end_date > NOW())
          AND ($3::VARCHAR IS NULL OR country_code = $3)
        "#
    )
    .bind(user_id)
    .bind(&payload.pass_type)
    .bind(&payload.country_code)
    .fetch_optional(&state.db)
    .await?;

    if existing.is_some() {
        return Err(AppError::bad_request("Active pass already exists"));
    }

    // Create pass
    let pass_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO university_passes (user_id, pass_type, country_code, status, start_date, end_date, amount_paid, payment_id, idempotency_key)
        VALUES ($1, $2, $3, 'active', NOW(), $4, $5, $6, $7)
        RETURNING id
        "#
    )
    .bind(user_id)
    .bind(&payload.pass_type)
    .bind(&payload.country_code)
    .bind(end_date)
    .bind(price)
    .bind(&payload.payment_id)
    .bind(&payload.idempotency_key)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(json!({
        "message": "Pass purchased successfully",
        "pass": {
            "id": pass_id,
            "type": payload.pass_type,
            "country_code": payload.country_code,
            "duration_days": duration,
            "end_date": end_date,
            "price": price,
            "currency": currency
        }
    })))
}

/// Get user's active passes
pub async fn get_my_university_passes(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Get user's university info
    let user_university = sqlx::query_as::<_, (Option<i64>, Option<String>, Option<String>)>(
        r#"
        SELECT sv.university_id, u.name as university_name, sv.university_country_code
        FROM student_verifications sv
        LEFT JOIN universities u ON u.id = sv.university_id
        WHERE sv.user_id = $1 AND sv.status = 'approved'
        "#
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    let (uni_id, uni_name, user_country) = user_university.unwrap_or((None, None, None));

    // Get active passes
    let passes = sqlx::query_as::<_, (i64, String, Option<String>, String, NaiveDateTime, Option<NaiveDateTime>, Option<rust_decimal::Decimal>)>(
        r#"
        SELECT id, pass_type, country_code, status, start_date, end_date, amount_paid
        FROM university_passes
        WHERE user_id = $1 AND status = 'active' AND (end_date IS NULL OR end_date > NOW())
        ORDER BY created_at DESC
        "#
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    let pass_list: Vec<Value> = passes.iter().map(|(id, ptype, country, status, start, end, amount)| {
        json!({
            "id": id,
            "pass_type": ptype,
            "country_code": country,
            "status": status,
            "start_date": start,
            "end_date": end,
            "amount_paid": amount
        })
    }).collect();

    // Determine access level
    let access_level = if passes.iter().any(|(_, t, _, _, _, _, _)| t == "global") {
        "global"
    } else if passes.iter().any(|(_, t, _, _, _, _, _)| t == "country") {
        "country"
    } else if uni_id.is_some() {
        "own_university"
    } else {
        "none"
    };

    Ok(Json(json!({
        "user_university": {
            "id": uni_id,
            "name": uni_name,
            "country_code": user_country
        },
        "access_level": access_level,
        "passes": pass_list,
        "pricing": {
            "country": {
                "7_days": 4.99,
                "30_days": 14.99,
                "90_days": 34.99
            },
            "global": {
                "7_days": 9.99,
                "30_days": 29.99,
                "90_days": 69.99,
                "365_days": 199.99
            }
        }
    })))
}

// ============================================================================
// ML Computation Endpoints
// ============================================================================

/// POST /ml/rl/rank — Rank candidates using RL agent
#[derive(Deserialize)]
pub struct MlRankRequest {
    pub candidate_ids: Vec<i32>,
}

pub async fn ml_rank_candidates(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<MlRankRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let mut ml = state.ml.write().await;
    let ranked = ml.rank_candidates(&state.db, user_id, &payload.candidate_ids).await;

    Ok(Json(json!({
        "ranked": ranked.iter().map(|(id, score)| json!({
            "candidate_id": id,
            "score": score
        })).collect::<Vec<_>>()
    })))
}

/// POST /ml/linucb/score — Score candidates using LinUCB bandit
#[derive(Deserialize)]
pub struct LinucbScoreRequest {
    pub arm_id: String,
    pub context: Vec<f64>,
}

pub async fn ml_linucb_score(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LinucbScoreRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _ = decode_access_token(&token, &state.config.secret_key)?;

    let ml = state.ml.read().await;
    let score = ml.linucb.score(&payload.arm_id, &payload.context);

    Ok(Json(json!({
        "arm_id": payload.arm_id,
        "ucb_score": score
    })))
}

// ============================================================================
// Student Global Search — LinkedIn-style student discovery
// ============================================================================

/// GET /search/students?q=...&university=...&city=...&country=...&gender=...&limit=...&offset=...
#[derive(Debug, Deserialize)]
pub struct StudentSearchQuery {
    pub q: Option<String>,              // Free-text search (name)
    pub university: Option<String>,      // University name or short_name
    pub university_id: Option<i64>,      // Direct university ID
    pub city: Option<String>,            // City filter
    pub country: Option<String>,         // Country code filter
    pub gender: Option<String>,          // Gender filter
    pub min_age: Option<i32>,
    pub max_age: Option<i32>,
    pub tier: Option<String>,            // University tier: top_private, top_public, regular
    pub class_year: Option<i32>,         // Graduation/class year filter (e.g. 2025)
    pub is_alumni: Option<bool>,         // Alumni-only filter
    pub new_in_city: Option<bool>,       // "New in town" filter (arrived within 60 days)
    pub limit: Option<i32>,
    pub offset: Option<i32>,
}

/// Search result matching frontend StudentResult spec
#[derive(Debug, Serialize)]
pub struct StudentSearchResult {
    pub id: String,
    /// Verified legal name (users.name). This is the search key; always returned.
    pub name: Option<String>,
    /// Mutable UI alias (users.display_name) — only present when the user has
    /// opted in via show_display_name_in_search. Clients should show this
    /// as a secondary line ("aka ...") not replace `name`.
    pub display_name_alias: Option<String>,
    pub age: Option<i32>,
    pub photos: Vec<String>,
    pub university: Option<String>,
    pub university_tier: Option<String>,
    pub study: Option<String>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub distance: Option<f64>,
    pub bio: Option<String>,
    pub is_verified: bool,
    pub can_message: bool,
    pub interaction_status: String,  // "none", "liked", "matched"
    pub class_year: Option<i32>,
    pub is_alumni: bool,
    pub is_new_in_city: bool,
}

#[derive(Debug, sqlx::FromRow)]
struct StudentSearchRow {
    id: i32,
    name: Option<String>,
    display_name: Option<String>,
    show_display_name_in_search: Option<bool>,
    show_verified_name: Option<bool>,
    dob: Option<NaiveDate>,
    gender: Option<String>,
    bio: Option<String>,
    profile_photo_url: Option<String>,
    profile_photos: Option<Value>,
    profile_photo_1: Option<String>,
    profile_photo_2: Option<String>,
    profile_photo_3: Option<String>,
    is_verified: Option<bool>,
    attractiveness_score: Option<f64>,
    looking_for: Option<String>,
    profession_title: Option<String>,
    height_cm: Option<i32>,
    city: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
    university_name: Option<String>,
    university_short_name: Option<String>,
    university_tier: Option<String>,
    university_country: Option<String>,
    graduation_year: Option<i32>,
    is_alumni: Option<bool>,
    is_new_in_city: Option<bool>,
}

// =============================================================================
// PUT /profile/city-arrival
// Sets city_arrival_date and current_city; auto-sets is_new_in_city if within 60 days
// =============================================================================
#[derive(Debug, Deserialize)]
pub struct CityArrivalPayload {
    pub city: String,
    pub arrival_date: String, // ISO date: "2026-01-15"
}

pub async fn set_city_arrival(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CityArrivalPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let arrival = chrono::NaiveDate::parse_from_str(&payload.arrival_date, "%Y-%m-%d")
        .map_err(|_| AppError::bad_request("Invalid date format, use YYYY-MM-DD"))?;

    let today = chrono::Utc::now().date_naive();
    let days_since = (today - arrival).num_days();
    let is_new = days_since >= 0 && days_since <= 60;

    sqlx::query(
        "UPDATE users SET current_city = $1, city_arrival_date = $2, is_new_in_city = $3, updated_at = NOW() WHERE id = $4"
    )
    .bind(&payload.city)
    .bind(arrival)
    .bind(is_new)
    .bind(user_id)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "updated": true,
        "city": payload.city,
        "arrival_date": payload.arrival_date,
        "is_new_in_city": is_new,
        "days_since_arrival": days_since
    })))
}

pub async fn search_students(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<StudentSearchQuery>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Must be a verified student to search
    let is_student = sqlx::query_scalar::<_, bool>(
        "SELECT COALESCE(is_student_verified, FALSE) FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    if !is_student {
        return Err(AppError::forbidden("Student verification required to search students"));
    }

    // Check premium status for messaging capability
    let active_pass = get_active_pass(&state.db, user_id).await?;
    let is_premium = active_pass.is_some();

    let limit = params.limit.unwrap_or(20).min(50);
    let offset = params.offset.unwrap_or(0);

    // Get searcher's location for distance calc
    let my_location = sqlx::query_as::<_, (Option<f64>, Option<f64>)>(
        "SELECT latitude, longitude FROM user_locations WHERE user_id = $1"
    )
    .bind(user_id)
    .fetch_optional(state.read_pool())
    .await?;
    let (my_lat, my_lon) = my_location.unwrap_or((None, None));

    // Build dynamic query
    let mut conditions = vec![
        "sv.status = 'approved'".to_string(),
        "u.is_active = TRUE".to_string(),
        "u.is_profile_complete = TRUE".to_string(),
        format!("u.id != {}", user_id),
    ];
    let mut bind_idx = 0u32;
    let mut bind_values: Vec<String> = Vec::new();

    // Free-text name search.
    // Always matches against users.name (verified legal name).
    // ALSO matches against users.display_name when that user has opted in
    // via show_display_name_in_search = TRUE. Single bind, one LOWER(q)
    // value — Postgres evaluates the OR branch short-circuit.
    if let Some(ref q) = params.q {
        if !q.trim().is_empty() {
            bind_idx += 1;
            conditions.push(format!(
                "(LOWER(u.name) LIKE ${idx} \
                  OR (COALESCE(u.show_display_name_in_search, FALSE) = TRUE \
                      AND LOWER(u.display_name) LIKE ${idx}))",
                idx = bind_idx
            ));
            bind_values.push(format!("%{}%", q.trim().to_lowercase()));
        }
    }

    // University name search
    if let Some(ref uni) = params.university {
        if !uni.trim().is_empty() {
            bind_idx += 1;
            conditions.push(format!(
                "(LOWER(univ.name) LIKE ${idx} OR LOWER(univ.short_name) LIKE ${idx})",
                idx = bind_idx
            ));
            bind_values.push(format!("%{}%", uni.trim().to_lowercase()));
        }
    }

    // Direct university ID
    if let Some(uni_id) = params.university_id {
        conditions.push(format!("sv.university_id = {}", uni_id));
    }

    // City filter
    if let Some(ref city) = params.city {
        if !city.trim().is_empty() {
            bind_idx += 1;
            conditions.push(format!("LOWER(l.city) LIKE ${}", bind_idx));
            bind_values.push(format!("%{}%", city.trim().to_lowercase()));
        }
    }

    // Country filter
    if let Some(ref country) = params.country {
        if !country.trim().is_empty() {
            bind_idx += 1;
            conditions.push(format!("univ.country_code = ${}", bind_idx));
            bind_values.push(country.trim().to_uppercase());
        }
    }

    // Gender filter
    if let Some(ref gender) = params.gender {
        if !gender.trim().is_empty() {
            bind_idx += 1;
            conditions.push(format!("u.gender = ${}", bind_idx));
            bind_values.push(gender.clone());
        }
    }

    // Age filters
    if let Some(min_age) = params.min_age {
        let max_dob = chrono::Utc::now().date_naive() - chrono::Duration::days(min_age as i64 * 365);
        conditions.push(format!("u.dob <= '{}'", max_dob));
    }
    if let Some(max_age) = params.max_age {
        let min_dob = chrono::Utc::now().date_naive() - chrono::Duration::days((max_age as i64 + 1) * 365);
        conditions.push(format!("u.dob >= '{}'", min_dob));
    }

    // University tier filter
    if let Some(ref tier) = params.tier {
        if !tier.trim().is_empty() {
            bind_idx += 1;
            conditions.push(format!("univ.tier = ${}", bind_idx));
            bind_values.push(tier.clone());
        }
    }

    // Class year filter (graduation year)
    if let Some(cy) = params.class_year {
        conditions.push(format!("sv.graduation_year = {}", cy));
    }

    // Alumni-only filter
    if let Some(alumni) = params.is_alumni {
        conditions.push(format!("sv.is_alumni = {}", alumni));
    }

    // New-in-city filter: arrived within last 60 days
    if params.new_in_city == Some(true) {
        conditions.push("u.is_new_in_city = TRUE".to_string());
        conditions.push(format!(
            "u.city_arrival_date >= '{}'",
            (chrono::Utc::now().date_naive() - chrono::Duration::days(60))
        ));
    }

    let where_clause = conditions.join(" AND ");

    let query_str = format!(
        r#"
        SELECT u.id, u.name, u.dob, u.gender, u.bio,
               u.profile_photo_url, u.profile_photos,
               u.profile_photo_1, u.profile_photo_2, u.profile_photo_3,
               u.is_verified, u.attractiveness_score, u.looking_for,
               u.profession_title, u.height_cm,
               l.city, l.latitude, l.longitude,
               univ.name AS university_name,
               univ.short_name AS university_short_name,
               univ.tier AS university_tier,
               univ.country AS university_country,
               sv.graduation_year,
               COALESCE(sv.is_alumni, FALSE) AS is_alumni,
               COALESCE(u.is_new_in_city, FALSE) AS is_new_in_city
        FROM users u
        INNER JOIN student_verifications sv ON sv.user_id = u.id
        INNER JOIN universities univ ON univ.id = sv.university_id
        LEFT JOIN user_locations l ON l.user_id = u.id
        WHERE {}
        ORDER BY u.attractiveness_score DESC NULLS LAST, u.created_at DESC
        LIMIT {} OFFSET {}
        "#,
        where_clause, limit, offset
    );

    // Build and execute the dynamic query
    let mut query = sqlx::query_as::<_, StudentSearchRow>(&query_str);
    for val in &bind_values {
        query = query.bind(val);
    }

    let rows = query.fetch_all(state.read_pool()).await?;

    // Get total count for pagination
    let count_query_str = format!(
        r#"
        SELECT COUNT(*)::bigint
        FROM users u
        INNER JOIN student_verifications sv ON sv.user_id = u.id
        INNER JOIN universities univ ON univ.id = sv.university_id
        LEFT JOIN user_locations l ON l.user_id = u.id
        WHERE {}
        "#,
        where_clause
    );
    let mut count_query = sqlx::query_scalar::<_, i64>(&count_query_str);
    for val in &bind_values {
        count_query = count_query.bind(val);
    }
    let total_count = count_query.fetch_one(state.read_pool()).await.unwrap_or(0);

    // Batch lookup interaction status for all result user IDs
    let result_ids: Vec<i64> = rows.iter().map(|r| r.id as i64).collect();

    // Get liked status
    let liked_ids: Vec<i64> = if !result_ids.is_empty() {
        sqlx::query_scalar::<_, i64>(
            "SELECT to_user_id FROM swipes WHERE from_user_id = $1 AND to_user_id = ANY($2) AND action = 'like'"
        )
        .bind(user_id as i64)
        .bind(&result_ids)
        .fetch_all(state.read_pool())
        .await
        .unwrap_or_default()
    } else {
        vec![]
    };

    // Get matched status
    let matched_ids: Vec<i32> = if !result_ids.is_empty() {
        let matched_rows = sqlx::query_as::<_, (i32, i32)>(
            r#"
            SELECT user1_id, user2_id FROM matches
            WHERE (user1_id = $1 OR user2_id = $1)
              AND status IN ('accepted', 'pending_direct')
            "#
        )
        .bind(user_id)
        .fetch_all(state.read_pool())
        .await
        .unwrap_or_default();

        matched_rows.iter().map(|(u1, u2)| {
            if *u1 == user_id { *u2 } else { *u1 }
        }).collect()
    } else {
        vec![]
    };

    let results: Vec<StudentSearchResult> = rows.iter().map(|row| {
        let photos = get_student_search_photos(row);
        let age = row.dob.map(|dob| {
            let today = chrono::Utc::now().date_naive();
            today.years_since(dob).unwrap_or(0) as i32
        });
        let distance = match (my_lat, my_lon, row.latitude, row.longitude) {
            (Some(lat1), Some(lon1), Some(lat2), Some(lon2)) => {
                Some(haversine_km(lat1, lon1, lat2, lon2))
            }
            _ => None,
        };

        let interaction_status = if matched_ids.contains(&row.id) {
            "matched".to_string()
        } else if liked_ids.contains(&(row.id as i64)) {
            "liked".to_string()
        } else {
            "none".to_string()
        };

        let is_matched = interaction_status == "matched";

        let display_name_alias = if row.show_display_name_in_search.unwrap_or(false) {
            row.display_name.clone().filter(|s| !s.trim().is_empty())
        } else {
            None
        };

        StudentSearchResult {
            id: row.id.to_string(),
            name: row.name.clone(),
            display_name_alias,
            age,
            photos,
            university: row.university_name.clone().or_else(|| row.university_short_name.clone()),
            university_tier: row.university_tier.as_ref().map(|t| format_tier(t)),
            study: row.profession_title.clone(),
            city: row.city.clone(),
            country: row.university_country.clone(),
            distance,
            bio: row.bio.clone(),
            is_verified: row.is_verified.unwrap_or(false),
            can_message: is_premium || is_matched,
            interaction_status,
            class_year: row.graduation_year,
            is_alumni: row.is_alumni.unwrap_or(false),
            is_new_in_city: row.is_new_in_city.unwrap_or(false),
        }
    }).collect();

    Ok(Json(json!({
        "students": results,
        "total": total_count,
        "is_premium": is_premium
    })))
}

/// GET /search/unified?q=vignan&limit=5
/// Unified search: returns matching universities with students grouped under each.
/// Searches university names, user names, and short names.
/// Returns: { universities: [{ id, name, tier, city, country, student_count, students: [...] }], people: [...] }
pub async fn unified_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<UnifiedSearchQuery>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let q = params.q.as_deref().unwrap_or("").trim();
    if q.is_empty() {
        return Ok(Json(json!({ "universities": [], "people": [], "query": "" })));
    }

    let search_term = format!("%{}%", q.to_lowercase());
    let uni_limit = params.uni_limit.unwrap_or(10).min(20) as i64;
    let students_per_uni = params.students_per_uni.unwrap_or(5).min(20) as i64;

    // Get searcher's location for distance calc
    let my_location = sqlx::query_as::<_, (Option<f64>, Option<f64>)>(
        "SELECT latitude, longitude FROM user_locations WHERE user_id = $1"
    )
    .bind(user_id)
    .fetch_optional(state.read_pool())
    .await?;
    let (my_lat, my_lon) = my_location.unwrap_or((None, None));

    // 1. Find matching universities
    let universities = sqlx::query_as::<_, UnifiedUniRow>(
        r#"
        SELECT u.id, u.name, u.short_name, u.tier, u.city, u.country, u.country_code,
               COUNT(sv.id) FILTER (WHERE sv.status = 'approved') AS student_count
        FROM universities u
        LEFT JOIN student_verifications sv ON sv.university_id = u.id
        WHERE u.is_active = TRUE
          AND (LOWER(u.name) LIKE $1 OR LOWER(u.short_name) LIKE $1)
        GROUP BY u.id, u.name, u.short_name, u.tier, u.city, u.country, u.country_code
        ORDER BY student_count DESC, u.name ASC
        LIMIT $2
        "#
    )
    .bind(&search_term)
    .bind(uni_limit)
    .fetch_all(state.read_pool())
    .await?;

    // 2. For each university, fetch students with distance + optional gender filter
    let mut uni_results: Vec<Value> = Vec::new();
    let gender_filter = params.gender.as_deref().filter(|g| !g.trim().is_empty());

    for uni in &universities {
        // If searching by university_id (user tapped a specific uni), show more profiles
        let per_uni_limit = if params.university_id == Some(uni.id) { 50i64 } else { students_per_uni };

        let students = if let Some(gender) = gender_filter {
            sqlx::query_as::<_, UnifiedStudentRow>(
                r#"
                SELECT u.id, u.name, u.dob, u.gender, u.bio,
                       u.profile_photo_url, u.profile_photo_1, u.profile_photo_2, u.profile_photo_3,
                       u.is_verified, u.profession_title,
                       l.city AS user_city, l.latitude, l.longitude
                FROM users u
                INNER JOIN student_verifications sv ON sv.user_id = u.id AND sv.status = 'approved'
                LEFT JOIN user_locations l ON l.user_id = u.id
                WHERE sv.university_id = $1
                  AND u.is_active = TRUE
                  AND u.is_profile_complete = TRUE
                  AND u.id != $2
                  AND LOWER(u.gender) = LOWER($3)
                ORDER BY u.attractiveness_score DESC NULLS LAST
                LIMIT $4
                "#
            )
            .bind(uni.id)
            .bind(user_id)
            .bind(gender)
            .bind(per_uni_limit)
            .fetch_all(state.read_pool())
            .await?
        } else {
            sqlx::query_as::<_, UnifiedStudentRow>(
                r#"
                SELECT u.id, u.name, u.dob, u.gender, u.bio,
                       u.profile_photo_url, u.profile_photo_1, u.profile_photo_2, u.profile_photo_3,
                       u.is_verified, u.profession_title,
                       l.city AS user_city, l.latitude, l.longitude
                FROM users u
                INNER JOIN student_verifications sv ON sv.user_id = u.id AND sv.status = 'approved'
                LEFT JOIN user_locations l ON l.user_id = u.id
                WHERE sv.university_id = $1
                  AND u.is_active = TRUE
                  AND u.is_profile_complete = TRUE
                  AND u.id != $2
                ORDER BY u.attractiveness_score DESC NULLS LAST
                LIMIT $3
                "#
            )
            .bind(uni.id)
            .bind(user_id)
            .bind(per_uni_limit)
            .fetch_all(state.read_pool())
            .await?
        };

        let student_list: Vec<Value> = students.iter().map(|s| {
            let age = s.dob.map(|dob| {
                chrono::Utc::now().date_naive().years_since(dob).unwrap_or(0) as i32
            });
            let distance = match (my_lat, my_lon, s.latitude, s.longitude) {
                (Some(lat1), Some(lon1), Some(lat2), Some(lon2)) => {
                    Some((haversine_km(lat1, lon1, lat2, lon2) * 10.0).round() / 10.0)
                }
                _ => None,
            };
            let photo = s.profile_photo_url.as_ref()
                .or(s.profile_photo_1.as_ref())
                .or(s.profile_photo_2.as_ref())
                .or(s.profile_photo_3.as_ref())
                .cloned();

            json!({
                "id": s.id,
                "name": s.name,
                "age": age,
                "photo": photo,
                "gender": s.gender,
                "study": s.profession_title,
                "city": s.user_city,
                "distance_km": distance,
                "is_verified": s.is_verified.unwrap_or(false)
            })
        }).collect();

        uni_results.push(json!({
            "id": uni.id,
            "name": uni.name,
            "short_name": uni.short_name,
            "tier": uni.tier.as_ref().map(|t| format_tier(t)),
            "city": uni.city,
            "country": uni.country,
            "country_code": uni.country_code,
            "student_count": uni.student_count.unwrap_or(0),
            "students": student_list
        }));
    }

    // 3. Also search people by name (across all universities) with filters.
    // Matches verified users.name; also display_name if the user opted in.
    let mut people_conditions = vec![
        "u.is_active = TRUE".to_string(),
        "u.is_profile_complete = TRUE".to_string(),
        format!("u.id != {}", user_id),
        "(LOWER(u.name) LIKE $1 OR (COALESCE(u.show_display_name_in_search, FALSE) = TRUE AND LOWER(u.display_name) LIKE $1))".to_string(),
    ];
    let mut people_binds: Vec<String> = vec![search_term.clone()];
    let mut pidx = 1u32;

    if let Some(ref gender) = params.gender {
        if !gender.trim().is_empty() {
            pidx += 1;
            people_conditions.push(format!("u.gender = ${}", pidx));
            people_binds.push(gender.clone());
        }
    }
    if let Some(ref city) = params.city {
        if !city.trim().is_empty() {
            pidx += 1;
            people_conditions.push(format!("LOWER(l.city) LIKE ${}", pidx));
            people_binds.push(format!("%{}%", city.trim().to_lowercase()));
        }
    }
    if let Some(min_age) = params.min_age {
        let max_dob = chrono::Utc::now().date_naive() - chrono::Duration::days(min_age as i64 * 365);
        people_conditions.push(format!("u.dob <= '{}'", max_dob));
    }
    if let Some(max_age) = params.max_age {
        let min_dob = chrono::Utc::now().date_naive() - chrono::Duration::days((max_age as i64 + 1) * 365);
        people_conditions.push(format!("u.dob >= '{}'", min_dob));
    }
    if let Some(uni_id) = params.university_id {
        people_conditions.push(format!(
            "EXISTS (SELECT 1 FROM student_verifications sv2 WHERE sv2.user_id = u.id AND sv2.university_id = {} AND sv2.status = 'approved')", uni_id
        ));
    }
    if let Some(ref tier) = params.tier {
        if !tier.trim().is_empty() {
            people_conditions.push(format!(
                "EXISTS (SELECT 1 FROM student_verifications sv3 JOIN universities univ3 ON univ3.id = sv3.university_id WHERE sv3.user_id = u.id AND univ3.tier = '{}')", tier.replace('\'', "")
            ));
        }
    }

    let people_where = people_conditions.join(" AND ");
    let people_sql = format!(
        r#"
        SELECT u.id, u.name, u.dob, u.gender, u.bio,
               u.profile_photo_url, u.profile_photo_1, u.profile_photo_2, u.profile_photo_3,
               u.is_verified, u.profession_title,
               l.city AS user_city, l.latitude, l.longitude
        FROM users u
        LEFT JOIN user_locations l ON l.user_id = u.id
        WHERE {}
        ORDER BY u.attractiveness_score DESC NULLS LAST
        LIMIT 20
        "#,
        people_where
    );

    let mut people_query = sqlx::query_as::<_, UnifiedStudentRow>(&people_sql);
    for val in &people_binds {
        people_query = people_query.bind(val);
    }
    let people = people_query.fetch_all(state.read_pool()).await?;

    // Get university info for people results
    let people_ids: Vec<i32> = people.iter().map(|p| p.id).collect();
    let uni_map = batch_lookup_university(&state.db, &people_ids).await?;

    let people_results: Vec<Value> = people.iter().map(|s| {
        let age = s.dob.map(|dob| {
            chrono::Utc::now().date_naive().years_since(dob).unwrap_or(0) as i32
        });
        let distance = match (my_lat, my_lon, s.latitude, s.longitude) {
            (Some(lat1), Some(lon1), Some(lat2), Some(lon2)) => {
                Some((haversine_km(lat1, lon1, lat2, lon2) * 10.0).round() / 10.0)
            }
            _ => None,
        };
        let photo = s.profile_photo_url.as_ref()
            .or(s.profile_photo_1.as_ref())
            .or(s.profile_photo_2.as_ref())
            .or(s.profile_photo_3.as_ref())
            .cloned();
        let (uni_name, uni_tier) = uni_map.get(&s.id).map(|(n, t)| (Some(n.as_str()), Some(format_tier(t)))).unwrap_or((None, None));

        json!({
            "id": s.id,
            "name": s.name,
            "age": age,
            "photo": photo,
            "gender": s.gender,
            "university": uni_name,
            "university_tier": uni_tier,
            "study": s.profession_title,
            "city": s.user_city,
            "distance_km": distance,
            "is_verified": s.is_verified.unwrap_or(false)
        })
    }).collect();

    let has_filters = params.gender.is_some() || params.min_age.is_some() || params.max_age.is_some()
        || params.city.is_some() || params.university_id.is_some() || params.tier.is_some();

    Ok(Json(json!({
        "query": q,
        "universities": uni_results,
        "people": people_results,
        "filters_applied": has_filters,
        "total_people": people_results.len(),
        "total_universities": uni_results.len()
    })))
}

#[derive(Debug, Deserialize)]
pub struct UnifiedSearchQuery {
    pub q: Option<String>,
    pub uni_limit: Option<i32>,
    pub students_per_uni: Option<i32>,
    // Filters for narrowing people results
    pub gender: Option<String>,
    pub min_age: Option<i32>,
    pub max_age: Option<i32>,
    pub city: Option<String>,
    pub country: Option<String>,
    pub university_id: Option<i64>,
    pub tier: Option<String>,
}

#[derive(Debug, sqlx::FromRow)]
struct UnifiedUniRow {
    id: i64,
    name: String,
    short_name: Option<String>,
    tier: Option<String>,
    city: Option<String>,
    country: String,
    country_code: String,
    student_count: Option<i64>,
}

#[derive(Debug, sqlx::FromRow)]
struct UnifiedStudentRow {
    id: i32,
    name: Option<String>,
    dob: Option<NaiveDate>,
    gender: Option<String>,
    bio: Option<String>,
    profile_photo_url: Option<String>,
    profile_photo_1: Option<String>,
    profile_photo_2: Option<String>,
    profile_photo_3: Option<String>,
    is_verified: Option<bool>,
    profession_title: Option<String>,
    user_city: Option<String>,
    latitude: Option<f64>,
    longitude: Option<f64>,
}

/// Get all profiles from a specific university with gender filter.
/// GET /universities/{id}/profiles?gender=male&limit=50
pub async fn get_university_profiles(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(university_id): AxumPath<i64>,
    Query(params): Query<UniversityProfilesQuery>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let limit = params.limit.unwrap_or(50).min(100) as i64;

    // Get university info
    let uni = sqlx::query_as::<_, UnifiedUniRow>(
        r#"
        SELECT u.id, u.name, u.short_name, u.tier, u.city, u.country, u.country_code,
               COUNT(sv.id) FILTER (WHERE sv.status = 'approved') AS student_count
        FROM universities u
        LEFT JOIN student_verifications sv ON sv.university_id = u.id
        WHERE u.id = $1 AND u.is_active = TRUE
        GROUP BY u.id, u.name, u.short_name, u.tier, u.city, u.country, u.country_code
        "#
    )
    .bind(university_id)
    .fetch_optional(state.read_pool())
    .await?
    .ok_or_else(|| AppError::not_found("University not found"))?;

    // Get searcher's location for distance
    let my_loc = sqlx::query_as::<_, (Option<f64>, Option<f64>)>(
        "SELECT latitude, longitude FROM user_locations WHERE user_id = $1"
    )
    .bind(user_id)
    .fetch_optional(state.read_pool())
    .await?;
    let (my_lat, my_lon) = my_loc.unwrap_or((None, None));

    // Fetch profiles with optional gender filter
    let gender_filter = params.gender.as_deref().filter(|g| !g.trim().is_empty());
    let students = if let Some(gender) = gender_filter {
        sqlx::query_as::<_, UnifiedStudentRow>(
            r#"
            SELECT u.id, u.name, u.dob, u.gender, u.bio,
                   u.profile_photo_url, u.profile_photo_1, u.profile_photo_2, u.profile_photo_3,
                   u.is_verified, u.profession_title,
                   l.city AS user_city, l.latitude, l.longitude
            FROM users u
            INNER JOIN student_verifications sv ON sv.user_id = u.id AND sv.status = 'approved'
            LEFT JOIN user_locations l ON l.user_id = u.id
            WHERE sv.university_id = $1
              AND u.is_active = TRUE
              AND u.is_profile_complete = TRUE
              AND u.id != $2
              AND LOWER(u.gender) = LOWER($3)
            ORDER BY u.attractiveness_score DESC NULLS LAST
            LIMIT $4
            "#
        )
        .bind(university_id)
        .bind(user_id)
        .bind(gender)
        .bind(limit)
        .fetch_all(state.read_pool())
        .await?
    } else {
        sqlx::query_as::<_, UnifiedStudentRow>(
            r#"
            SELECT u.id, u.name, u.dob, u.gender, u.bio,
                   u.profile_photo_url, u.profile_photo_1, u.profile_photo_2, u.profile_photo_3,
                   u.is_verified, u.profession_title,
                   l.city AS user_city, l.latitude, l.longitude
            FROM users u
            INNER JOIN student_verifications sv ON sv.user_id = u.id AND sv.status = 'approved'
            LEFT JOIN user_locations l ON l.user_id = u.id
            WHERE sv.university_id = $1
              AND u.is_active = TRUE
              AND u.is_profile_complete = TRUE
              AND u.id != $2
            ORDER BY u.attractiveness_score DESC NULLS LAST
            LIMIT $3
            "#
        )
        .bind(university_id)
        .bind(user_id)
        .bind(limit)
        .fetch_all(state.read_pool())
        .await?
    };

    let profiles: Vec<Value> = students.iter().map(|s| {
        let age = s.dob.map(|dob| {
            chrono::Utc::now().date_naive().years_since(dob).unwrap_or(0) as i32
        });
        let distance = match (my_lat, my_lon, s.latitude, s.longitude) {
            (Some(lat1), Some(lon1), Some(lat2), Some(lon2)) => {
                Some((haversine_km(lat1, lon1, lat2, lon2) * 10.0).round() / 10.0)
            }
            _ => None,
        };
        let photo = s.profile_photo_url.as_ref()
            .or(s.profile_photo_1.as_ref())
            .or(s.profile_photo_2.as_ref())
            .or(s.profile_photo_3.as_ref())
            .cloned();

        json!({
            "id": s.id,
            "name": s.name,
            "age": age,
            "photo": photo,
            "gender": s.gender,
            "bio": s.bio,
            "study": s.profession_title,
            "city": s.user_city,
            "distance_km": distance,
            "distance_text": distance.map(format_distance),
            "is_verified": s.is_verified.unwrap_or(false)
        })
    }).collect();

    Ok(Json(json!({
        "university": {
            "id": uni.id,
            "name": uni.name,
            "short_name": uni.short_name,
            "tier": uni.tier.as_ref().map(|t| format_tier(t)),
            "city": uni.city,
            "country": uni.country,
            "student_count": uni.student_count.unwrap_or(0)
        },
        "profiles": profiles,
        "total": profiles.len(),
        "gender_filter": gender_filter
    })))
}

#[derive(Debug, Deserialize)]
pub struct UniversityProfilesQuery {
    pub gender: Option<String>,
    pub limit: Option<i32>,
}

/// Format tier from DB value to display string
fn format_tier(tier: &str) -> String {
    match tier {
        "top_private" => "Top Private".to_string(),
        "top_public" => "Top Public".to_string(),
        "regular" => "Regular".to_string(),
        "graduate" => "Graduate".to_string(),
        "alumni" => "Alumni".to_string(),
        other => other.to_string(),
    }
}

/// Batch lookup university name + tier for a list of user IDs.
/// Returns HashMap<user_id, (university_name, tier)>.
async fn batch_lookup_university(
    db: &sqlx::PgPool,
    user_ids: &[i32],
) -> Result<std::collections::HashMap<i32, (String, String)>, AppError> {
    if user_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let placeholders: Vec<String> = user_ids.iter().enumerate().map(|(i, _)| format!("${}", i + 1)).collect();
    let query_str = format!(
        r#"
        SELECT sv.user_id, univ.name, univ.tier
        FROM student_verifications sv
        INNER JOIN universities univ ON univ.id = sv.university_id
        WHERE sv.status = 'approved' AND sv.user_id IN ({})
        "#,
        placeholders.join(", ")
    );

    let mut query = sqlx::query_as::<_, (i32, String, String)>(&query_str);
    for id in user_ids {
        query = query.bind(id);
    }

    let rows = query.fetch_all(db).await?;
    let mut map = std::collections::HashMap::new();
    for (uid, name, tier) in rows {
        map.insert(uid, (name, tier));
    }
    Ok(map)
}

/// Batch lookup university name + tier + country for a list of user IDs.
/// Returns HashMap<user_id, (university_name, tier, country)>.
async fn batch_lookup_university_full(
    db: &sqlx::PgPool,
    user_ids: &[i32],
) -> Result<std::collections::HashMap<i32, (String, String, String)>, AppError> {
    if user_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let placeholders: Vec<String> = user_ids.iter().enumerate().map(|(i, _)| format!("${}", i + 1)).collect();
    let query_str = format!(
        r#"
        SELECT sv.user_id, univ.name, univ.tier, univ.country
        FROM student_verifications sv
        INNER JOIN universities univ ON univ.id = sv.university_id
        WHERE sv.status = 'approved' AND sv.user_id IN ({})
        "#,
        placeholders.join(", ")
    );

    let mut query = sqlx::query_as::<_, (i32, String, String, String)>(&query_str);
    for id in user_ids {
        query = query.bind(id);
    }

    let rows = query.fetch_all(db).await?;
    let mut map = std::collections::HashMap::new();
    for (uid, name, tier, country) in rows {
        map.insert(uid, (name, tier, country));
    }
    Ok(map)
}

/// Batch lookup interaction status between current user and a list of target user IDs.
/// Returns HashMap<target_user_id, "none"|"liked"|"matched">.
async fn batch_lookup_interactions(
    db: &sqlx::PgPool,
    user_id: i32,
    target_ids: &[i32],
) -> Result<std::collections::HashMap<i32, String>, AppError> {
    if target_ids.is_empty() {
        return Ok(std::collections::HashMap::new());
    }

    let placeholders: Vec<String> = target_ids.iter().enumerate().map(|(i, _)| format!("${}", i + 2)).collect();
    let query_str = format!(
        r#"
        SELECT
            CASE WHEN m.user1_id = $1 THEN m.user2_id ELSE m.user1_id END as target_id,
            m.is_mutual_match,
            CASE WHEN m.user1_id = $1 THEN m.user1_liked ELSE m.user2_liked END as i_liked
        FROM matches m
        WHERE (
            (m.user1_id = $1 AND m.user2_id IN ({}))
            OR (m.user2_id = $1 AND m.user1_id IN ({}))
        )
        "#,
        placeholders.join(", "),
        placeholders.join(", ")
    );

    let mut query = sqlx::query_as::<_, (i32, Option<bool>, Option<bool>)>(&query_str);
    query = query.bind(user_id);
    for id in target_ids {
        query = query.bind(id);
    }

    let rows = query.fetch_all(db).await?;
    let mut map = std::collections::HashMap::new();
    for (target_id, is_mutual, i_liked) in rows {
        let status = if is_mutual.unwrap_or(false) {
            "matched"
        } else if i_liked.unwrap_or(false) {
            "liked"
        } else {
            "none"
        };
        map.insert(target_id, status.to_string());
    }
    Ok(map)
}

/// Convert ISO 3166-1 alpha-3 country code to flag emoji
fn country_code_to_flag(code: &str) -> String {
    // Convert 3-letter to 2-letter for flag emoji
    let alpha2 = match code.to_uppercase().as_str() {
        "IND" => "IN", "USA" => "US", "GBR" => "GB", "AUS" => "AU",
        "CAN" => "CA", "SGP" => "SG", "DEU" => "DE", "FRA" => "FR",
        "JPN" => "JP", "KOR" => "KR", "CHN" => "CN", "NLD" => "NL",
        "CHE" => "CH", "ISR" => "IL", "ZAF" => "ZA", "SAU" => "SA",
        "ARE" => "AE", "BRA" => "BR", "MEX" => "MX", "NZL" => "NZ",
        "HKG" => "HK", "TWN" => "TW", "MYS" => "MY", "THA" => "TH",
        "PHL" => "PH", "IDN" => "ID", "VNM" => "VN", "PAK" => "PK",
        "BGD" => "BD", "LKA" => "LK", "NPL" => "NP", "TUR" => "TR",
        "RUS" => "RU", "POL" => "PL", "ESP" => "ES", "ITA" => "IT",
        "IRL" => "IE", "SWE" => "SE", "NOR" => "NO", "DNK" => "DK",
        "FIN" => "FI", "AUT" => "AT", "BEL" => "BE", "PRT" => "PT",
        "COL" => "CO", "ARG" => "AR", "PER" => "PE", "CHL" => "CL",
        "EGY" => "EG", "NGA" => "NG",
        // If already 2-letter, use as-is
        s if s.len() == 2 => return regional_indicator(s),
        _ => return "🌍".to_string(),
    };
    regional_indicator(alpha2)
}

fn regional_indicator(code: &str) -> String {
    code.chars()
        .map(|c| char::from_u32(0x1F1E6 + (c as u32 - 'A' as u32)).unwrap_or('?'))
        .collect()
}

fn get_student_search_photos(row: &StudentSearchRow) -> Vec<String> {
    if let Some(Value::Array(items)) = &row.profile_photos {
        let photos: Vec<String> = items
            .iter()
            .filter_map(|v| v.as_str().map(|s| s.to_string()))
            .collect();
        if !photos.is_empty() {
            return photos;
        }
    }
    if let Some(csv) = &row.profile_photo_url {
        let photos: Vec<String> = csv
            .split(',')
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect();
        if !photos.is_empty() {
            return photos;
        }
    }
    let mut photos = Vec::new();
    if let Some(v) = &row.profile_photo_1 { if !v.is_empty() { photos.push(v.clone()); } }
    if let Some(v) = &row.profile_photo_2 { if !v.is_empty() { photos.push(v.clone()); } }
    if let Some(v) = &row.profile_photo_3 { if !v.is_empty() { photos.push(v.clone()); } }
    photos
}

/// GET /search/students/suggestions — Trending universities + search suggestions
pub async fn student_search_suggestions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let is_student = sqlx::query_scalar::<_, bool>(
        "SELECT COALESCE(is_student_verified, FALSE) FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    if !is_student {
        return Err(AppError::forbidden("Student verification required"));
    }

    // Top universities by verified student count
    let trending = sqlx::query_as::<_, (i64, String, Option<String>, String, String, Option<String>, i64)>(
        r#"
        SELECT univ.id, univ.name, univ.short_name, univ.country, univ.country_code,
               univ.tier, COUNT(sv.id) AS student_count
        FROM universities univ
        INNER JOIN student_verifications sv ON sv.university_id = univ.id AND sv.status = 'approved'
        WHERE univ.is_active = TRUE
        GROUP BY univ.id, univ.name, univ.short_name, univ.country, univ.country_code, univ.tier
        ORDER BY student_count DESC
        LIMIT 20
        "#
    )
    .fetch_all(state.read_pool())
    .await?;

    let trending_universities: Vec<Value> = trending.iter().map(|(id, name, _short, _country, _cc, _tier, count)| {
        json!({
            "id": id.to_string(),
            "name": name,
            "student_count": count
        })
    }).collect();

    // Top cities with students
    let top_cities = sqlx::query_as::<_, (String, i64)>(
        r#"
        SELECT l.city, COUNT(DISTINCT sv.user_id) AS student_count
        FROM user_locations l
        INNER JOIN student_verifications sv ON sv.user_id = l.user_id AND sv.status = 'approved'
        WHERE l.city IS NOT NULL AND l.city != ''
        GROUP BY l.city
        ORDER BY student_count DESC
        LIMIT 15
        "#
    )
    .fetch_all(state.read_pool())
    .await?;

    let cities: Vec<Value> = top_cities.iter().map(|(city, count)| {
        json!({ "name": city, "student_count": count })
    }).collect();

    // Countries with student presence
    let countries = sqlx::query_as::<_, (String, String, i64)>(
        r#"
        SELECT univ.country, univ.country_code, COUNT(DISTINCT sv.user_id) AS student_count
        FROM universities univ
        INNER JOIN student_verifications sv ON sv.university_id = univ.id AND sv.status = 'approved'
        GROUP BY univ.country, univ.country_code
        ORDER BY student_count DESC
        "#
    )
    .fetch_all(state.read_pool())
    .await?;

    let country_list: Vec<Value> = countries.iter().map(|(name, code, count)| {
        json!({
            "code": code,
            "name": name,
            "flag": country_code_to_flag(code),
            "student_count": count
        })
    }).collect();

    Ok(Json(json!({
        "trending_universities": trending_universities,
        "top_cities": cities,
        "countries": country_list
    })))
}

/// POST /search/students/{user_id}/like — Like a student from search results
pub async fn like_student_from_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(target_user_id): AxumPath<i32>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Both must be verified students
    let both_verified = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) FROM users
        WHERE id IN ($1, $2) AND is_student_verified = TRUE
        "#
    )
    .bind(user_id)
    .bind(target_user_id)
    .fetch_one(&state.db)
    .await?;

    if both_verified < 2 {
        return Err(AppError::forbidden("Both users must be verified students"));
    }

    // Check if already swiped
    let existing = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM swipes WHERE from_user_id = $1 AND to_user_id = $2"
    )
    .bind(user_id as i64)
    .bind(target_user_id as i64)
    .fetch_one(&state.db)
    .await?;

    if existing > 0 {
        return Err(AppError::bad_request("Already swiped on this user"));
    }

    // Record the swipe
    sqlx::query(
        "INSERT INTO swipes (from_user_id, to_user_id, action, source) VALUES ($1, $2, 'like', 'student_search')"
    )
    .bind(user_id as i64)
    .bind(target_user_id as i64)
    .execute(&state.db)
    .await?;

    // Check for mutual match
    let mutual = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM swipes WHERE from_user_id = $1 AND to_user_id = $2 AND action = 'like'"
    )
    .bind(target_user_id as i64)
    .bind(user_id as i64)
    .fetch_one(&state.db)
    .await?;

    let mut match_id: Option<String> = None;
    if mutual > 0 {
        let new_match_id = Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT INTO matches (id, user1_id, user2_id, status, user1_liked, user2_liked, created_at)
            VALUES ($1, $2, $3, 'accepted', TRUE, TRUE, NOW())
            ON CONFLICT DO NOTHING
            "#
        )
        .bind(&new_match_id)
        .bind(user_id)
        .bind(target_user_id)
        .execute(&state.db)
        .await?;
        match_id = Some(new_match_id);
    }

    Ok(Json(json!({
        "liked": true,
        "is_match": mutual > 0,
        "match_id": match_id,
        "source": "student_search"
    })))
}

/// POST /search/students/{user_id}/message — Premium: direct message from search
#[derive(Debug, Deserialize)]
pub struct DirectMessageRequest {
    pub message: String,
}

pub async fn direct_message_from_search(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(target_user_id): AxumPath<i32>,
    Json(payload): Json<DirectMessageRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Premium check
    let active_pass = get_active_pass(&state.db, user_id).await?;
    if active_pass.is_none() {
        return Err(AppError::forbidden(
            "Premium subscription required to message directly. Upgrade to unlock direct messaging."
        ));
    }

    // Both must be verified students
    let both_verified = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE id IN ($1, $2) AND is_student_verified = TRUE"
    )
    .bind(user_id)
    .bind(target_user_id)
    .fetch_one(&state.db)
    .await?;

    if both_verified < 2 {
        return Err(AppError::forbidden("Both users must be verified students"));
    }

    // Validate message
    let message = payload.message.trim();
    if message.is_empty() || message.len() > 500 {
        return Err(AppError::bad_request("Message must be 1-500 characters"));
    }

    // Ensure a match exists (create if premium direct message)
    let existing_match = sqlx::query_scalar::<_, String>(
        r#"
        SELECT id FROM matches
        WHERE (user1_id = $1 AND user2_id = $2) OR (user1_id = $2 AND user2_id = $1)
        LIMIT 1
        "#
    )
    .bind(user_id)
    .bind(target_user_id)
    .fetch_optional(&state.db)
    .await?;

    let match_id = if let Some(mid) = existing_match {
        mid
    } else {
        // Premium privilege: create a direct connection (like LinkedIn InMail)
        let new_match_id = Uuid::new_v4().to_string();
        sqlx::query(
            r#"
            INSERT INTO matches (id, user1_id, user2_id, status, user1_liked, user2_liked, created_at)
            VALUES ($1, $2, $3, 'pending_direct', TRUE, FALSE, NOW())
            "#
        )
        .bind(&new_match_id)
        .bind(user_id)
        .bind(target_user_id)
        .execute(&state.db)
        .await?;
        new_match_id
    };

    // Send the message
    let message_id = sqlx::query_scalar::<_, i32>(
        r#"
        INSERT INTO messages (match_id, sender_id, content, message_type, created_at)
        VALUES ($1, $2, $3, 'direct_search', NOW())
        RETURNING id
        "#
    )
    .bind(&match_id)
    .bind(user_id)
    .bind(message)
    .fetch_one(&state.db)
    .await?;

    // Auto-queue for LLM labeling
    auto_queue_for_labeling(state.db.clone(), state.config.llm_enabled, "message", message_id as i64, 5);

    Ok(Json(json!({
        "sent": true,
        "message_id": message_id,
        "match_id": match_id,
        "type": "direct_search_message",
        "premium_feature": true
    })))
}

/// GET /search/students/profile/{user_id} — View a student's full profile from search
pub async fn view_student_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(target_user_id): AxumPath<i32>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Must be verified student
    let is_student = sqlx::query_scalar::<_, bool>(
        "SELECT COALESCE(is_student_verified, FALSE) FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    if !is_student {
        return Err(AppError::forbidden("Student verification required"));
    }

    let active_pass = get_active_pass(&state.db, user_id).await?;
    let is_premium = active_pass.is_some();

    // Fetch target profile with university info
    let profile = sqlx::query_as::<_, StudentSearchRow>(
        r#"
        SELECT u.id, u.name, u.display_name, u.show_display_name_in_search, u.show_verified_name,
               u.dob, u.gender, u.bio,
               u.profile_photo_url, u.profile_photos,
               u.profile_photo_1, u.profile_photo_2, u.profile_photo_3,
               u.is_verified, u.attractiveness_score, u.looking_for,
               u.profession_title, u.height_cm,
               l.city, l.latitude, l.longitude,
               univ.name AS university_name,
               univ.short_name AS university_short_name,
               univ.tier AS university_tier,
               univ.country AS university_country
        FROM users u
        INNER JOIN student_verifications sv ON sv.user_id = u.id AND sv.status = 'approved'
        INNER JOIN universities univ ON univ.id = sv.university_id
        LEFT JOIN user_locations l ON l.user_id = u.id
        WHERE u.id = $1
        "#
    )
    .bind(target_user_id)
    .fetch_optional(state.read_pool())
    .await?
    .ok_or_else(|| AppError::not_found("Student profile not found"))?;

    let photos = get_student_search_photos(&profile);
    let age = profile.dob.map(|dob| {
        let today = chrono::Utc::now().date_naive();
        today.years_since(dob).unwrap_or(0) as i32
    });

    // Check existing interaction
    let already_liked = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM swipes WHERE from_user_id = $1 AND to_user_id = $2 AND action = 'like'"
    )
    .bind(user_id as i64)
    .bind(target_user_id as i64)
    .fetch_one(state.read_pool())
    .await
    .unwrap_or(0);

    let is_matched = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT COUNT(*) FROM matches
        WHERE ((user1_id = $1 AND user2_id = $2) OR (user1_id = $2 AND user2_id = $1))
          AND status IN ('accepted', 'pending_direct')
        "#
    )
    .bind(user_id)
    .bind(target_user_id)
    .fetch_one(state.read_pool())
    .await
    .unwrap_or(0);

    let interaction_status = if is_matched > 0 {
        "matched"
    } else if already_liked > 0 {
        "liked"
    } else {
        "none"
    };

    let public_name = public_name_for_viewer(
        profile.name.as_deref(),
        profile.display_name.as_deref(),
        profile.show_verified_name,
    );

    Ok(Json(json!({
        "profile": {
            "id": profile.id.to_string(),
            "name": public_name,
            "display_name": profile.display_name,
            "age": age,
            "gender": profile.gender,
            "bio": profile.bio,
            "photos": photos,
            "is_verified": profile.is_verified.unwrap_or(false),
            "looking_for": profile.looking_for,
            "study": profile.profession_title,
            "height_cm": profile.height_cm,
            "city": profile.city,
            "country": profile.university_country,
            "university": profile.university_name,
            "university_tier": profile.university_tier.as_ref().map(|t| format_tier(t)),
            "distance": serde_json::Value::Null,
            "can_message": is_premium || is_matched > 0,
            "interaction_status": interaction_status
        },
        "is_premium": is_premium
    })))
}

// ============================================================================
// Unified Profile View — works from discover, reels, search, or any surface
// ============================================================================

/// GET /profile/{user_id} — Unified profile view from any surface
pub async fn get_user_profile(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(target_user_id): AxumPath<i32>,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let source = params.get("source").cloned().unwrap_or_else(|| "profile".to_string());

    if user_id == target_user_id {
        return Err(AppError::bad_request("Use /me for your own profile"));
    }

    let read_db = state.read_pool();

    // Fetch target user
    let target = sqlx::query_as::<_, DiscoverUserRow>(
        r#"
        SELECT u.id, u.name, u.display_name, u.show_verified_name,
               u.dob, u.gender, u.bio, u.profile_photo_url, u.profile_photos,
               u.profile_photo_1, u.profile_photo_2, u.profile_photo_3,
               u.is_verified, u.attractiveness_score, u.looking_for,
               u.profession_title, u.height_cm,
               l.city, l.latitude, l.longitude
        FROM users u
        LEFT JOIN user_locations l ON l.user_id = u.id
        WHERE u.id = $1 AND u.is_active = TRUE
        "#
    )
    .bind(target_user_id)
    .fetch_optional(read_db)
    .await?
    .ok_or_else(|| AppError::not_found("User not found"))?;

    let photos = get_photos_from_row(&target);
    let age = target.dob.map(calculate_age);

    // Distance from viewer
    let my_location = fetch_user_location(read_db, user_id).await?;
    let distance_km = if let (Some(ul), Some(lat), Some(lon)) = (&my_location, target.latitude, target.longitude) {
        ul.latitude.zip(ul.longitude).map(|(ulat, ulon)| haversine_km(ulat, ulon, lat, lon))
    } else {
        None
    };

    // University info (name, tier, country)
    let uni_map = batch_lookup_university_full(read_db, &[target_user_id]).await?;
    let uni_info = uni_map.get(&target_user_id);

    // Interaction status
    let interaction_map = batch_lookup_interactions(read_db, user_id, &[target_user_id]).await?;
    let interaction_status = interaction_map.get(&target_user_id).cloned().unwrap_or_else(|| "none".to_string());

    // Premium check for messaging
    let active_pass = get_active_pass(&state.db, user_id).await?;
    let is_premium = active_pass.is_some();
    let can_message = is_premium || interaction_status == "matched";

    // Log profile view event for ML
    let _ = log_interaction_event(
        &state.db, user_id, target_user_id,
        "profile_view", None, None, Some(&source),
    ).await;

    let public_name = public_name_for_viewer(
        target.name.as_deref(),
        target.display_name.as_deref(),
        target.show_verified_name,
    );

    // Professional-only privacy gate for global search surfaces.
    // Photo search / Visual span ALL verified users — a broader pool than the
    // viewer's filtered discover feed — so a non-match reached via search must
    // see only professional fields, never the dating bio or dating photos.
    // Discover, reels, and every other surface are unchanged.
    let professional_only = matches!(source.as_str(), "search" | "visual" | "clip")
        && interaction_status != "matched";

    if professional_only {
        return Ok(Json(json!({
            "profile": {
                "id": target.id,
                "name": public_name,
                "study": target.profession_title,
                "is_verified": target.is_verified.unwrap_or(false),
                "city": target.city,
                "country": uni_info.map(|(_, _, country)| country.clone()),
                "university": uni_info.map(|(name, _, _)| name.clone()),
                "university_tier": uni_info.map(|(_, tier, _)| format_tier(tier)),
                "distance": distance_km,
                "distance_text": distance_km.map(format_distance),
                "interaction_status": interaction_status,
                "is_match": false,
                "professional_only": true,
                "can_message": can_message,
                "can_like": interaction_status == "none"
            },
            "source": source,
            "is_premium": is_premium
        })));
    }

    Ok(Json(json!({
        "profile": {
            "id": target.id,
            "name": public_name,
            "display_name": target.display_name,
            "age": age,
            "gender": target.gender,
            "bio": target.bio,
            "photos": photos,
            "is_verified": target.is_verified.unwrap_or(false),
            "looking_for": target.looking_for,
            "study": target.profession_title,
            "height_cm": target.height_cm,
            "city": target.city,
            "country": uni_info.map(|(_, _, country)| country.clone()),
            "university": uni_info.map(|(name, _, _)| name.clone()),
            "university_tier": uni_info.map(|(_, tier, _)| format_tier(tier)),
            "distance": distance_km,
            "distance_text": distance_km.map(format_distance),
            "interaction_status": interaction_status,
            "can_message": can_message,
            "can_like": interaction_status == "none"
        },
        "source": source,
        "is_premium": is_premium
    })))
}

// ============================================================================
// Like from Reel — like the reel creator directly from the reel feed
// ============================================================================

/// POST /reels/{reel_id}/like-creator — Like the creator of a reel (dating action)
pub async fn like_reel_creator(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(reel_id): AxumPath<i64>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Get the reel's creator
    let creator_id_i64 = sqlx::query_scalar::<_, i64>("SELECT user_id FROM reels WHERE id = $1 AND is_active = TRUE")
        .bind(reel_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("Reel not found"))?;
    let creator_id = creator_id_i64 as i32;

    if user_id == creator_id {
        return Err(AppError::bad_request("Cannot like yourself"));
    }

    // Determine user order (lower ID is user1)
    let (user1_id, user2_id, is_user1) = if user_id < creator_id {
        (user_id, creator_id, true)
    } else {
        (creator_id, user_id, false)
    };

    // Check for existing match record
    let existing = sqlx::query_as::<_, MatchCheckRow>(
        "SELECT id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match FROM matches WHERE user1_id = $1 AND user2_id = $2",
    )
    .bind(user1_id)
    .bind(user2_id)
    .fetch_optional(&state.db)
    .await?;

    let (is_match, match_id) = if let Some(m) = existing {
        let other_liked = if is_user1 { m.user2_liked } else { m.user1_liked };
        if other_liked.unwrap_or(false) {
            // Mutual match!
            sqlx::query(
                "UPDATE matches SET is_mutual_match = TRUE, status = 'accepted', user1_liked = TRUE, user2_liked = TRUE WHERE id = $1"
            )
            .bind(&m.id)
            .execute(&state.db)
            .await?;
            (true, Some(m.id))
        } else {
            // Update our like
            let col = if is_user1 { "user1_liked" } else { "user2_liked" };
            sqlx::query(&format!("UPDATE matches SET {} = TRUE WHERE id = $1", col))
                .bind(&m.id)
                .execute(&state.db)
                .await?;
            (false, Some(m.id))
        }
    } else {
        // Create new match record
        let new_id = sqlx::query_scalar::<_, String>(
            r#"
            INSERT INTO matches (user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, status)
            VALUES ($1, $2, $3, $4, FALSE, 'pending')
            RETURNING id
            "#,
        )
        .bind(user1_id)
        .bind(user2_id)
        .bind(is_user1)
        .bind(!is_user1)
        .fetch_one(&state.db)
        .await
        .ok();

        (false, new_id)
    };

    // Log the interaction for ML (source = reel)
    let _ = log_interaction_event(
        &state.db, user_id, creator_id,
        "like", None, None, Some("reel"),
    ).await;

    // Also record in swipes for consistency
    let _ = sqlx::query(
        "INSERT INTO swipes (from_user_id, to_user_id, action, source) VALUES ($1, $2, 'like', 'reel') ON CONFLICT DO NOTHING"
    )
    .bind(user_id as i64)
    .bind(creator_id as i64)
    .execute(&state.db)
    .await;

    state.metrics.inc_swipe_writes();

    Ok(Json(json!({
        "liked": true,
        "is_match": is_match,
        "match_id": match_id,
        "creator_id": creator_id,
        "source": "reel",
        "reel_id": reel_id
    })))
}

// ============================================================================
// AI Insights — compatibility breakdown between two users
// ============================================================================

/// GET /ai/insights/{user_id} — Returns ML-powered compatibility insights
/// between the authenticated user and the target user.
/// Used by AIInsightsView in the iOS app.
pub async fn ai_insights(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(target_user_id): AxumPath<i32>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    if user_id == target_user_id {
        return Err(AppError::bad_request("Cannot get insights for yourself"));
    }

    let db = state.read_pool();

    // --- Fetch both users' tags ---
    let (my_interests, my_langs, my_intent) =
        crate::ml::affinity::fetch_user_tags(db, user_id).await;
    let (their_interests, their_langs, their_intent) =
        crate::ml::affinity::fetch_user_tags(db, target_user_id).await;

    // --- Affinity scores ---
    let scorer = crate::ml::affinity::AffinityScorer::default();
    let cf_score = {
        let ml = state.ml.read().await;
        ml.co_likes.cf_score(user_id, target_user_id)
    };
    let affinity = scorer.score(
        &my_interests,
        &their_interests,
        &my_langs,
        &their_langs,
        my_intent.as_deref(),
        their_intent.as_deref(),
        cf_score,
    );

    // --- Geo score ---
    let my_loc = fetch_user_location(db, user_id).await?;
    let their_loc = fetch_user_location(db, target_user_id).await?;
    let distance_km = match (&my_loc, &their_loc) {
        (Some(a), Some(b)) => {
            match (a.latitude.zip(a.longitude), b.latitude.zip(b.longitude)) {
                (Some((alat, alng)), Some((blat, blng))) => {
                    Some(haversine_km(alat, alng, blat, blng))
                }
                _ => None,
            }
        }
        _ => None,
    };
    let geo_score = distance_km.map(|d| {
        let max_km = 100.0_f64;
        (1.0 - (d / max_km).min(1.0)).max(0.0)
    }).unwrap_or(0.5);

    // --- Shared interests list ---
    let my_set: std::collections::HashSet<&str> =
        my_interests.iter().map(|s| s.as_str()).collect();
    let shared_interests: Vec<&str> = their_interests
        .iter()
        .filter(|i| my_set.contains(i.as_str()))
        .map(|s| s.as_str())
        .collect();

    // --- Shared languages list ---
    let my_lang_set: std::collections::HashSet<&str> =
        my_langs.iter().map(|s| s.as_str()).collect();
    let shared_languages: Vec<&str> = their_langs
        .iter()
        .filter(|l| my_lang_set.contains(l.as_str()))
        .map(|s| s.as_str())
        .collect();

    // --- RL score (read-only, no training) ---
    let rl_score = {
        let mut ml = state.ml.write().await;
        let user_f = crate::ml::features::UserFeatures::from_db(db, user_id)
            .await
            .unwrap_or_else(|_| ml.feature_defaults.for_user(user_id));
        let cand_f = crate::ml::features::UserFeatures::from_db(db, target_user_id)
            .await
            .unwrap_or_else(|_| ml.feature_defaults.for_user(target_user_id));
        let state_vec = crate::ml::features::combine_features(&user_f, &cand_f);
        ml.rl_agent.score_candidate(user_id, &state_vec)
    };

    // --- Super like check ---
    let super_liked_me: bool = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM swipes WHERE from_user_id = $1 AND to_user_id = $2 AND action = 'superlike')"
    )
    .bind(target_user_id as i64)
    .bind(user_id as i64)
    .fetch_one(db)
    .await
    .unwrap_or(false);

    let super_like_boost = if super_liked_me { 0.15 } else { 0.0 };

    // --- Final blended compatibility score ---
    let compatibility = (0.55 * rl_score.clamp(0.0, 1.0)
        + 0.20 * geo_score
        + 0.20 * affinity.total
        + super_like_boost)
        .clamp(0.0, 1.0);

    // --- Human-readable insight labels ---
    let interest_label = if affinity.interest_overlap >= 0.6 {
        "Very high shared interests"
    } else if affinity.interest_overlap >= 0.3 {
        "Good interest overlap"
    } else if affinity.interest_overlap > 0.0 {
        "Some shared interests"
    } else {
        "Different interests"
    };

    let intent_label = match affinity.intent_alignment {
        x if x >= 0.9 => "Same relationship goals",
        x if x >= 0.4 => "Similar relationship goals",
        _ => "Different relationship goals",
    };

    let distance_label = distance_km.map(|d| {
        if d < 5.0 { "Very close — under 5 km".to_string() }
        else if d < 20.0 { format!("{:.0} km away", d) }
        else if d < 100.0 { format!("{:.0} km away", d) }
        else { format!("{:.0} km away — long distance", d) }
    });

    let compatibility_label = if compatibility >= 0.80 {
        "Exceptional match"
    } else if compatibility >= 0.65 {
        "Strong compatibility"
    } else if compatibility >= 0.50 {
        "Good potential"
    } else if compatibility >= 0.35 {
        "Some compatibility"
    } else {
        "Low compatibility"
    };

    // --- University match ---
    let uni_map = batch_lookup_university_full(db, &[user_id, target_user_id]).await?;
    let my_uni = uni_map.get(&user_id).map(|(name, _, _)| name.clone());
    let their_uni = uni_map.get(&target_user_id).map(|(name, _, _)| name.clone());
    let same_university = my_uni.is_some()
        && their_uni.is_some()
        && my_uni == their_uni;

    Ok(Json(json!({
        "compatibility_score": (compatibility * 100.0).round() as i32,
        "compatibility_label": compatibility_label,
        "breakdown": {
            "personality_match": {
                "score": (rl_score.clamp(0.0, 1.0) * 100.0).round() as i32,
                "label": if rl_score >= 0.7 { "Your profiles are highly aligned" }
                         else if rl_score >= 0.4 { "Moderate profile alignment" }
                         else { "Still learning your preferences" },
                "weight_pct": 55
            },
            "shared_interests": {
                "score": (affinity.interest_overlap * 100.0).round() as i32,
                "label": interest_label,
                "shared": shared_interests,
                "weight_pct": 10
            },
            "shared_languages": {
                "score": (affinity.language_overlap * 100.0).round() as i32,
                "label": if !shared_languages.is_empty() {
                    "You speak the same language"
                } else {
                    "Different languages"
                },
                "shared": shared_languages,
                "weight_pct": 5
            },
            "relationship_goals": {
                "score": (affinity.intent_alignment * 100.0).round() as i32,
                "label": intent_label,
                "weight_pct": 5
            },
            "proximity": {
                "score": (geo_score * 100.0).round() as i32,
                "label": distance_label.as_deref().unwrap_or("Location unknown"),
                "distance_km": distance_km,
                "weight_pct": 20
            },
            "super_like_boost": {
                "active": super_liked_me,
                "label": if super_liked_me { "They super liked you!" } else { "No super like" }
            }
        },
        "highlights": {
            "same_university": same_university,
            "university": their_uni,
            "cf_signal": cf_score > 0.1,
            "cf_label": if cf_score > 0.5 { "Many mutual connections liked them" }
                        else if cf_score > 0.1 { "Some mutual connections liked them" }
                        else { "" }
        }
    })))
}

// ============================================================================
// Message Requests — pending like-messages waiting for acceptance
// ============================================================================

/// GET /messages/requests — Returns all pending like-messages sent to the
/// authenticated user from people they haven't liked back yet.
pub async fn get_message_requests(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let db = state.read_pool();

    #[derive(sqlx::FromRow)]
    struct RequestRow {
        sender_id: i32,
        message_content: String,
        sent_at: chrono::NaiveDateTime,
        name: Option<String>,
        profile_photo_url: Option<String>,
        dob: Option<chrono::NaiveDate>,
        is_verified: Option<bool>,
        city: Option<String>,
    }

    let requests = sqlx::query_as::<_, RequestRow>(r#"
        SELECT
            m.sender_id,
            m.content AS message_content,
            m.created_at AS sent_at,
            u.name,
            u.profile_photo_url,
            u.dob,
            u.is_verified,
            l.city
        FROM messages m
        JOIN users u ON u.id = m.sender_id
        LEFT JOIN user_locations l ON l.user_id = m.sender_id
        WHERE m.receiver_id = $1
          AND m.message_type = 'like_message'
          AND NOT EXISTS (
              SELECT 1 FROM matches mx
              WHERE (
                  (mx.user1_id = $1 AND mx.user2_id = m.sender_id AND mx.user1_liked = TRUE)
                  OR
                  (mx.user1_id = m.sender_id AND mx.user2_id = $1 AND mx.user2_liked = TRUE)
              )
          )
        ORDER BY m.created_at DESC
        LIMIT 100
    "#)
    .bind(user_id)
    .fetch_all(db)
    .await?;

    let items: Vec<Value> = requests.into_iter().map(|r| {
        let age = r.dob.map(calculate_age);
        json!({
            "from_user_id": r.sender_id,
            "name": r.name,
            "age": age,
            "photo": r.profile_photo_url,
            "is_verified": r.is_verified.unwrap_or(false),
            "city": r.city,
            "message": r.message_content,
            "sent_at": r.sent_at,
        })
    }).collect();

    Ok(Json(json!({
        "requests": items,
        "count": items.len(),
    })))
}

/// POST /messages/requests/{from_user_id}/accept — Like them back, creating a match.
/// The like_message becomes the first message in the match conversation.
pub async fn accept_message_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(from_user_id): AxumPath<i32>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Like them back — reuse the same match logic
    let (user1_id, user2_id, is_user1) = if user_id < from_user_id {
        (user_id, from_user_id, true)
    } else {
        (from_user_id, user_id, false)
    };

    let existing = sqlx::query_as::<_, MatchCheckRow>(
        "SELECT id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match FROM matches WHERE user1_id = $1 AND user2_id = $2",
    )
    .bind(user1_id)
    .bind(user2_id)
    .fetch_optional(&state.db)
    .await?;

    let match_id = match existing {
        Some(m) => {
            let query = if is_user1 {
                "UPDATE matches SET user1_liked = TRUE, is_mutual_match = TRUE, updated_at = NOW() WHERE id = $1"
            } else {
                "UPDATE matches SET user2_liked = TRUE, is_mutual_match = TRUE, updated_at = NOW() WHERE id = $1"
            };
            sqlx::query(query).bind(&m.id).execute(&state.db).await?;
            m.id
        }
        None => {
            let new_id = Uuid::new_v4().to_string();
            let (u1_liked, u2_liked) = if is_user1 { (true, true) } else { (true, true) };
            sqlx::query(
                "INSERT INTO matches (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, status, created_at, updated_at)
                 VALUES ($1, $2, $3, $4, $5, TRUE, 'active', NOW(), NOW())",
            )
            .bind(&new_id).bind(user1_id).bind(user2_id)
            .bind(u1_liked).bind(u2_liked)
            .execute(&state.db).await?;
            new_id
        }
    };

    let _ = log_interaction_event(&state.db, user_id, from_user_id, "like", None, None, Some("message_request")).await;

    let ml = state.ml.clone();
    let db = state.db.clone();
    tokio::spawn(async move {
        let mut ml = ml.write().await;
        ml.record_swipe(&db, user_id, from_user_id, true).await;
        ml.record_swipe(&db, from_user_id, user_id, true).await;
    });

    Ok(Json(json!({
        "matched": true,
        "match_id": match_id,
        "message": "It's a match! The conversation is ready.",
    })))
}

/// POST /messages/requests/{from_user_id}/decline — Decline the request (soft pass).
pub async fn decline_message_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(from_user_id): AxumPath<i32>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Mark the like_message as declined so it doesn't reappear
    sqlx::query(
        "UPDATE messages SET message_type = 'like_message_declined', updated_at = NOW()
         WHERE sender_id = $1 AND receiver_id = $2 AND message_type = 'like_message'",
    )
    .bind(from_user_id)
    .bind(user_id)
    .execute(&state.db)
    .await?;

    // Record as pass for ML
    let ml = state.ml.clone();
    let db = state.db.clone();
    tokio::spawn(async move {
        let mut ml = ml.write().await;
        ml.record_swipe(&db, user_id, from_user_id, false).await;
    });

    Ok(Json(json!({ "declined": true })))
}

// ============================================================================
// Message Request — Reply without matching (soft conversation)
// ============================================================================

/// POST /messages/requests/{from_user_id}/reply
/// User B replies to User A's message request WITHOUT creating a match.
/// User A sees the reply but cannot see User B's full profile yet.
/// Once User A likes back, a real match is created automatically.
pub async fn reply_message_request(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(from_user_id): AxumPath<i32>,
    Json(payload): Json<serde_json::Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let reply_text = payload.get("message")
        .and_then(|v| v.as_str())
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| AppError::bad_request("message is required"))?;

    if reply_text.len() > 300 {
        return Err(AppError::bad_request("message must be under 300 characters"));
    }

    // Verify the original request exists
    let request_exists = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM messages WHERE sender_id = $1 AND receiver_id = $2 AND message_type = 'like_message')"
    )
    .bind(from_user_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    if !request_exists {
        return Err(AppError::not_found("Message request not found"));
    }

    // Use a deterministic thread_id for the pre-match conversation
    // Format: req_{smaller_id}_{larger_id}
    let (a, b) = if from_user_id < user_id { (from_user_id, user_id) } else { (user_id, from_user_id) };
    let thread_id = format!("req_{}_{}", a, b);

    // Store the reply as like_message_reply — no match record created
    let reply_msg_id = sqlx::query_scalar::<_, i64>(
        r#"INSERT INTO messages (match_id, sender_id, receiver_id, content, message_type, is_read, created_at)
           VALUES ($1, $2, $3, $4, 'like_message_reply', FALSE, NOW()) RETURNING id"#,
    )
    .bind(&thread_id)
    .bind(user_id)
    .bind(from_user_id)
    .bind(reply_text)
    .fetch_one(&state.db)
    .await?;

    // Auto-queue reply for LLM labeling
    auto_queue_for_labeling(state.db.clone(), state.config.llm_enabled, "message", reply_msg_id, 5);

    // Mark original like_message as replied so it shows differently in inbox
    sqlx::query(
        "UPDATE messages SET message_type = 'like_message_replied' WHERE sender_id = $1 AND receiver_id = $2 AND message_type = 'like_message'"
    )
    .bind(from_user_id)
    .bind(user_id)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({
        "replied": true,
        "thread_id": thread_id,
        "info": "Reply sent. Their profile will be fully visible once you match.",
    })))
}

/// GET /messages/requests/sent — User A sees replies from people they sent requests to.
/// Shows the pre-match conversation thread with limited profile info.
pub async fn get_sent_message_requests(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let db = state.read_pool();

    #[derive(sqlx::FromRow)]
    struct SentRow {
        receiver_id: i32,
        original_message: String,
        sent_at: chrono::NaiveDateTime,
        reply_content: Option<String>,
        replied_at: Option<chrono::NaiveDateTime>,
        name: Option<String>,
        // profile photo hidden until match — only show first name and blurred indicator
        is_verified: Option<bool>,
    }

    let sent = sqlx::query_as::<_, SentRow>(r#"
        SELECT
            m.receiver_id,
            m.content AS original_message,
            m.created_at AS sent_at,
            r.content AS reply_content,
            r.created_at AS replied_at,
            u.name,
            u.is_verified
        FROM messages m
        JOIN users u ON u.id = m.receiver_id
        LEFT JOIN messages r ON r.sender_id = m.receiver_id
            AND r.receiver_id = m.sender_id
            AND r.message_type = 'like_message_reply'
        WHERE m.sender_id = $1
          AND m.message_type IN ('like_message', 'like_message_replied')
        ORDER BY COALESCE(r.created_at, m.created_at) DESC
        LIMIT 100
    "#)
    .bind(user_id)
    .fetch_all(db)
    .await?;

    let items: Vec<Value> = sent.into_iter().map(|r| {
        let has_reply = r.reply_content.is_some();
        // Only show first name to preserve privacy until match
        let display_name = r.name.as_deref()
            .and_then(|n| n.split_whitespace().next())
            .map(|n| n.to_string());

        json!({
            "to_user_id": r.receiver_id,
            "display_name": display_name,   // first name only
            "photo_hidden": true,           // full profile hidden until match
            "is_verified": r.is_verified.unwrap_or(false),
            "your_message": r.original_message,
            "sent_at": r.sent_at,
            "has_reply": has_reply,
            "reply": r.reply_content,
            "replied_at": r.replied_at,
            "status": if has_reply { "replied" } else { "pending" },
        })
    }).collect();

    Ok(Json(json!({
        "sent_requests": items,
        "count": items.len(),
    })))
}

/// POST /messages/requests/{from_user_id}/like-back
/// User A likes back after seeing a reply — creates the full match.
/// The pre-match thread messages are migrated to the real match conversation.
pub async fn like_back_after_reply(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(from_user_id): AxumPath<i32>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Create the match (same logic as accept_message_request)
    let (user1_id, user2_id, is_user1) = if user_id < from_user_id {
        (user_id, from_user_id, true)
    } else {
        (from_user_id, user_id, false)
    };

    let existing = sqlx::query_as::<_, MatchCheckRow>(
        "SELECT id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match FROM matches WHERE user1_id = $1 AND user2_id = $2",
    )
    .bind(user1_id)
    .bind(user2_id)
    .fetch_optional(&state.db)
    .await?;

    let match_id = match existing {
        Some(m) => {
            let query = if is_user1 {
                "UPDATE matches SET user1_liked = TRUE, is_mutual_match = TRUE, updated_at = NOW() WHERE id = $1"
            } else {
                "UPDATE matches SET user2_liked = TRUE, is_mutual_match = TRUE, updated_at = NOW() WHERE id = $1"
            };
            sqlx::query(query).bind(&m.id).execute(&state.db).await?;
            m.id
        }
        None => {
            let new_id = Uuid::new_v4().to_string();
            sqlx::query(
                "INSERT INTO matches (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, status, created_at, updated_at)
                 VALUES ($1, $2, $3, TRUE, TRUE, TRUE, 'active', NOW(), NOW())",
            )
            .bind(&new_id).bind(user1_id).bind(user2_id)
            .execute(&state.db).await?;
            new_id
        }
    };

    // Migrate pre-match thread to real match_id so the conversation history is preserved
    let (a, b) = if user_id < from_user_id { (user_id, from_user_id) } else { (from_user_id, user_id) };
    let thread_id = format!("req_{}_{}", a, b);

    let _ = sqlx::query(
        "UPDATE messages SET match_id = $1, message_type = 'text' WHERE match_id = $2"
    )
    .bind(&match_id)
    .bind(&thread_id)
    .execute(&state.db)
    .await;

    // Also migrate the original like_message
    let _ = sqlx::query(
        "UPDATE messages SET match_id = $1, message_type = 'text' WHERE sender_id = $2 AND receiver_id = $3 AND message_type IN ('like_message', 'like_message_replied')"
    )
    .bind(&match_id)
    .bind(from_user_id)
    .bind(user_id)
    .execute(&state.db)
    .await;

    let _ = log_interaction_event(&state.db, user_id, from_user_id, "like", None, None, Some("message_request_reply")).await;

    let ml = state.ml.clone();
    let db = state.db.clone();
    tokio::spawn(async move {
        let mut ml = ml.write().await;
        ml.record_swipe(&db, user_id, from_user_id, true).await;
        ml.record_swipe(&db, from_user_id, user_id, true).await;
    });

    Ok(Json(json!({
        "matched": true,
        "match_id": match_id,
        "message": "It's a match! Full conversation history preserved.",
        "conversation_migrated": true,
    })))
}

// =============================================================================
// Graph Abstraction API Handlers
// Netflix-inspired property graph: FoF, university network, reel collab, fraud
// =============================================================================

use crate::services::graph::schema::{EdgeType, NodeType};

/// GET /graph/fof?limit=20
/// Friend-of-Friend recommendations via 2-hop matched_with traversal
pub async fn graph_fof(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let limit = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(20usize);

    let recs = state.graph.friend_of_friend(&user_id.to_string(), limit).await?;
    let count = recs.len();
    Ok(Json(json!({
        "recommendations": recs,
        "count": count,
        "algorithm": "friend_of_friend_2hop"
    })))
}

/// GET /graph/university?limit=50
/// University network — users at the same university (2-hop via University node)
pub async fn graph_university_network(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let limit = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(50usize);

    let recs = state.graph.university_network(&user_id.to_string(), limit).await?;
    let count = recs.len();
    Ok(Json(json!({
        "recommendations": recs,
        "count": count,
        "algorithm": "university_network_2hop"
    })))
}

/// GET /graph/reel-collaborators?limit=30
/// Users with similar reel taste (2-hop via Reel viewed edges)
pub async fn graph_reel_collaborators(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let limit = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(30usize);

    let recs = state.graph.reel_collaborators(&user_id.to_string(), limit).await?;
    let count = recs.len();
    Ok(Json(json!({
        "recommendations": recs,
        "count": count,
        "algorithm": "reel_collaborative_filtering_2hop"
    })))
}

/// GET /graph/proximity/{target_user_id}
/// Count mutual connections between current user and target user
pub async fn graph_proximity(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(target_user_id): AxumPath<i64>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let mutual = state.graph.social_proximity(
        &user_id.to_string(),
        &target_user_id.to_string(),
    ).await?;

    Ok(Json(json!({
        "user_id": user_id,
        "target_user_id": target_user_id,
        "mutual_connections": mutual
    })))
}

/// GET /graph/fraud/{user_id}
/// Detect multi-account fraud via shared device graph traversal
pub async fn graph_fraud_check(
    State(state): State<AppState>,
    headers: HeaderMap,
    AxumPath(target_user_id): AxumPath<i64>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _caller = decode_access_token(&token, &state.config.secret_key)?;

    let result = state.graph.fraud_check(&target_user_id.to_string()).await?;

    Ok(Json(json!({
        "fraud_analysis": result,
        "traversal": "shared_device_2hop"
    })))
}

/// GET /graph/stats
/// Graph stats: node counts, edge type distribution
pub async fn graph_stats(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _user_id = decode_access_token(&token, &state.config.secret_key)?;

    let stats = state.graph.stats().await?;

    Ok(Json(json!({
        "stats": stats,
        "layer": "nava_graph_abstraction_v1"
    })))
}

/// POST /graph/edge
/// Write a validated edge into the graph
/// Body: { from_type, from_id, edge_type, to_type, to_id, properties? }
pub async fn graph_write_edge(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let _user_id = decode_access_token(&token, &state.config.secret_key)?;

    let from_type  = NodeType::from_str(body["from_type"].as_str().unwrap_or("user"));
    let from_id    = body["from_id"].as_str().ok_or_else(|| AppError::bad_request("from_id required"))?.to_string();
    let edge_type  = EdgeType::from_str(body["edge_type"].as_str().unwrap_or("liked"));
    let to_type    = NodeType::from_str(body["to_type"].as_str().unwrap_or("user"));
    let to_id      = body["to_id"].as_str().ok_or_else(|| AppError::bad_request("to_id required"))?.to_string();
    let properties = body.get("properties").cloned();

    state.graph.write_edge(from_type, &from_id, edge_type, to_type, &to_id, properties).await?;

    Ok(Json(json!({ "ok": true, "edge": format!("{} -[{}]-> {}", from_id, edge_type, to_id) })))
}

// ============================================================================
// App Bootstrap & Badges — reduce cold-start round-trips
// ============================================================================

/// GET /app/bootstrap — Single call returns everything needed on app launch.
/// Runs profile, matches, badge counts, and preferences queries in parallel
/// via `tokio::join!` so total latency ≈ slowest single query (~20 ms).
pub async fn app_bootstrap(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let read_db = state.read_pool();

    // ── parallel fan-out ────────────────────────────────────────────────
    let (user_res, matches_res, unread_messages, unread_likes, new_matches, prefs_res) = tokio::join!(
        // 1. User profile
        fetch_user_by_id(read_db, user_id),
        // 2. Recent mutual matches (last 50)
        sqlx::query_as::<_, MatchRow>(
            r#"
            SELECT id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match,
                   ai_compatibility_score, visual_compatibility_score, match_reason,
                   messages_count, voice_messages_count, last_message_at, can_send_text,
                   status, blocked_by_user_id, created_at, updated_at
            FROM matches
            WHERE (user1_id = $1 OR user2_id = $1)
              AND is_mutual_match = TRUE
              AND status = 'active'
            ORDER BY last_message_at DESC NULLS LAST, created_at DESC
            LIMIT 50
            "#,
        )
        .bind(user_id)
        .fetch_all(read_db),
        // 3. Unread message count
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM messages WHERE receiver_id = $1 AND is_read = FALSE"
        )
        .bind(user_id)
        .fetch_one(read_db),
        // 4. Unread likes (pending incoming likes)
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM matches WHERE user2_id = $1 AND user2_liked IS NULL AND user1_liked = TRUE"
        )
        .bind(user_id)
        .fetch_one(read_db),
        // 5. New mutual matches in the last 24 h
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM matches WHERE (user1_id = $1 OR user2_id = $1) AND is_mutual_match = TRUE AND created_at > NOW() - INTERVAL '24 hours'"
        )
        .bind(user_id)
        .fetch_one(read_db),
        // 6. User preferences
        fetch_user_preferences(read_db, user_id)
    );

    // ── unwrap results ──────────────────────────────────────────────────
    let user = user_res?
        .ok_or_else(|| AppError::not_found("User not found"))?;

    let matches_rows = matches_res?;

    // Build lightweight match list (id, other user name/photo, last message, unread)
    let mut matches_out = Vec::with_capacity(matches_rows.len());
    for m in &matches_rows {
        let other_id = if m.user1_id == user_id { m.user2_id } else { m.user1_id };
        if let Some(other_user) = fetch_user_by_id(read_db, other_id).await? {
            matches_out.push(json!({
                "match_id": m.id,
                "is_mutual": true,
                "matched_at": m.created_at.map(format_datetime),
                "can_send_text": m.can_send_text.unwrap_or(false),
                "messages_count": m.messages_count.unwrap_or(0),
                "voice_messages_count": m.voice_messages_count.unwrap_or(0),
                "last_message_at": m.last_message_at.map(format_datetime),
                "other_user": {
                    "id": other_user.id,
                    "name": other_user.name,
                    "photos": get_user_photos(&other_user),
                    "is_verified": other_user.is_verified.unwrap_or(false),
                }
            }));
        }
    }

    // Profile payload (essential fields only)
    let profile = json!({
        "id": user.id,
        "name": user.name,
        "display_name": user.display_name,
        "dob": user.dob.map(format_date),
        "age": user.dob.map(calculate_age),
        "gender": user.gender,
        "bio": user.bio,
        "location": user.location_text,
        "photos": get_user_photos(&user),
        "is_profile_complete": user.is_profile_complete,
        "profile_completion": compute_profile_completion(&user),
        "is_verified": user.is_verified,
        "is_student_verified": user.is_student_verified,
    });

    let preferences = prefs_res?.map(|pref| json!({
        "min_age": pref.min_age,
        "max_age": pref.max_age,
        "preferred_genders": pref.preferred_genders,
        "max_distance_km": pref.max_distance,
        "only_verified": pref.only_verified,
        "only_students": pref.only_students,
        "preferred_locations": pref.preferred_locations,
    }));

    Ok(Json(json!({
        "profile": profile,
        "matches": matches_out,
        "badges": {
            "unread_messages": unread_messages.unwrap_or(0),
            "unread_likes": unread_likes.unwrap_or(0),
            "new_matches_24h": new_matches.unwrap_or(0),
        },
        "preferences": preferences,
    })))
}

/// GET /app/badges — Lightweight unread counts for app badges.
/// Called on app foreground / tab switches; must be very fast.
pub async fn app_badges(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let read_db = state.read_pool();

    let (unread_messages, unread_reel_messages, unread_likes, new_matches) = tokio::join!(
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM messages WHERE receiver_id = $1 AND is_read = FALSE"
        )
        .bind(user_id)
        .fetch_one(read_db),
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM reel_messages WHERE receiver_id = $1 AND is_read = FALSE"
        )
        .bind(user_id)
        .fetch_one(read_db),
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM matches WHERE user2_id = $1 AND user2_liked IS NULL AND user1_liked = TRUE"
        )
        .bind(user_id)
        .fetch_one(read_db),
        sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM matches WHERE (user1_id = $1 OR user2_id = $1) AND is_mutual_match = TRUE AND created_at > NOW() - INTERVAL '24 hours'"
        )
        .bind(user_id)
        .fetch_one(read_db),
    );

    let msgs = unread_messages.unwrap_or(0);
    let reel_msgs = unread_reel_messages.unwrap_or(0);
    let likes = unread_likes.unwrap_or(0);

    Ok(Json(json!({
        "unread_messages": msgs,
        "unread_reel_messages": reel_msgs,
        "unread_likes": likes,
        "new_matches_24h": new_matches.unwrap_or(0),
        "total": msgs + reel_msgs + likes,
    })))
}

// ============================================================================
// Knowledge Graph: Session + Location + Behavior tracking
// ============================================================================

/// POST /sessions/start — Start app session, capture device + screen metrics
/// Body: { device_id, device_type, device_model, os_version, app_version,
///         screen_width, screen_height, network_type, latitude, longitude, city }
pub async fn start_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let session_id = sqlx::query_scalar::<_, uuid::Uuid>(
        r#"INSERT INTO user_sessions
           (user_id, device_id, device_type, device_model, os_version, app_version,
            screen_width, screen_height, network_type, latitude, longitude, city)
           VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
           RETURNING id"#
    )
    .bind(user_id)
    .bind(payload.get("device_id").and_then(|v| v.as_str()))
    .bind(payload.get("device_type").and_then(|v| v.as_str()))
    .bind(payload.get("device_model").and_then(|v| v.as_str()))
    .bind(payload.get("os_version").and_then(|v| v.as_str()))
    .bind(payload.get("app_version").and_then(|v| v.as_str()))
    .bind(payload.get("screen_width").and_then(|v| v.as_i64()).map(|v| v as i32))
    .bind(payload.get("screen_height").and_then(|v| v.as_i64()).map(|v| v as i32))
    .bind(payload.get("network_type").and_then(|v| v.as_str()))
    .bind(payload.get("latitude").and_then(|v| v.as_f64()))
    .bind(payload.get("longitude").and_then(|v| v.as_f64()))
    .bind(payload.get("city").and_then(|v| v.as_str()))
    .fetch_one(&state.db)
    .await?;

    // Also record location snapshot in history (fire-and-forget)
    if let (Some(lat), Some(lon)) = (
        payload.get("latitude").and_then(|v| v.as_f64()),
        payload.get("longitude").and_then(|v| v.as_f64()),
    ) {
        let db = state.db.clone();
        let city = payload.get("city").and_then(|v| v.as_str()).map(String::from);
        tokio::spawn(async move {
            let _ = sqlx::query(
                "INSERT INTO location_history (user_id, latitude, longitude, city, source, session_id) VALUES ($1, $2, $3, $4, 'session_start', $5)"
            )
            .bind(user_id).bind(lat).bind(lon).bind(city).bind(session_id)
            .execute(&db).await;
        });
    }

    Ok(Json(json!({ "session_id": session_id })))
}

/// POST /sessions/heartbeat — Keep session alive, update last_heartbeat_at
/// Body: { session_id }
pub async fn session_heartbeat(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let session_id: uuid::Uuid = payload.get("session_id")
        .and_then(|v| v.as_str())
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .ok_or_else(|| AppError::bad_request("session_id required"))?;

    sqlx::query(
        "UPDATE user_sessions SET last_heartbeat_at = NOW() WHERE id = $1 AND user_id = $2 AND ended_at IS NULL"
    )
    .bind(session_id).bind(user_id)
    .execute(&state.db).await?;

    Ok(Json(json!({ "ok": true })))
}

/// POST /sessions/end — Close session
/// Body: { session_id }
pub async fn end_session(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let session_id: uuid::Uuid = payload.get("session_id")
        .and_then(|v| v.as_str())
        .and_then(|s| uuid::Uuid::parse_str(s).ok())
        .ok_or_else(|| AppError::bad_request("session_id required"))?;

    sqlx::query(
        "UPDATE user_sessions SET ended_at = NOW() WHERE id = $1 AND user_id = $2 AND ended_at IS NULL"
    )
    .bind(session_id).bind(user_id)
    .execute(&state.db).await?;

    Ok(Json(json!({ "ok": true })))
}

/// POST /location/track — Append location to history + update current location
/// Body: { latitude, longitude, accuracy_m?, city?, country?, source? }
pub async fn track_location(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let lat_raw = payload.get("latitude").and_then(|v| v.as_f64())
        .ok_or_else(|| AppError::bad_request("latitude required"))?;
    let lon_raw = payload.get("longitude").and_then(|v| v.as_f64())
        .ok_or_else(|| AppError::bad_request("longitude required"))?;
    // Validate coordinates are plausible
    if !(-90.0..=90.0).contains(&lat_raw) || !(-180.0..=180.0).contains(&lon_raw)
        || (lat_raw == 0.0 && lon_raw == 0.0) {
        return Err(AppError::bad_request("invalid coordinates"));
    }
    // Privacy guardrail: store only ~100m precision in history trail (3 decimal places).
    // Current location table keeps the original precision for distance calc.
    let lat = (lat_raw * 1000.0).round() / 1000.0;
    let lon = (lon_raw * 1000.0).round() / 1000.0;
    state.metrics.location_precision_reduced.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    state.metrics.location_track_ingested.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    let accuracy = payload.get("accuracy_m").and_then(|v| v.as_f64());
    let city = payload.get("city").and_then(|v| v.as_str());
    let country = payload.get("country").and_then(|v| v.as_str());
    let source = payload.get("source").and_then(|v| v.as_str()).unwrap_or("gps");

    // Append to history (async)
    {
        let db = state.db.clone();
        let city_own = city.map(String::from);
        let country_own = country.map(String::from);
        let source_own = source.to_string();
        tokio::spawn(async move {
            let _ = sqlx::query(
                "INSERT INTO location_history (user_id, latitude, longitude, accuracy_m, city, country, source) VALUES ($1, $2, $3, $4, $5, $6, $7)"
            )
            .bind(user_id).bind(lat).bind(lon).bind(accuracy).bind(city_own).bind(country_own).bind(source_own)
            .execute(&db).await;
        });
    }

    // Update current location (upsert)
    sqlx::query(
        r#"INSERT INTO user_locations (user_id, latitude, longitude, accuracy, city, country, update_source, last_updated)
           VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
           ON CONFLICT (user_id) DO UPDATE SET
               latitude = EXCLUDED.latitude, longitude = EXCLUDED.longitude,
               accuracy = EXCLUDED.accuracy, city = COALESCE(EXCLUDED.city, user_locations.city),
               country = COALESCE(EXCLUDED.country, user_locations.country),
               update_source = EXCLUDED.update_source, last_updated = NOW()"#
    )
    .bind(user_id).bind(lat_raw).bind(lon_raw).bind(accuracy).bind(city).bind(country).bind(source)
    .execute(&state.db).await?;

    // Invalidate location LRU cache
    state.location_cache.write().await.pop(&user_id);

    Ok(Json(json!({ "updated": true })))
}

/// POST /admin/graph/replay — Rebuild graph edges from interaction_events.
/// Query param: since_days (int, optional — omit for full rebuild)
pub async fn admin_replay_graph(
    State(state): State<AppState>,
    _admin: AdminClaims,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let since_days = params.get("since_days").and_then(|v| v.parse::<i32>().ok());
    let report = crate::services::graph_replay::replay_user_edges(&state.db, since_days)
        .await
        .map_err(|e| AppError::internal(format!("Replay failed: {e}")))?;
    tracing::info!(?report, since_days, "Graph replay complete");
    Ok(Json(serde_json::to_value(report).unwrap_or(json!({}))))
}

/// GET /admin/data-quality — Reports drift between operational and derived state.
pub async fn admin_data_quality(
    State(state): State<AppState>,
    _admin: AdminClaims,
) -> Result<Json<Value>, AppError> {
    let db = state.read_pool();

    // Mutual matches without corresponding graph edges
    let orphaned_matches = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*) FROM matches m
           WHERE m.is_mutual_match = TRUE
             AND NOT EXISTS (
                SELECT 1 FROM graph_edge_links_fwd g
                WHERE g.edge_type = 'matched_with'
                  AND g.from_type = 'user' AND g.to_type = 'user'
                  AND g.from_id = m.user1_id::text AND g.to_id = m.user2_id::text
             )"#
    ).fetch_one(db).await.unwrap_or(0);

    // Users with recent events but stale (or missing) behavior profiles
    let stale_behavior_profiles = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(DISTINCT ie.user_id)
           FROM interaction_events ie
           LEFT JOIN user_behavior_profile bp ON bp.user_id = ie.user_id
           WHERE ie.created_at > NOW() - INTERVAL '7 days'
             AND (bp.last_computed_at IS NULL OR bp.last_computed_at < NOW() - INTERVAL '6 hours')"#
    ).fetch_one(db).await.unwrap_or(0);

    // Session durations > 24h = likely bad client shutdown
    let impossible_sessions = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*) FROM user_sessions
           WHERE ended_at IS NULL AND started_at < NOW() - INTERVAL '24 hours'"#
    ).fetch_one(db).await.unwrap_or(0);

    // Invalid GPS: out of range or exact zero
    let bad_gps = sqlx::query_scalar::<_, i64>(
        r#"SELECT COUNT(*) FROM location_history
           WHERE created_at > NOW() - INTERVAL '7 days'
             AND (latitude NOT BETWEEN -90 AND 90 OR longitude NOT BETWEEN -180 AND 180
                  OR (latitude = 0 AND longitude = 0))"#
    ).fetch_one(db).await.unwrap_or(0);

    // Sessions missing device_type (client instrumentation gap)
    let missing_device_type = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM user_sessions WHERE started_at > NOW() - INTERVAL '7 days' AND device_type IS NULL"
    ).fetch_one(db).await.unwrap_or(0);

    Ok(Json(json!({
        "orphaned_mutual_matches": orphaned_matches,
        "stale_behavior_profiles": stale_behavior_profiles,
        "impossible_sessions_over_24h": impossible_sessions,
        "bad_gps_last_7d": bad_gps,
        "sessions_missing_device_type_7d": missing_device_type,
    })))
}

/// GET /behavior/me — Get computed behavior profile for current user
pub async fn get_my_behavior(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let row: Option<(Option<f64>, Option<f64>, Option<i32>, Option<i16>, Option<i16>, Option<f64>, Option<String>)> =
        sqlx::query_as(
            "SELECT swipes_per_min_7d, like_rate_7d, avg_session_duration_sec, peak_hour_utc, peak_day_of_week, sessions_per_day_7d, primary_city FROM user_behavior_profile WHERE user_id = $1"
        ).bind(user_id).fetch_optional(state.read_pool()).await?;

    if let Some((spm, lr, dur, hour, dow, spd, city)) = row {
        Ok(Json(json!({
            "swipes_per_min_7d": spm,
            "like_rate_7d": lr,
            "avg_session_duration_sec": dur,
            "peak_hour_utc": hour,
            "peak_day_of_week": dow,
            "sessions_per_day_7d": spd,
            "primary_city": city,
        })))
    } else {
        Ok(Json(json!({ "message": "Profile not yet computed — need at least 7 days of activity" })))
    }
}
