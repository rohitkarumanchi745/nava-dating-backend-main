use chrono::{NaiveDate, NaiveDateTime};
use serde_json::Value;
use sqlx::FromRow;

#[derive(Debug, FromRow)]
pub struct UserAuthRow {
    pub id: i64,
    pub is_profile_complete: Option<bool>,
}

#[derive(Debug, FromRow)]
pub struct UserRow {
    pub id: i64,
    pub phone_number: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub dob: Option<NaiveDate>,
    pub gender: Option<String>,
    pub bio: Option<String>,
    pub location_text: Option<String>,
    pub interests: Option<Value>,
    pub languages: Option<Value>,
    pub looking_for: Option<String>,
    pub profession_category: Option<String>,
    pub profession_title: Option<String>,
    pub height_cm: Option<i32>,
    pub profile_photo_url: Option<String>,
    pub profile_photos: Option<Value>,
    pub profile_photo_1: Option<String>,
    pub profile_photo_2: Option<String>,
    pub profile_photo_3: Option<String>,
    pub is_profile_complete: Option<bool>,
    pub attractiveness_score: Option<f64>,
    pub is_verified: Option<bool>,
    pub is_student_verified: Option<bool>,
}

#[derive(Debug, FromRow)]
pub struct ProfileStatusRow {
    pub name: Option<String>,
    pub dob: Option<NaiveDate>,
    pub gender: Option<String>,
    pub bio: Option<String>,
    pub profile_photo_url: Option<String>,
    pub profile_photos: Option<Value>,
    pub is_profile_complete: Option<bool>,
}

#[derive(Debug, FromRow)]
pub struct UserPreferencesRow {
    pub min_age: Option<i32>,
    pub max_age: Option<i32>,
    pub preferred_genders: Option<Value>,
    pub max_distance: Option<i32>,
    pub only_verified: Option<bool>,
    pub only_students: Option<bool>,
    pub preferred_locations: Option<Value>,
}

#[derive(Debug, FromRow)]
pub struct UserLocationRow {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub country: Option<String>,
    pub neighborhood: Option<String>,
    pub is_fuzzy: Option<bool>,
    pub show_exact_distance: Option<bool>,
    pub last_updated: Option<NaiveDateTime>,
}

#[derive(Debug, FromRow)]
pub struct UserSubscriptionRow {
    pub id: i64,
    pub subscription_type: Option<String>,
    pub pass_type: Option<String>,
    pub start_date: Option<NaiveDateTime>,
    pub end_date: Option<NaiveDateTime>,
    pub status: Option<String>,
    pub is_active: Option<bool>,
}

#[derive(Debug, FromRow)]
pub struct SpotRow {
    pub id: i64,
    pub title: Option<String>,
    pub poster_url: Option<String>,
    pub renditions: Option<Value>,
    pub expires_at: Option<NaiveDateTime>,
    pub created_at: Option<NaiveDateTime>,
    pub is_global: Option<bool>,
    pub city: Option<String>,
    pub tags: Option<Value>,
}
