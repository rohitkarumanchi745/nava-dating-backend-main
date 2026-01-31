//! Common utilities and helper functions shared across handlers
//!
//! This module provides:
//! - File upload helpers (multipart processing)
//! - Image processing utilities
//! - Database fetch helpers
//! - Age calculation and validation

use axum::extract::multipart::Field;
use chrono::{Datelike, NaiveDate, Utc};
use image::codecs::jpeg::JpegEncoder;
use image::{ColorType, DynamicImage};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Mutex;
use tokio::task;

use crate::error::AppError;
use crate::models::UserRow;
use crate::vision::{VisionAnalysis, VisionAnalyzer};

/// Photo input with optional vision analysis
pub struct PhotoInput {
    pub image: DynamicImage,
    pub analysis: Option<VisionAnalysis>,
}

/// Allowed gender values for validation
pub const ALLOWED_GENDERS: [&str; 4] = ["male", "female", "non_binary", "other"];

/// Read a text field from multipart form data with size limit
pub async fn read_text_field(field: &mut Field<'_>, max_bytes: usize) -> Result<String, AppError> {
    let bytes = read_binary_field(field, max_bytes).await?;
    let value = String::from_utf8(bytes).map_err(|_| AppError::bad_request("Invalid text field"))?;
    Ok(value.trim().to_string())
}

/// Read binary data from multipart form data with size limit
pub async fn read_binary_field(field: &mut Field<'_>, max_bytes: usize) -> Result<Vec<u8>, AppError> {
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

/// Calculate age from date of birth
pub fn calculate_age(dob: NaiveDate) -> i32 {
    let today = Utc::now().date_naive();
    let mut age = today.year() - dob.year();
    if (today.month(), today.day()) < (dob.month(), dob.day()) {
        age -= 1;
    }
    age
}

/// Encode image as JPEG with quality 90
pub fn encode_jpeg(image: &DynamicImage) -> Result<Vec<u8>, AppError> {
    let rgb = image.to_rgb8();
    let mut buffer = Vec::new();
    let mut encoder = JpegEncoder::new_with_quality(&mut buffer, 90);
    encoder
        .encode(&rgb, rgb.width(), rgb.height(), ColorType::Rgb8.into())
        .map_err(|_| AppError::internal("Failed to encode image"))?;
    Ok(buffer)
}

/// Clean up uploaded files on error
pub async fn cleanup_files(paths: &[String]) {
    for path in paths {
        let _ = fs::remove_file(path).await;
    }
}

/// Analyze photo using vision AI model
pub async fn analyze_photo_bytes(
    vision: Arc<Mutex<VisionAnalyzer>>,
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

/// Fetch user by ID with all fields
pub async fn fetch_user_by_id(db: &PgPool, user_id: i32) -> Result<Option<UserRow>, sqlx::Error> {
    sqlx::query_as::<_, UserRow>(
        r#"
        SELECT id, phone_number, email, name, dob, gender, bio, location_text,
               interests, languages, looking_for, profession_category, profession_title,
               height_cm, profile_photo_url, profile_photos, profile_photo_1,
               profile_photo_2, profile_photo_3, is_profile_complete, attractiveness_score,
               verification_status, is_premium, is_active, last_active, created_at, updated_at
        FROM users
        WHERE id = $1
        "#,
    )
    .bind(user_id)
    .fetch_optional(db)
    .await
}

/// Validate that a date of birth makes the user at least 18 years old
pub fn validate_age_18_plus(dob: NaiveDate) -> Result<(), AppError> {
    if calculate_age(dob) < 18 {
        return Err(AppError::bad_request("Must be at least 18 years old"));
    }
    Ok(())
}

/// Validate gender against allowed values
pub fn validate_gender(gender: &str) -> Result<(), AppError> {
    if !ALLOWED_GENDERS.contains(&gender) {
        return Err(AppError::bad_request(format!(
            "Invalid gender. Allowed values: {:?}",
            ALLOWED_GENDERS
        )));
    }
    Ok(())
}

/// Parse date string in YYYY-MM-DD format
pub fn parse_date(date_str: &str) -> Result<NaiveDate, AppError> {
    NaiveDate::parse_from_str(date_str.trim(), "%Y-%m-%d")
        .map_err(|_| AppError::bad_request("Date must be in YYYY-MM-DD format"))
}
