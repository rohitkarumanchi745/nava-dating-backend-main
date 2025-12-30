use std::collections::HashMap;
use std::path::Path;

use axum::{
    extract::{Multipart, Query, State},
    http::{HeaderMap, StatusCode},
    Json,
};
use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use chrono::{Datelike, NaiveDate, NaiveDateTime, Utc};
use image::codecs::jpeg::JpegEncoder;
use image::{ColorType, DynamicImage};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::PgPool;
use tokio::fs;
use tokio::task;
use uuid::Uuid;

use crate::{
    auth::{create_access_token, decode_access_token, extract_bearer_token},
    error::AppError,
    models::{
        ProfileStatusRow, SpotRow, UserAuthRow, UserLocationRow, UserPreferencesRow, UserRow,
        UserSubscriptionRow,
    },
    state::AppState,
    vision::VisionAnalysis,
};

#[derive(Serialize)]
pub struct HealthResponse {
    status: &'static str,
    db: &'static str,
}

const UPLOAD_DIR: &str = "uploads";
const ALLOWED_GENDERS: [&str; 4] = ["male", "female", "non_binary", "other"];

#[derive(Deserialize)]
pub struct SendOtpPayload {
    phone_number: String,
}

#[derive(Deserialize)]
pub struct VerifyOtpPayload {
    phone_number: String,
    otp: String,
}

#[derive(Serialize)]
pub struct SendOtpResponse {
    message: &'static str,
    otp: &'static str,
}

#[derive(Serialize)]
pub struct VerifyOtpResponse {
    access_token: String,
    token_type: &'static str,
    user_id: i64,
    is_profile_complete: bool,
}

pub async fn health(State(state): State<AppState>) -> (StatusCode, Json<HealthResponse>) {
    let db_ok = sqlx::query("SELECT 1").execute(&state.db).await.is_ok();
    let response = HealthResponse {
        status: if db_ok { "ok" } else { "degraded" },
        db: if db_ok { "ok" } else { "down" },
    };
    let status = if db_ok {
        StatusCode::OK
    } else {
        StatusCode::SERVICE_UNAVAILABLE
    };
    (status, Json(response))
}

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
    let analysis = analyze_photo_bytes(vision, bytes).await?.analysis;
    Ok(Json(serde_json::to_value(analysis).unwrap_or(json!({}))))
}

pub async fn update_profile(
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
                let photo = analyze_photo_bytes(vision.clone(), bytes).await?;
                if photo.analysis.inappropriate_content {
                    return Err(AppError::bad_request(format!(
                        "Photo {} contains inappropriate content",
                        idx + 1
                    )));
                }
                photos[idx] = Some(photo);
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

    fs::create_dir_all(UPLOAD_DIR)
        .await
        .map_err(|_| AppError::internal("Failed to create upload directory"))?;

    let mut saved_paths = Vec::new();
    let mut insights = Vec::new();

    for (idx, photo) in photo_inputs.into_iter() {
        let filename = format!(
            "{}_photo_{}_{}_{}.jpg",
            user_id,
            idx,
            Utc::now().timestamp(),
            Uuid::new_v4()
        );
        let path = Path::new(UPLOAD_DIR).join(filename);
        let jpeg_bytes = encode_jpeg(&photo.image)
            .map_err(|_| AppError::internal("Failed to encode image"))?;
        if let Err(err) = fs::write(&path, jpeg_bytes).await {
            cleanup_files(&saved_paths).await;
            return Err(AppError::internal(format!(
                "Failed to save photo: {err}"
            )));
        }
        saved_paths.push(path.to_string_lossy().to_string());
        insights.push(json!({
            "quality": photo.analysis.quality_score,
            "smile_detected": photo.analysis.smile_intensity > 0.5,
            "authenticity": photo.analysis.authenticity_score,
        }));
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
            is_profile_complete = TRUE,
            updated_at = NOW()
        WHERE id = $9
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
    .bind(user_id)
    .execute(&state.db)
    .await?;

    if result.rows_affected() == 0 {
        cleanup_files(&saved_paths).await;
        return Err(AppError::not_found("User not found"));
    }

    Ok(Json(json!({
        "message": "Profile updated successfully",
        "photos": saved_paths,
        "photo_insights": insights,
    })))
}

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
    let selfie_analysis = analyze_photo_bytes(vision.clone(), selfie_bytes)
        .await?
        .analysis;

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
        let analysis = match analyze_photo_bytes(vision.clone(), bytes).await {
            Ok(photo) => photo.analysis,
            Err(_) => continue,
        };
        if let Some(similarity) =
            cosine_similarity(&selfie_analysis.style_embedding, &analysis.style_embedding)
        {
            if best_similarity.map(|best| similarity > best).unwrap_or(true) {
                best_similarity = Some(similarity);
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

    Ok(Json(SendOtpResponse {
        message: "OTP sent successfully",
        otp: "1234",
    }))
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

    let row = sqlx::query_as::<_, UserAuthRow>(
        "SELECT id, is_profile_complete FROM users WHERE phone_number = $1",
    )
    .bind(&payload.phone_number)
    .fetch_optional(&state.db)
    .await?;

    let user = row.ok_or_else(|| AppError::not_found("User not found"))?;
    let token = create_access_token(
        user.id,
        &state.config.secret_key,
        state.config.access_token_expire_minutes,
    )?;

    Ok(Json(VerifyOtpResponse {
        access_token: token,
        token_type: "bearer",
        user_id: user.id,
        is_profile_complete: user.is_profile_complete.unwrap_or(false),
    }))
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

async fn fetch_user_by_id(db: &PgPool, user_id: i64) -> Result<Option<UserRow>, sqlx::Error> {
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
    user_id: i64,
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
    user_id: i64,
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
    user_id: i64,
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
    user_id: i64,
    limit: i64,
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
        photos.push(value.clone());
    }
    if let Some(value) = &user.profile_photo_2 {
        photos.push(value.clone());
    }
    if let Some(value) = &user.profile_photo_3 {
        photos.push(value.clone());
    }
    photos
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

struct PhotoInput {
    image: DynamicImage,
    analysis: VisionAnalysis,
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
        Ok(PhotoInput { image, analysis })
    })
    .await
    .map_err(|_| AppError::internal("Vision task failed"))?
}

fn extract_photo_paths(user: &UserRow) -> Vec<String> {
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
        photos.push(value.clone());
    }
    if let Some(value) = &user.profile_photo_2 {
        photos.push(value.clone());
    }
    if let Some(value) = &user.profile_photo_3 {
        photos.push(value.clone());
    }
    photos
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
