//! Ads endpoints for ad monetization
//!
//! Endpoints:
//! - GET /api/ads/request - Request an ad for display
//! - POST /api/ads/impression - Record ad impression
//! - POST /api/ads/rewarded-complete - Record rewarded ad completion

use axum::{
    extract::{Query, State},
    Json,
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::Claims,
    error::AppError,
    services::ads::{AdImpression, AdNetwork, AdRequest, AdType, RewardType, RewardedAdCompletion},
    state::AppState,
};

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Deserialize)]
pub struct RequestAdQuery {
    pub placement_id: String,
    pub platform: Option<String>,
    pub country_code: Option<String>,
    /// Indian state code (e.g., "TS", "AP", "TN") for regional targeting
    pub state_code: Option<String>,
    /// City for metro targeting (e.g., "Hyderabad", "Bangalore")
    pub city: Option<String>,
    /// Preferred language (e.g., "te", "hi", "ta")
    pub language: Option<String>,
}

#[derive(Deserialize)]
pub struct RecordImpressionPayload {
    pub placement_id: String,
    pub ad_network: String,
    pub ad_type: String,
    pub revenue_micros: Option<i64>,
    pub ecpm_micros: Option<i64>,
    pub clicked: Option<bool>,
    pub completed: Option<bool>,
    pub platform: Option<String>,
    pub country_code: Option<String>,
    /// State code for regional analytics
    pub state_code: Option<String>,
    /// City for metro analytics
    pub city: Option<String>,
    /// Language the ad was served in
    pub language_code: Option<String>,
}

#[derive(Deserialize)]
pub struct RewardedCompletePayload {
    pub placement_id: String,
    pub reward_type: String,
    pub reward_amount: Option<i32>,
}

#[derive(Serialize)]
pub struct RewardedCompleteResponse {
    pub success: bool,
    pub reward_granted: String,
    pub reward_amount: i32,
}

// ============================================================================
// Handlers
// ============================================================================

/// Request an ad for display
/// GET /api/ads/request
pub async fn request_ad(
    State(state): State<AppState>,
    claims: Claims,
    Query(query): Query<RequestAdQuery>,
) -> Result<Json<crate::services::ads::AdResponse>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let ads_service = state.ads_service.as_ref()
        .ok_or_else(|| AppError::internal("Ads service not configured"))?;

    let request = AdRequest {
        user_id,
        placement_id: query.placement_id,
        platform: query.platform.unwrap_or_else(|| "android".to_string()),
        country_code: query.country_code.unwrap_or_else(|| "IN".to_string()),
        state_code: query.state_code,
        city: query.city,
        language: query.language,
    };

    let response = ads_service.request_ad(request).await?;
    Ok(Json(response))
}

/// Record ad impression
/// POST /api/ads/impression
pub async fn record_impression(
    State(state): State<AppState>,
    claims: Claims,
    Json(payload): Json<RecordImpressionPayload>,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let ads_service = state.ads_service.as_ref()
        .ok_or_else(|| AppError::internal("Ads service not configured"))?;

    let ad_network = match payload.ad_network.to_lowercase().as_str() {
        "admob" => AdNetwork::AdMob,
        "facebook" => AdNetwork::Facebook,
        "unity" => AdNetwork::Unity,
        _ => AdNetwork::AdMob,
    };

    let ad_type = match payload.ad_type.to_lowercase().as_str() {
        "banner" => AdType::Banner,
        "interstitial" => AdType::Interstitial,
        "native" => AdType::Native,
        "rewarded" => AdType::Rewarded,
        _ => AdType::Interstitial,
    };

    let impression = AdImpression {
        user_id,
        placement_id: payload.placement_id,
        ad_network,
        ad_type,
        revenue_usd_micro: payload.revenue_micros.unwrap_or(0),
        ecpm_usd_micro: payload.ecpm_micros.unwrap_or(0),
        clicked: payload.clicked.unwrap_or(false),
        completed: payload.completed.unwrap_or(false),
        platform: payload.platform.unwrap_or_else(|| "android".to_string()),
        country_code: payload.country_code.unwrap_or_else(|| "IN".to_string()),
        state_code: payload.state_code,
        city: payload.city,
        language_code: payload.language_code,
    };

    let impression_id = ads_service.track_impression(impression).await?;

    Ok(Json(json!({ "recorded": true, "impression_id": impression_id })))
}

/// Record rewarded ad completion and grant reward
/// POST /api/ads/rewarded-complete
pub async fn rewarded_complete(
    State(state): State<AppState>,
    claims: Claims,
    Json(payload): Json<RewardedCompletePayload>,
) -> Result<Json<RewardedCompleteResponse>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let ads_service = state.ads_service.as_ref()
        .ok_or_else(|| AppError::internal("Ads service not configured"))?;

    let reward_type = match payload.reward_type.to_lowercase().as_str() {
        "boost" => RewardType::Boost,
        "super_like" | "superlike" => RewardType::SuperLike,
        "premium_hours" => RewardType::PremiumHours,
        "extra_likes" => RewardType::ExtraLikes,
        "profile_view" => RewardType::ProfileView,
        _ => RewardType::ExtraLikes,
    };

    let reward_amount = payload.reward_amount.unwrap_or_else(|| reward_type.default_amount());

    let completion = RewardedAdCompletion {
        user_id,
        placement_id: payload.placement_id,
        reward_type,
        reward_amount,
    };

    ads_service.process_rewarded_completion(completion).await?;

    Ok(Json(RewardedCompleteResponse {
        success: true,
        reward_granted: reward_type.as_str().to_string(),
        reward_amount,
    }))
}

/// Get user's consumable balances
/// GET /api/ads/rewards-balance
pub async fn get_rewards_balance(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let consumables: Vec<(String, i32)> = sqlx::query_as(
        r#"
        SELECT consumable_type, balance
        FROM user_consumables
        WHERE user_id = $1 AND balance > 0
        "#
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;

    let balances: std::collections::HashMap<String, i32> = consumables
        .into_iter()
        .collect();

    Ok(Json(json!({ "balances": balances })))
}
