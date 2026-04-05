use chrono::{NaiveDate, NaiveDateTime};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sqlx::FromRow;

use crate::config::Config;

// ============================================================================
// User Models
// ============================================================================

#[derive(Debug, FromRow)]
pub struct UserAuthRow {
    pub id: i64,
    pub is_profile_complete: Option<bool>,
}

#[derive(Debug, FromRow)]
pub struct UserRow {
    pub id: i32,
    pub phone_number: String,
    pub email: Option<String>,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub show_verified_name: Option<bool>,
    pub show_display_name_in_search: Option<bool>,
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
pub struct DiscoverUserRow {
    pub id: i32,
    pub name: Option<String>,
    pub display_name: Option<String>,
    pub show_verified_name: Option<bool>,
    pub dob: Option<NaiveDate>,
    pub gender: Option<String>,
    pub bio: Option<String>,
    pub profile_photo_url: Option<String>,
    pub profile_photos: Option<Value>,
    pub profile_photo_1: Option<String>,
    pub profile_photo_2: Option<String>,
    pub profile_photo_3: Option<String>,
    pub is_verified: Option<bool>,
    pub attractiveness_score: Option<f64>,
    pub looking_for: Option<String>,
    pub profession_title: Option<String>,
    pub height_cm: Option<i32>,
    pub city: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
}

// ============================================================================
// User Preferences
// ============================================================================

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
pub struct UserPreferencesFullRow {
    pub id: i32,
    pub user_id: i32,
    pub min_age: Option<i32>,
    pub max_age: Option<i32>,
    pub preferred_genders: Option<Value>,
    pub intent: Option<String>,
    pub languages: Option<Value>,
    pub max_distance: Option<i32>,
    pub distance_miles: Option<i32>,
    pub preferred_locations: Option<Value>,
    pub only_verified: Option<bool>,
    pub only_students: Option<bool>,
}

// ============================================================================
// Location
// ============================================================================

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
pub struct UserLocationFullRow {
    pub id: i32,
    pub user_id: i32,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub accuracy: Option<f64>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub country: Option<String>,
    pub neighborhood: Option<String>,
    pub is_fuzzy: Option<bool>,
    pub show_exact_distance: Option<bool>,
    pub last_updated: Option<NaiveDateTime>,
    pub update_source: Option<String>,
}

// ============================================================================
// Subscriptions & Passes
// ============================================================================

#[derive(Debug, FromRow)]
pub struct UserSubscriptionRow {
    pub id: i32,
    pub subscription_type: Option<String>,
    pub start_date: Option<NaiveDateTime>,
    pub end_date: Option<NaiveDateTime>,
    pub status: Option<String>,
}

#[derive(Debug, FromRow)]
pub struct UserSubscriptionFullRow {
    pub id: i32,
    pub user_id: i32,
    pub subscription_type: Option<String>,
    pub pass_type: Option<String>,
    pub status: Option<String>,
    pub original_price: Option<Decimal>,
    pub amount_paid: Option<Decimal>,
    pub discount_applied: Option<Decimal>,
    pub discount_type: Option<String>,
    pub start_date: Option<NaiveDateTime>,
    pub end_date: Option<NaiveDateTime>,
    pub payment_id: Option<String>,
    pub payment_method: Option<String>,
    pub stripe_subscription_id: Option<String>,
    pub is_active: Option<bool>,
    pub enhanced_radius: Option<f64>,
    pub can_see_city_names: Option<bool>,
    pub can_see_neighborhoods: Option<bool>,
    pub unlimited_swipes: Option<bool>,
    pub priority_visibility: Option<bool>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum PassType {
    Free,
    Hourly,
    Daily,
    Weekly,
    Monthly,
    Ultra,
}

impl PassType {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "hourly" => Self::Hourly,
            "daily" => Self::Daily,
            "weekly" => Self::Weekly,
            "monthly" => Self::Monthly,
            "ultra" => Self::Ultra,
            _ => Self::Free,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Hourly => "hourly",
            Self::Daily => "daily",
            Self::Weekly => "weekly",
            Self::Monthly => "monthly",
            Self::Ultra => "ultra",
        }
    }

    pub fn price_cents(&self, config: &Config) -> i64 {
        match self {
            Self::Free => 0,
            Self::Hourly => config.pass_price_hourly,
            Self::Daily => config.pass_price_daily,
            Self::Weekly => config.pass_price_weekly,
            Self::Monthly => config.pass_price_monthly,
            Self::Ultra => config.pass_price_ultra,
        }
    }

    pub fn duration_hours(&self) -> Option<i64> {
        match self {
            Self::Free => None,
            Self::Hourly => Some(1),
            Self::Daily => Some(24),
            Self::Weekly => Some(24 * 7),
            Self::Monthly => Some(24 * 30),
            Self::Ultra => None, // unlimited
        }
    }

    pub fn enhanced_radius_miles(&self) -> f64 {
        match self {
            Self::Free => 0.0,
            Self::Hourly => 2.0,
            Self::Daily => 5.0,
            Self::Weekly => 10.0,
            Self::Monthly => 25.0,
            Self::Ultra => f64::MAX,
        }
    }

    pub fn can_see_city_names(&self) -> bool {
        matches!(self, Self::Ultra)
    }

    pub fn can_see_exact_distance(&self) -> bool {
        !matches!(self, Self::Free)
    }
}

// ============================================================================
// Matches
// ============================================================================

#[derive(Debug, FromRow)]
pub struct MatchRow {
    pub id: String,
    pub user1_id: i32,
    pub user2_id: i32,
    pub user1_liked: Option<bool>,
    pub user2_liked: Option<bool>,
    pub is_mutual_match: Option<bool>,
    pub ai_compatibility_score: Option<f64>,
    pub visual_compatibility_score: Option<f64>,
    pub match_reason: Option<String>,
    pub messages_count: Option<i32>,
    pub voice_messages_count: Option<i32>,
    pub last_message_at: Option<NaiveDateTime>,
    pub can_send_text: Option<bool>,
    pub status: Option<String>,
    pub blocked_by_user_id: Option<i32>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Debug, FromRow)]
pub struct MatchCheckRow {
    pub id: String,
    pub user1_id: i32,
    pub user2_id: i32,
    pub user1_liked: Option<bool>,
    pub user2_liked: Option<bool>,
    pub is_mutual_match: Option<bool>,
}

// ============================================================================
// Messages
// ============================================================================

#[derive(Debug, FromRow, Serialize)]
pub struct MessageRow {
    pub id: i32,
    pub match_id: String,
    pub sender_id: i32,
    pub receiver_id: i32,
    pub content: String,
    pub message_type: Option<String>,
    pub is_read: Option<bool>,
    pub read_at: Option<NaiveDateTime>,
    pub created_at: Option<NaiveDateTime>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct VoiceMessageRow {
    pub id: i32,
    pub match_id: String,
    pub sender_id: i32,
    pub receiver_id: i32,
    pub voice_url: String,
    pub duration: Option<i32>,
    pub file_size: Option<i32>,
    pub transcription: Option<String>,
    pub is_played: Option<bool>,
    pub played_at: Option<NaiveDateTime>,
    pub created_at: Option<NaiveDateTime>,
}

// ============================================================================
// Spots (Short Videos/Audio)
// ============================================================================

#[derive(Debug, FromRow)]
pub struct SpotRow {
    pub id: i32,
    pub title: Option<String>,
    pub poster_url: Option<String>,
    pub renditions: Option<Value>,
    pub expires_at: Option<NaiveDateTime>,
    pub created_at: Option<NaiveDateTime>,
    pub is_global: Option<bool>,
    pub city: Option<String>,
    pub tags: Option<Value>,
}

#[derive(Debug, FromRow)]
pub struct SpotFullRow {
    pub id: i32,
    pub user_id: i32,
    pub title: Option<String>,
    pub original_url: Option<String>,
    pub poster_url: Option<String>,
    pub mime_type: Option<String>,
    pub duration_sec: Option<i32>,
    pub renditions: Option<Value>,
    pub tags: Option<Value>,
    pub city: Option<String>,
    pub is_global: Option<bool>,
    pub expires_at: Option<NaiveDateTime>,
    pub created_at: Option<NaiveDateTime>,
    pub updated_at: Option<NaiveDateTime>,
}

#[derive(Debug, FromRow, Serialize)]
pub struct SpotMessageRow {
    pub id: i32,
    pub spot_id: i32,
    pub sender_id: i32,
    pub text: String,
    pub created_at: Option<NaiveDateTime>,
}

// ============================================================================
// Student Verification
// ============================================================================

#[derive(Debug, FromRow)]
pub struct StudentVerificationRow {
    pub id: i32,
    pub user_id: i32,
    pub university_name: Option<String>,
    pub university_type: Option<String>,
    pub email: Option<String>,
    pub student_id: Option<String>,
    pub status: Option<String>,
    pub verification_method: Option<String>,
    pub discount_tier: Option<String>,
    pub submitted_at: Option<NaiveDateTime>,
    pub verified_at: Option<NaiveDateTime>,
    pub expires_at: Option<NaiveDateTime>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StudentTier {
    None,
    Regular,
    TopPublic,
    TopPrivate,
    Graduate,
    Alumni,
}

impl StudentTier {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "regular" => Self::Regular,
            "top_public" => Self::TopPublic,
            "top_private" => Self::TopPrivate,
            "graduate" => Self::Graduate,
            "alumni" => Self::Alumni,
            _ => Self::None,
        }
    }

    pub fn discount_percent(&self, config: &Config) -> i32 {
        let decimal = match self {
            Self::None => 0.0,
            Self::Regular => config.student_discount_other,
            Self::TopPublic => config.student_discount_top50,
            Self::TopPrivate => config.student_discount_ivy,
            Self::Graduate => config.student_discount_graduate,
            Self::Alumni => config.student_discount_alumni,
        };
        (decimal * 100.0) as i32
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::None => "none",
            Self::Regular => "regular",
            Self::TopPublic => "top_public",
            Self::TopPrivate => "top_private",
            Self::Graduate => "graduate",
            Self::Alumni => "alumni",
        }
    }
}

// ============================================================================
// Interaction Events (for ML)
// ============================================================================

#[derive(Debug, FromRow)]
pub struct InteractionEventRow {
    pub id: i32,
    pub user_id: i32,
    pub target_user_id: i32,
    pub event_type: String,
    pub event_metadata: Option<Value>,
    pub slate_id: Option<String>,
    pub rank: Option<i32>,
    pub surface: Option<String>,
    pub reward: Option<f64>,
    pub delay_ms: Option<i32>,
    pub created_at: Option<NaiveDateTime>,
}

// ============================================================================
// Call Sessions
// ============================================================================

#[derive(Debug, Serialize, Clone)]
pub struct CallSession {
    pub call_id: String,
    pub match_id: String,
    pub caller_id: i32,
    pub callee_id: i32,
    pub call_type: String, // "voice" or "video"
    pub status: String,    // "ringing", "active", "ended"
    pub started_at: NaiveDateTime,
    pub ended_at: Option<NaiveDateTime>,
}

// ============================================================================
// Admin Stats
// ============================================================================

#[derive(Debug, Serialize)]
pub struct AdminStats {
    pub total_users: i64,
    pub verified_users: i64,
    pub active_users_24h: i64,
    pub total_matches: i64,
    pub mutual_matches: i64,
    pub total_messages: i64,
    pub total_spots: i64,
    pub student_verified_users: i64,
    pub active_subscriptions: i64,
}

// ============================================================================
// Request/Response DTOs
// ============================================================================

#[derive(Debug, Deserialize)]
pub struct SendOtpRequest {
    pub phone_number: String,
}

#[derive(Debug, Deserialize)]
pub struct VerifyOtpRequest {
    pub phone_number: String,
    pub otp: String,
}

#[derive(Debug, Serialize)]
pub struct VerifyOtpResponse {
    pub access_token: String,
    pub token_type: &'static str,
    pub user_id: i64,
    pub is_new_user: bool,
    pub is_profile_complete: bool,
}

#[derive(Debug, Deserialize)]
pub struct UpdateBioRequest {
    pub bio: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdatePreferencesRequest {
    pub min_age: Option<i32>,
    pub max_age: Option<i32>,
    pub preferred_genders: Option<Vec<String>>,
    pub max_distance_km: Option<i32>,
    pub only_verified: Option<bool>,
    pub only_students: Option<bool>,
    pub intent: Option<String>,
    pub preferred_locations: Option<Vec<String>>,
}

#[derive(Debug, Deserialize)]
pub struct LocationUpdateRequest {
    pub latitude: f64,
    pub longitude: f64,
    pub accuracy: Option<f64>,
    pub city: Option<String>,
    pub state: Option<String>,
    pub country: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct PurchasePassRequest {
    pub pass_type: String,
    pub promo_code: Option<String>,
    /// Client-generated idempotency key to prevent duplicate purchases
    pub idempotency_key: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct LikeRequest {
    pub target_user_id: i32,
    /// Optional message sent with the like (message request feature)
    pub message: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StudentVerifyRequest {
    pub email: String,
    pub university_name: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StudentVerifyOtpRequest {
    pub email: String,
    pub otp: String,
}

#[derive(Debug, Deserialize)]
pub struct CreateCallRequest {
    pub match_id: String,
    pub call_type: Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CreateCallResponse {
    pub call_id: String,
    pub call_token: String,
    pub expires_in: i64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DiscoverProfile {
    pub id: i32,
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_name: Option<String>,
    pub age: Option<i32>,
    pub gender: Option<String>,
    pub bio: Option<String>,
    pub photos: Vec<String>,
    pub is_verified: bool,
    pub looking_for: Option<String>,
    pub profession_title: Option<String>,
    pub height_cm: Option<i32>,
    pub distance_km: Option<f64>,
    pub distance_text: Option<String>,
    pub city: Option<String>,
    pub compatibility_score: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub university: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub university_tier: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub interaction_status: Option<String>,
    /// True if this person has super-liked the viewing user
    #[serde(skip_serializing_if = "Option::is_none")]
    pub super_liked_you: Option<bool>,
}

#[derive(Debug, Serialize)]
pub struct MatchDetail {
    pub match_id: String,
    pub is_mutual: bool,
    pub matched_at: Option<String>,
    pub can_send_text: bool,
    pub messages_count: i32,
    pub voice_messages_count: i32,
    pub other_user: DiscoverProfile,
}

#[derive(Debug, Serialize)]
pub struct NearbyMatch {
    pub user_id: i32,
    pub name: Option<String>,
    pub photos: Vec<String>,
    pub distance_km: f64,
    pub distance_text: String,
    pub city: Option<String>,
    pub is_verified: bool,
}

#[derive(Debug, Serialize)]
pub struct StudentStatusResponse {
    pub is_verified: bool,
    pub university_name: Option<String>,
    pub discount_tier: Option<String>,
    pub discount_percent: i32,
    pub expires_at: Option<String>,
}
