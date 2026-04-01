//! Ads endpoints for ad monetization
//!
//! Endpoints:
//! - GET /ads/placements - Get all active ad placements (iOS)
//! - GET /api/ads/request - Request an ad for display
//! - POST /api/ads/impression | /ads/impression - Record ad impression
//! - POST /api/ads/rewarded-complete | /ads/rewarded/complete - Record rewarded ad completion
//! - GET /api/ads/rewards-balance | /ads/balances - Get consumable balances

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
    .fetch_all(state.read_pool())
    .await?;

    let balances: std::collections::HashMap<String, i32> = consumables
        .into_iter()
        .collect();

    Ok(Json(json!({ "balances": balances })))
}

/// Get all active ad placements with frequency caps and network unit IDs
/// GET /ads/placements
pub async fn get_all_placements(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let ads_service = state.ads_service.as_ref()
        .ok_or_else(|| AppError::internal("Ads service not configured"))?;

    let is_premium = !ads_service.should_show_ads(user_id).await?;

    #[derive(sqlx::FromRow, Serialize)]
    struct PlacementRow {
        placement_id: String,
        name: String,
        placement_type: String,
        location: String,
        admob_unit_id: Option<String>,
        facebook_placement_id: Option<String>,
        unity_placement_id: Option<String>,
        show_to_free_users: bool,
        show_to_premium_users: bool,
        frequency_cap_per_hour: i32,
        is_active: bool,
    }

    let placements = sqlx::query_as::<_, PlacementRow>(
        r#"
        SELECT placement_id, name, placement_type, location,
               admob_unit_id, facebook_placement_id, unity_placement_id,
               show_to_free_users, show_to_premium_users, frequency_cap_per_hour, is_active
        FROM ad_placements
        WHERE is_active = true
        ORDER BY placement_id
        "#
    )
    .fetch_all(state.read_pool())
    .await?;

    // Filter based on user's premium status
    let filtered: Vec<&PlacementRow> = placements.iter().filter(|p| {
        if is_premium { p.show_to_premium_users } else { p.show_to_free_users }
    }).collect();

    let result: Vec<serde_json::Value> = filtered.iter().map(|p| {
        json!({
            "placement_id": p.placement_id,
            "name": p.name,
            "ad_type": p.placement_type,
            "location": p.location,
            "admob_unit_id": p.admob_unit_id,
            "facebook_placement_id": p.facebook_placement_id,
            "unity_placement_id": p.unity_placement_id,
            "frequency_cap_per_hour": p.frequency_cap_per_hour,
        })
    }).collect();

    Ok(Json(json!({
        "placements": result,
        "is_premium": is_premium,
        "show_ads": !is_premium,
    })))
}
