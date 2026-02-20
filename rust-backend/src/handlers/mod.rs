// Graph-powered endpoints module
pub mod graph_handlers;
// Web payments module (Razorpay + Stripe)
pub mod payments;
// Ads monetization module
pub mod ads;
// Ambassador program module
pub mod ambassador;

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
    let mut photos: Vec<Option<PhotoInput>> = vec![None, None, None];

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

                if let Some(ref v) = vision {
                    let photo = analyze_photo_bytes(v.clone(), bytes).await?;
                    if let Some(ref analysis) = photo.analysis {
                        if analysis.inappropriate_content {
                            return Err(AppError::bad_request(format!(
                                "Photo {} contains inappropriate content",
                                idx + 1
                            )));
                        }
                    }
                    photos[idx] = Some(photo);
                } else {
                    // Vision disabled - just load image
                    let image = task::spawn_blocking(move || {
                        image::load_from_memory(&bytes)
                            .map_err(|_| AppError::bad_request("Invalid image"))
                    })
                    .await
                    .map_err(|_| AppError::internal("Image task failed"))??;
                    photos[idx] = Some(PhotoInput {
                        image,
                        analysis: None,
                    });
                }
            }
            _ => {}
        }
    }

    let name = name.ok_or_else(|| AppError::bad_request("name is required"))?;
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

    let mut photo_inputs = Vec::new();
    for (idx, entry) in photos.into_iter().enumerate() {
        match entry {
            Some(photo) => photo_inputs.push((idx + 1, photo)),
            None => {
                return Err(AppError::bad_request(format!(
                    "profile_photo_{} is required",
                    idx + 1
                )))
            }
        }
    }

    let upload_dir = &state.config.upload_dir;
    fs::create_dir_all(upload_dir)
        .await
        .map_err(|_| AppError::internal("Failed to create upload directory"))?;

    let mut saved_paths = Vec::new();
    let mut insights = Vec::new();
    let mut avg_attractiveness: Option<f64> = None;
    let mut attractiveness_sum = 0.0;
    let mut attractiveness_count = 0;

    for (idx, photo) in photo_inputs.into_iter() {
        let filename = format!(
            "{}_photo_{}_{}_{}.jpg",
            user_id,
            idx,
            Utc::now().timestamp(),
            Uuid::new_v4()
        );
        let path = Path::new(upload_dir).join(&filename);
        let jpeg_bytes = encode_jpeg(&photo.image)
            .map_err(|_| AppError::internal("Failed to encode image"))?;
        if let Err(err) = fs::write(&path, jpeg_bytes).await {
            cleanup_files(&saved_paths).await;
            return Err(AppError::internal(format!(
                "Failed to save photo: {err}"
            )));
        }
        saved_paths.push(path.to_string_lossy().to_string());

        if let Some(ref analysis) = photo.analysis {
            attractiveness_sum += analysis.attractiveness_score as f64;
            attractiveness_count += 1;
            insights.push(json!({
                "quality": analysis.quality_score,
                "smile_detected": analysis.smile_intensity > 0.5,
                "authenticity": analysis.authenticity_score,
                "attractiveness": analysis.attractiveness_score,
            }));
        } else {
            insights.push(json!({
                "quality": null,
                "smile_detected": null,
                "authenticity": null,
            }));
        }
    }

    if attractiveness_count > 0 {
        avg_attractiveness = Some(attractiveness_sum / attractiveness_count as f64);
    }

    let csv_paths = saved_paths.join(",");
    let photos_json = sqlx::types::Json(saved_paths.clone());

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
            is_profile_complete = TRUE,
            updated_at = NOW()
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
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        cleanup_files(&saved_paths).await;
        return Err(AppError::not_found("User not found"));
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
    .fetch_optional(&state.db)
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

    let user = fetch_user_by_id(&state.db, user_id).await?;
    let user = user.ok_or_else(|| AppError::not_found("User not found"))?;

    let (preferences, location, subscriptions, spots) = tokio::try_join!(
        fetch_user_preferences(&state.db, user_id),
        fetch_user_location(&state.db, user_id),
        fetch_user_subscriptions(&state.db, user_id),
        fetch_user_spots(&state.db, user_id, 10),
    )?;

    let profile = json!({
        "id": user.id,
        "phone_number": user.phone_number,
        "email": user.email,
        "name": user.name,
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
            "pass_type": sub.pass_type,
            "start_date": sub.start_date.map(format_datetime),
            "end_date": sub.end_date.map(format_datetime),
            "status": sub.status,
            "is_active": sub.is_active,
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

    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<i32>().ok())
        .unwrap_or(state.config.discover_limit);

    // Get user and preferences
    let user = fetch_user_by_id(&state.db, user_id)
        .await?
        .ok_or_else(|| AppError::not_found("User not found"))?;

    let prefs = fetch_user_preferences(&state.db, user_id).await?;
    let user_loc = fetch_user_location(&state.db, user_id).await?;

    // Build discovery query with filters
    let min_age = prefs.as_ref().and_then(|p| p.min_age).unwrap_or(18);
    let max_age = prefs.as_ref().and_then(|p| p.max_age).unwrap_or(100);
    let only_verified = prefs.as_ref().and_then(|p| p.only_verified).unwrap_or(false);
    let max_distance = prefs.as_ref().and_then(|p| p.max_distance).unwrap_or(state.config.default_max_distance_km);

    // Get users who haven't been liked/passed by this user
    let candidates = sqlx::query_as::<_, DiscoverUserRow>(
        r#"
        SELECT u.id, u.name, u.dob, u.gender, u.bio, u.profile_photo_url, u.profile_photos,
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
    .fetch_all(&state.db)
    .await?;

    let profiles: Vec<DiscoverProfile> = candidates
        .into_iter()
        .map(|c| {
            let distance_km = if let (Some(ul), Some(lat), Some(lon)) = (&user_loc, c.latitude, c.longitude) {
                ul.latitude.zip(ul.longitude).map(|(ulat, ulon)| {
                    haversine_km(ulat, ulon, lat, lon)
                })
            } else {
                None
            };

            let photos = get_photos_from_row(&c);
            DiscoverProfile {
                id: c.id,
                name: c.name,
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
                compatibility_score: c.attractiveness_score,
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

    // Log impression events for ML
    let slate_id = Uuid::new_v4().to_string();
    for (rank, profile) in profiles.iter().enumerate() {
        let _ = log_interaction_event(
            &state.db,
            user_id,
            profile.id,
            "impression",
            Some(&slate_id),
            Some(rank as i32),
            Some("discover"),
        )
        .await;
    }

    Ok(Json(json!({
        "profiles": profiles,
        "slate_id": slate_id,
    })))
}

pub async fn like_user(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<LikeRequest>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;
    let target_id = payload.target_user_id;

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

    // Determine user order (lower ID is user1)
    let (user1_id, user2_id, is_user1) = if user_id < target_id {
        (user_id, target_id, true)
    } else {
        (target_id, user_id, false)
    };

    // Check for existing match record
    let existing = sqlx::query_as::<_, MatchCheckRow>(
        "SELECT id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match FROM matches WHERE user1_id = $1 AND user2_id = $2",
    )
    .bind(user1_id)
    .bind(user2_id)
    .fetch_optional(&state.db)
    .await?;

    let (match_id, is_mutual) = match existing {
        Some(m) => {
            // Update existing match
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
            // Create new match record
            let new_id = Uuid::new_v4().to_string();
            let (u1_liked, u2_liked) = if is_user1 { (true, false) } else { (false, true) };

            sqlx::query(
                r#"
                INSERT INTO matches (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, status, created_at, updated_at)
                VALUES ($1, $2, $3, $4, $5, FALSE, 'active', NOW(), NOW())
                "#,
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

    // Log like event
    let _ = log_interaction_event(&state.db, user_id, target_id, "like", None, None, Some("discover")).await;

    Ok(Json(json!({
        "message": if is_mutual { "It's a match!" } else { "Like sent" },
        "match_id": match_id,
        "is_mutual": is_mutual,
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

    // Log pass event (for ML training - negative signal)
    let _ = log_interaction_event(&state.db, user_id, target_id, "pass", None, None, Some("discover")).await;

    // Determine user order
    let (user1_id, user2_id, is_user1) = if user_id < target_id {
        (user_id, target_id, true)
    } else {
        (target_id, user_id, false)
    };

    // Check for existing match record and mark as passed
    let existing = sqlx::query_scalar::<_, String>(
        "SELECT id FROM matches WHERE user1_id = $1 AND user2_id = $2",
    )
    .bind(user1_id)
    .bind(user2_id)
    .fetch_optional(&state.db)
    .await?;

    if let Some(match_id) = existing {
        let query = if is_user1 {
            "UPDATE matches SET user1_liked = FALSE, updated_at = NOW() WHERE id = $1"
        } else {
            "UPDATE matches SET user2_liked = FALSE, updated_at = NOW() WHERE id = $1"
        };
        sqlx::query(query)
            .bind(&match_id)
            .execute(&state.db)
            .await?;
    } else {
        // Create record to track the pass
        let new_id = Uuid::new_v4().to_string();
        let (u1_liked, u2_liked): (Option<bool>, Option<bool>) = if is_user1 {
            (Some(false), None)
        } else {
            (None, Some(false))
        };

        sqlx::query(
            r#"
            INSERT INTO matches (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, status, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, FALSE, 'active', NOW(), NOW())
            "#,
        )
        .bind(&new_id)
        .bind(user1_id)
        .bind(user2_id)
        .bind(u1_liked)
        .bind(u2_liked)
        .execute(&state.db)
        .await?;
    }

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

    let profile = DiscoverProfile {
        id: other_user.id,
        name: other_user.name,
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
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

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
        ORDER BY last_message_at DESC NULLS LAST, created_at DESC
        "#,
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    let mut results = Vec::new();
    for m in matches {
        let other_id = if m.user1_id == user_id { m.user2_id } else { m.user1_id };
        if let Some(other_user) = fetch_user_by_id(&state.db, other_id).await? {
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

    Ok(Json(json!({ "matches": results })))
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
        .and_then(|p| p.pass_type.as_ref())
        .map(|s| PassType::from_str(s))
        .unwrap_or(PassType::Free);

    let max_distance = state.config.default_max_distance_km as f64 + pass_type.enhanced_radius_miles() * 1.60934;

    // Find nearby users with location
    let nearby = sqlx::query_as::<_, DiscoverUserRow>(
        r#"
        SELECT u.id, u.name, u.dob, u.gender, u.bio, u.profile_photo_url, u.profile_photos,
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
    .fetch_all(&state.db)
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
            Some(NearbyMatch {
                user_id: n.id,
                name: n.name,
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

    // In production, verify this matches your RevenueCat webhook secret
    let expected_secret = state.config.revenuecat_webhook_secret.as_deref().unwrap_or("");
    if !expected_secret.is_empty() && auth_header != format!("Bearer {}", expected_secret) {
        return Err(AppError::unauthorized("Invalid webhook authorization"));
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
    let otp_expires_at = Utc::now().naive_utc() + chrono::Duration::minutes(10);
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
    let verification = sqlx::query_as::<_, (i64, String, String, String)>(
        r#"
        SELECT id, verification_code, discount_tier, university_name
        FROM student_verifications
        WHERE user_id = $1 AND email = $2 AND status = 'pending'
        "#,
    )
    .bind(user_id)
    .bind(&payload.email)
    .fetch_optional(&state.db)
    .await?;

    let (verification_id, stored_otp, discount_tier, university_name) = verification
        .ok_or_else(|| AppError::bad_request("No pending verification found for this email"))?;

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
            country_code = NULLIF($3, '')
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

    let ext = if mime.contains("mp4") {
        "mp4"
    } else if mime.contains("webm") {
        "webm"
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

    // Insert spot record
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

    Ok(Json(json!({
        "message": "Spot created successfully",
        "spot_id": spot_id,
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

    let spots = fetch_user_spots(&state.db, user_id, 50).await?;

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
        .ok_or_else(|| AppError::internal("Vision service is disabled"))?
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
    let vision = state
        .vision
        .as_ref()
        .ok_or_else(|| AppError::internal("Vision service is disabled"))?
        .clone();

    let mut selfie_bytes: Option<Vec<u8>> = None;

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
            return Err(AppError::bad_request("Selfie must be an image"));
        }
        selfie_bytes = Some(read_binary_field(&mut field, state.config.max_photo_bytes).await?);
    }

    let selfie_bytes =
        selfie_bytes.ok_or_else(|| AppError::bad_request("selfie is required"))?;
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
        let bytes = match fs::read(&path).await {
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

pub async fn admin_stats(
    State(state): State<AppState>,
    _admin: AdminClaims, // Requires admin authorization
) -> Result<Json<AdminStats>, AppError> {
    let total_users = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM users")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    let verified_users = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE is_verified = TRUE",
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let active_users_24h = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE last_active > NOW() - INTERVAL '24 hours'",
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let total_matches = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM matches")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    let mutual_matches = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM matches WHERE is_mutual_match = TRUE",
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let total_messages = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM messages")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    let total_spots = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM spots")
        .fetch_one(&state.db)
        .await
        .unwrap_or(0);

    let student_verified_users = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE is_student_verified = TRUE",
    )
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let active_subscriptions = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM user_subscriptions WHERE is_active = TRUE AND (end_date IS NULL OR end_date > NOW())",
    )
    .fetch_one(&state.db)
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

async fn fetch_user_by_id(db: &PgPool, user_id: i32) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"
        SELECT id, phone_number, email, name, dob, gender, bio, location_text,
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
        SELECT id, subscription_type, pass_type, start_date, end_date, status, is_active
        FROM user_subscriptions
        WHERE user_id = $1
          AND is_active = TRUE
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
    // Top private universities (Ivy League, etc.)
    let top_private = [
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
    ];

    // Top public universities
    let top_public = [
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

    // Check for .edu domain as regular student
    if domain.ends_with(".edu") {
        let uni_name = name
            .map(|s| s.to_string())
            .unwrap_or_else(|| format!("University ({})", domain));
        return (uni_name, StudentTier::Regular);
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
    .fetch_optional(&state.db)
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

    let results = q.fetch_all(&state.db).await?;

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
    .fetch_optional(&state.db)
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
        .fetch_all(&state.db)
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
        .fetch_all(&state.db)
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
    .fetch_all(&state.db)
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

pub async fn create_reel(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<CreateReelPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let tags_json = payload.tags.as_ref().and_then(|t| serde_json::to_value(t).ok());

    let reel_id = sqlx::query_scalar::<_, i64>(
        r#"
        INSERT INTO reels (user_id, video_url, thumbnail_url, duration_sec, caption, audio_track, tags, category, location_tag, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, NOW())
        RETURNING id
        "#,
    )
    .bind(user_id)
    .bind(&payload.video_url)
    .bind(&payload.thumbnail_url)
    .bind(payload.duration_sec)
    .bind(&payload.caption)
    .bind(&payload.audio_track)
    .bind(&tags_json)
    .bind(&payload.category)
    .bind(&payload.location_tag)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(json!({
        "reel_id": reel_id,
        "message": "Reel created successfully"
    })))
}

/// Get personalized reel feed
pub async fn get_reel_feed(
    State(state): State<AppState>,
    headers: HeaderMap,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let limit: i32 = params.get("limit").and_then(|v| v.parse().ok()).unwrap_or(20);
    let session_id = params.get("session_id").cloned().unwrap_or_else(|| Uuid::new_v4().to_string());

    #[derive(sqlx::FromRow, Serialize)]
    struct ReelFeedItem {
        id: i64,
        user_id: i32,
        video_url: String,
        thumbnail_url: Option<String>,
        duration_sec: Option<i32>,
        caption: Option<String>,
        tags: Option<Value>,
        category: Option<String>,
        engagement_score: Option<f64>,
        view_count: Option<i32>,
        like_count: Option<i32>,
        created_at: Option<NaiveDateTime>,
        creator_name: Option<String>,
        creator_photo: Option<String>,
        creator_verified: Option<bool>,
    }

    let reels = sqlx::query_as::<_, ReelFeedItem>(
        r#"
        SELECT r.id, r.user_id, r.video_url, r.thumbnail_url, r.duration_sec, r.caption,
               r.tags, r.category, r.engagement_score, r.view_count, r.like_count, r.created_at,
               u.name as creator_name, u.profile_photo_1 as creator_photo, u.is_verified as creator_verified
        FROM reels r
        JOIN users u ON u.id = r.user_id
        WHERE r.is_active = TRUE AND r.user_id != $1
          AND NOT EXISTS (
              SELECT 1 FROM matches m
              WHERE ((m.user1_id = $1 AND m.user2_id = r.user_id) OR (m.user2_id = $1 AND m.user1_id = r.user_id))
              AND m.status = 'blocked'
          )
        ORDER BY r.engagement_score DESC NULLS LAST, r.created_at DESC
        LIMIT $2
        "#,
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(&state.db)
    .await?;

    Ok(Json(json!({ "reels": reels, "session_id": session_id, "count": reels.len() })))
}

/// Track reel view - ML learns interest patterns
#[derive(Deserialize)]
pub struct TrackReelViewPayload {
    pub reel_id: i32,
    pub watch_duration_sec: i32,
    pub watch_percent: f64,
    pub rewatched: Option<bool>,
    pub source: Option<String>,
    pub session_id: Option<String>,
    pub scroll_velocity: Option<f64>,
    pub position_in_feed: Option<i32>,
}

pub async fn track_reel_view(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<TrackReelViewPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let session_id = payload.session_id.clone().unwrap_or_else(|| Uuid::new_v4().to_string());

    let reel_owner = sqlx::query_scalar::<_, i32>("SELECT user_id FROM reels WHERE id = $1")
        .bind(payload.reel_id)
        .fetch_optional(&state.db)
        .await?
        .ok_or_else(|| AppError::not_found("Reel not found"))?;

    // Interest score: watch%, duration, rewatch, scroll speed
    let interest_score = calc_interest_score(payload.watch_percent, payload.watch_duration_sec, payload.rewatched.unwrap_or(false), payload.scroll_velocity);

    sqlx::query(
        r#"
        INSERT INTO reel_views (reel_id, viewer_id, watch_duration_sec, watch_percent, rewatched, source, session_id, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, $7, NOW())
        ON CONFLICT (reel_id, viewer_id, session_id) DO UPDATE SET
            watch_duration_sec = GREATEST(reel_views.watch_duration_sec, $3),
            watch_percent = GREATEST(reel_views.watch_percent, $4),
            rewatched = $5 OR reel_views.rewatched,
            rewatch_count = CASE WHEN $5 THEN reel_views.rewatch_count + 1 ELSE reel_views.rewatch_count END
        "#,
    )
    .bind(payload.reel_id).bind(user_id).bind(payload.watch_duration_sec).bind(payload.watch_percent)
    .bind(payload.rewatched.unwrap_or(false)).bind(&payload.source).bind(&session_id)
    .execute(&state.db).await?;

    // Update reel stats
    sqlx::query("UPDATE reels SET view_count = view_count + 1, avg_watch_percent = (avg_watch_percent * view_count + $2) / (view_count + 1), updated_at = NOW() WHERE id = $1")
        .bind(payload.reel_id).bind(payload.watch_percent).execute(&state.db).await?;

    // Log for ML training
    let reward = if payload.watch_percent >= 90.0 { 1.0 } else if payload.watch_percent >= 50.0 { 0.5 } else if payload.watch_percent >= 25.0 { 0.2 } else { -0.1 };
    log_reel_event(&state.db, user_id, payload.reel_id, reel_owner, "view", payload.watch_percent, payload.watch_duration_sec, payload.scroll_velocity, payload.source.as_deref(), payload.position_in_feed, reward).await?;

    // Update preferences if showed interest
    if payload.watch_percent > 50.0 {
        update_content_prefs(&state.db, user_id, payload.reel_id, interest_score).await?;
    }

    Ok(Json(json!({ "tracked": true, "interest_score": interest_score })))
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

    let reel_owner = sqlx::query_scalar::<_, i32>("SELECT user_id FROM reels WHERE id = $1")
        .bind(payload.reel_id).fetch_optional(&state.db).await?.ok_or_else(|| AppError::not_found("Reel not found"))?;

    if reel_owner == user_id { return Err(AppError::bad_request("Cannot like your own reel")); }

    let result = sqlx::query("INSERT INTO reel_likes (reel_id, user_id, created_at) VALUES ($1, $2, NOW()) ON CONFLICT DO NOTHING")
        .bind(payload.reel_id).bind(user_id).execute(&state.db).await?;

    if result.rows_affected() > 0 {
        sqlx::query("UPDATE reels SET like_count = like_count + 1, updated_at = NOW() WHERE id = $1").bind(payload.reel_id).execute(&state.db).await?;
        log_reel_event(&state.db, user_id, payload.reel_id, reel_owner, "like", 100.0, 0, None, None, None, 2.0).await?;
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

    let receiver_id = sqlx::query_scalar::<_, i32>("SELECT user_id FROM reels WHERE id = $1")
        .bind(payload.reel_id).fetch_optional(&state.db).await?.ok_or_else(|| AppError::not_found("Reel not found"))?;

    if receiver_id == sender_id { return Err(AppError::bad_request("Cannot message yourself")); }

    // Calculate effort: length, has question, thoughtfulness
    let effort_score = calc_message_effort(&payload.content, payload.reaction_emoji.is_some());
    let msg_type = payload.message_type.as_deref().unwrap_or("text");

    let message_id = sqlx::query_scalar::<_, i32>(
        "INSERT INTO reel_messages (reel_id, sender_id, receiver_id, content, message_type, reaction_emoji, created_at) VALUES ($1, $2, $3, $4, $5, $6, NOW()) RETURNING id",
    )
    .bind(payload.reel_id).bind(sender_id).bind(receiver_id).bind(&payload.content).bind(msg_type).bind(&payload.reaction_emoji)
    .fetch_one(&state.db).await?;

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
    log_reel_event(&state.db, sender_id, payload.reel_id, receiver_id, "message", 100.0, 0, None, None, None, 3.0 + effort_score).await?;
    update_content_prefs(&state.db, sender_id, payload.reel_id, effort_score).await?;

    // Record for response tracking
    let msg_features = serde_json::json!({ "length": payload.content.len(), "has_question": payload.content.contains('?'), "effort": effort_score });
    sqlx::query("INSERT INTO response_training_data (sender_id, receiver_id, interaction_source, reel_id, message_features, got_response, created_at) VALUES ($1, $2, 'reel_message', $3, $4, FALSE, NOW())")
        .bind(sender_id).bind(receiver_id).bind(payload.reel_id).bind(&msg_features).execute(&state.db).await?;

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

    #[derive(sqlx::FromRow, Serialize)]
    struct InboxMsg {
        id: i64, reel_id: i32, sender_id: i32, content: String, message_type: Option<String>,
        reaction_emoji: Option<String>, is_read: Option<bool>, created_at: Option<NaiveDateTime>,
        sender_name: Option<String>, sender_photo: Option<String>, reel_thumbnail: Option<String>,
    }

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

    let messages = sqlx::query_as::<_, InboxMsg>(query).bind(user_id).bind(limit).fetch_all(&state.db).await?;
    let unread_count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM reel_messages WHERE receiver_id = $1 AND is_read = FALSE").bind(user_id).fetch_one(&state.db).await.unwrap_or(0);

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
    struct OrigMsg { reel_id: i32, sender_id: i32, receiver_id: i32, created_at: Option<NaiveDateTime> }

    let orig = sqlx::query_as::<_, OrigMsg>("SELECT reel_id, sender_id, receiver_id, created_at FROM reel_messages WHERE id = $1")
        .bind(payload.original_message_id).fetch_optional(&state.db).await?.ok_or_else(|| AppError::not_found("Message not found"))?;

    if orig.receiver_id != user_id { return Err(AppError::forbidden("Not authorized")); }

    let response_time_sec = orig.created_at.map(|t| (Utc::now().naive_utc() - t).num_seconds() as i32);

    let reply_id = sqlx::query_scalar::<_, i32>("INSERT INTO reel_messages (reel_id, sender_id, receiver_id, content, message_type, created_at) VALUES ($1, $2, $3, $4, 'text', NOW()) RETURNING id")
        .bind(orig.reel_id).bind(user_id).bind(orig.sender_id).bind(&payload.content).fetch_one(&state.db).await?;

    // Mark original as replied
    sqlx::query("UPDATE reel_messages SET replied = TRUE, reply_delay_sec = $2 WHERE id = $1").bind(payload.original_message_id).bind(response_time_sec).execute(&state.db).await?;

    // Update conversation
    let (user_a, user_b) = if user_id < orig.sender_id { (user_id, orig.sender_id) } else { (orig.sender_id, user_id) };
    let is_replier_a = user_id == user_a;

    let conv = sqlx::query_as::<_, (i32, i32)>("SELECT a_message_count, b_message_count FROM reel_conversations WHERE reel_id = $1 AND user_a = $2 AND user_b = $3")
        .bind(orig.reel_id).bind(user_a).bind(user_b).fetch_optional(&state.db).await?;
    let conversation_continued = conv.map(|(a, b)| a + b >= 2).unwrap_or(false);

    sqlx::query(r#"UPDATE reel_conversations SET a_message_count = a_message_count + $4, b_message_count = b_message_count + $5, total_messages = total_messages + 1, last_message_by = $6, last_message_at = NOW(), updated_at = NOW() WHERE reel_id = $1 AND user_a = $2 AND user_b = $3"#)
        .bind(orig.reel_id).bind(user_a).bind(user_b).bind(if is_replier_a { 1 } else { 0 }).bind(if is_replier_a { 0 } else { 1 }).bind(user_id).execute(&state.db).await?;

    // KEY ML LEARNING: Original sender got a response - their approach worked!
    let reward = 3.0 + if conversation_continued { 2.0 } else { 0.0 };
    sqlx::query(r#"UPDATE response_training_data SET got_response = TRUE, response_time_sec = $4, conversation_continued = $5, reward = $6 WHERE sender_id = $1 AND receiver_id = $2 AND reel_id = $3 AND got_response = FALSE"#)
        .bind(orig.sender_id).bind(user_id).bind(orig.reel_id).bind(response_time_sec).bind(conversation_continued).bind(reward).execute(&state.db).await?;

    // Update sender's response patterns
    sqlx::query(r#"INSERT INTO user_response_patterns (user_id, total_responses_received, conversations_continued, updated_at) VALUES ($1, 1, $2, NOW())
        ON CONFLICT (user_id) DO UPDATE SET total_responses_received = user_response_patterns.total_responses_received + 1, conversations_continued = user_response_patterns.conversations_continued + $2, response_rate = (user_response_patterns.total_responses_received + 1)::float / GREATEST(user_response_patterns.total_messages_sent, 1), updated_at = NOW()"#)
        .bind(orig.sender_id).bind(if conversation_continued { 1 } else { 0 }).execute(&state.db).await?;

    log_reel_event(&state.db, user_id, orig.reel_id, orig.sender_id, "reply", 100.0, 0, None, None, None, 4.0).await?;

    // Check match eligibility
    check_reel_match_eligibility(&state.db, orig.reel_id, user_a, user_b).await?;

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
    Ok(Json(json!({ "marked_read": true })))
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

    #[derive(sqlx::FromRow, Serialize)]
    struct ConvMsg { id: i32, sender_id: i32, content: String, message_type: Option<String>, is_read: Option<bool>, created_at: Option<NaiveDateTime> }

    let messages = sqlx::query_as::<_, ConvMsg>(
        "SELECT id, sender_id, content, message_type, is_read, created_at FROM reel_messages WHERE reel_id = $1 AND ((sender_id = $2 AND receiver_id = $3) OR (sender_id = $3 AND receiver_id = $2)) ORDER BY created_at ASC"
    ).bind(reel_id).bind(user_id).bind(other_user).fetch_all(&state.db).await?;

    let (user_a, user_b) = if user_id < other_user { (user_id, other_user) } else { (other_user, user_id) };

    #[derive(sqlx::FromRow, Serialize)]
    struct ConvStats { total_messages: Option<i32>, eligible_for_match: Option<bool>, match_suggested: Option<bool>, match_id: Option<String> }

    let stats = sqlx::query_as::<_, ConvStats>("SELECT total_messages, eligible_for_match, match_suggested, match_id FROM reel_conversations WHERE reel_id = $1 AND user_a = $2 AND user_b = $3")
        .bind(reel_id).bind(user_a).bind(user_b).fetch_optional(&state.db).await?;

    Ok(Json(json!({ "messages": messages, "stats": stats })))
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

    let content = sqlx::query_as::<_, ContentPrefs>("SELECT preferred_categories, preferred_tags, completion_rate, like_rate, message_rate, response_rate FROM user_content_preferences WHERE user_id = $1").bind(user_id).fetch_optional(&state.db).await?;
    let response = sqlx::query_as::<_, RespPatterns>("SELECT successful_categories, successful_opener_types, response_rate, conversations_continued, matches_from_reels FROM user_response_patterns WHERE user_id = $1").bind(user_id).fetch_optional(&state.db).await?;
    let interaction = sqlx::query_as::<_, IntStats>("SELECT total_swipes, total_matches_from_swipes, swipe_success_rate, total_reel_interactions, total_matches_from_reels, reel_success_rate, best_interaction_mode FROM user_interaction_model WHERE user_id = $1").bind(user_id).fetch_optional(&state.db).await?;

    Ok(Json(json!({ "content_preferences": content, "response_patterns": response, "interaction_stats": interaction })))
}

// ============================================================================
// Helper functions for reel ML
// ============================================================================

fn calc_interest_score(watch_pct: f64, duration: i32, rewatched: bool, scroll_vel: Option<f64>) -> f64 {
    let mut score = (watch_pct / 100.0) * 0.4;
    if rewatched { score += 0.2; }
    score += ((duration as f64) / 30.0).min(1.0) * 0.2;
    if let Some(v) = scroll_vel { score += (1.0 - (v / 100.0).min(1.0)) * 0.2; }
    score.min(1.0)
}

fn calc_message_effort(content: &str, has_reaction: bool) -> f64 {
    let mut score = (content.len() as f64 / 200.0).min(1.0) * 0.3;
    if content.contains('?') { score += 0.2; }
    if !has_reaction && content.len() > 10 { score += 0.2; }
    if content.matches('.').count() + content.matches('!').count() + content.matches('?').count() >= 2 { score += 0.2; }
    if content.len() < 5 { score = 0.1; }
    score.min(1.0)
}

async fn log_reel_event(db: &PgPool, user_id: i32, reel_id: i32, owner_id: i32, event_type: &str, watch_pct: f64, duration: i32, scroll_vel: Option<f64>, source: Option<&str>, position: Option<i32>, reward: f64) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO reel_engagement_events (user_id, reel_id, reel_owner_id, event_type, watch_percent, time_on_reel_sec, scroll_velocity, source, position_in_feed, reward, created_at) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,NOW())")
        .bind(user_id).bind(reel_id).bind(owner_id).bind(event_type).bind(watch_pct).bind(duration).bind(scroll_vel).bind(source).bind(position).bind(reward).execute(db).await?;
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

        Ok(Json(json!({
            "eligible": true,
            "round": r,
            "client_id": client_id
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

    Ok(Json(json!({
        "round_id": round_id,
        "round_number": next_round,
        "status": "in_progress"
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

    // FedAvg: weighted average by num_samples
    let total_samples: i32 = updates.iter().map(|u| u.num_samples).sum();
    let avg_loss: f64 = updates.iter().map(|u| u.local_loss * u.num_samples as f64).sum::<f64>() / total_samples as f64;
    let avg_accuracy: f64 = updates.iter().filter_map(|u| u.local_accuracy.map(|a| a * u.num_samples as f64)).sum::<f64>() / total_samples as f64;

    // In production, you'd do actual weight aggregation here
    // For now, we store the aggregated stats and mark complete
    let new_version = round.2.unwrap_or(1) + 1;

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
        }
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
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Verify user is a verified student
    let is_verified = sqlx::query_scalar::<_, bool>(
        "SELECT COALESCE(is_student_verified, FALSE) FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_one(&state.db)
    .await?;

    if !is_verified {
        return Err(AppError::forbidden("Student verification required to search universities"));
    }

    let search_term = format!("%{}%", params.q.to_lowercase());
    let limit = params.limit.unwrap_or(20).min(50);

    let universities = if let Some(country) = &params.country {
        sqlx::query_as::<_, UniversityRow>(
            r#"
            SELECT id, name, short_name, domain, country, country_code, state_province, city, tier
            FROM universities
            WHERE is_active = TRUE
              AND country_code = $1
              AND (LOWER(name) LIKE $2 OR LOWER(short_name) LIKE $2 OR LOWER(domain) LIKE $2)
            ORDER BY tier DESC, name ASC
            LIMIT $3
            "#
        )
        .bind(country)
        .bind(&search_term)
        .bind(limit)
        .fetch_all(&state.db)
        .await?
    } else {
        sqlx::query_as::<_, UniversityRow>(
            r#"
            SELECT id, name, short_name, domain, country, country_code, state_province, city, tier
            FROM universities
            WHERE is_active = TRUE
              AND (LOWER(name) LIKE $1 OR LOWER(short_name) LIKE $1 OR LOWER(domain) LIKE $1)
            ORDER BY tier DESC, name ASC
            LIMIT $2
            "#
        )
        .bind(&search_term)
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
            "state": u.state_province,
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
        SELECT university_id, country_code
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

    // Check access
    let (has_access, access_type) = check_university_access(&state.db, user_id, params.university_id).await?;
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
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("University not found"))?;

    // Get verified students from this university
    let profiles = sqlx::query_as::<_, DiscoverUserRow>(
        r#"
        SELECT u.id, u.name, u.dob, u.gender, u.bio, u.profile_photo_url, u.profile_photos,
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
    .fetch_all(&state.db)
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

        DiscoverProfile {
            id: row.id,
            name: row.name.clone(),
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
        SELECT sv.university_id, u.name as university_name, sv.country_code
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
