// =============================================================================
// NAVA Platform - Graph-Powered Endpoints
// =============================================================================
// Provides graph-optimized endpoints with automatic PostgreSQL fallback:
// - Friend-of-Friend recommendations
// - University network discovery
// - Interest-based matching
// - Fraud detection
// - Social graph analytics
// =============================================================================

use axum::{
    extract::{Path, Query, State},
    Json,
    routing::{get, post},
    Router,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;
use std::sync::Arc;
use tracing::{info, instrument};

use crate::state::AppState;
use crate::error::AppError;
use crate::auth::{Claims, AdminClaims};
use crate::services::graph_service::{RecommendationReason, UserRecommendation};

// -----------------------------------------------------------------------------
// Request/Response Types
// -----------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct RecommendationParams {
    #[serde(default = "default_limit")]
    pub limit: i32,
    #[serde(default)]
    pub include_reasons: bool,
}

fn default_limit() -> i32 {
    20
}

#[derive(Debug, Serialize)]
pub struct RecommendationResponse {
    pub recommendations: Vec<UserRecommendationDto>,
    pub source: String,
    pub total: usize,
}

#[derive(Debug, Serialize)]
pub struct UserRecommendationDto {
    pub user_id: String,
    pub score: f64,
    pub reason: String,
    pub mutual_connections: i32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub shared_interests: Vec<String>,
}

impl From<UserRecommendation> for UserRecommendationDto {
    fn from(rec: UserRecommendation) -> Self {
        let reason = match rec.reason {
            RecommendationReason::FriendOfFriend => "friend_of_friend",
            RecommendationReason::SameUniversity => "same_university",
            RecommendationReason::SharedInterests => "shared_interests",
            RecommendationReason::LocationBased => "location_based",
            RecommendationReason::Popular => "popular",
        };

        Self {
            user_id: rec.user_id.to_string(),
            score: rec.score,
            reason: reason.to_string(),
            mutual_connections: rec.mutual_connections,
            shared_interests: rec.shared_interests,
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FraudAnalysisResponse {
    pub user_id: String,
    pub fraud_score: f64,
    pub risk_level: String,
    pub indicators: FraudIndicators,
}

#[derive(Debug, Serialize)]
pub struct FraudIndicators {
    pub circular_block_patterns: i64,
    pub likes_last_hour: i64,
    pub suspicious_patterns: Vec<String>,
}

#[derive(Debug, Serialize)]
pub struct SocialGraphStats {
    pub total_matches: i64,
    pub total_likes_sent: i64,
    pub total_likes_received: i64,
    pub profile_views: i64,
    pub connection_degree: f64, // How connected user is to network
}

#[derive(Debug, Serialize)]
pub struct UniversityNetworkResponse {
    pub university_name: String,
    pub total_students: i64,
    pub active_students: i64,
    pub potential_matches: Vec<UserRecommendationDto>,
}

// -----------------------------------------------------------------------------
// Handlers
// -----------------------------------------------------------------------------

/// Get friend-of-friend recommendations
/// GET /api/graph/recommendations/fof
#[instrument(skip(state, claims))]
pub async fn get_fof_recommendations(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(params): Query<RecommendationParams>,
) -> Result<Json<RecommendationResponse>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::bad_request("Invalid user ID"))?;

    let recommendations = state.graph()
        .get_friend_of_friend_recommendations(user_id, params.limit)
        .await?;

    let total = recommendations.len();
    let recommendations: Vec<UserRecommendationDto> = recommendations
        .into_iter()
        .map(|r| r.into())
        .collect();

    Ok(Json(RecommendationResponse {
        recommendations,
        source: "postgres".to_string(),
        total,
    }))
}

/// Get university network recommendations
/// GET /api/graph/recommendations/university
#[instrument(skip(state, claims))]
pub async fn get_university_recommendations(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(params): Query<RecommendationParams>,
) -> Result<Json<RecommendationResponse>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::bad_request("Invalid user ID"))?;

    let recommendations = state.graph()
        .get_university_recommendations(user_id, params.limit)
        .await?;

    let total = recommendations.len();
    let recommendations: Vec<UserRecommendationDto> = recommendations
        .into_iter()
        .map(|r| r.into())
        .collect();

    Ok(Json(RecommendationResponse {
        recommendations,
        source: "postgres".to_string(),
        total,
    }))
}

/// Get interest-based recommendations
/// GET /api/graph/recommendations/interests
#[instrument(skip(state, claims))]
pub async fn get_interest_recommendations(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(params): Query<RecommendationParams>,
) -> Result<Json<RecommendationResponse>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::bad_request("Invalid user ID"))?;

    let recommendations = state.graph()
        .get_interest_recommendations(user_id, params.limit)
        .await?;

    let total = recommendations.len();
    let recommendations: Vec<UserRecommendationDto> = recommendations
        .into_iter()
        .map(|r| r.into())
        .collect();

    Ok(Json(RecommendationResponse {
        recommendations,
        source: "postgres".to_string(),
        total,
    }))
}

/// Get combined recommendations (all sources merged and ranked)
/// GET /api/graph/recommendations/combined
#[instrument(skip(state, claims))]
pub async fn get_combined_recommendations(
    State(state): State<Arc<AppState>>,
    claims: Claims,
    Query(params): Query<RecommendationParams>,
) -> Result<Json<RecommendationResponse>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::bad_request("Invalid user ID"))?;

    let graph_service = state.graph();

    // Fetch from all sources concurrently
    let (fof, university, interests) = tokio::join!(
        graph_service.get_friend_of_friend_recommendations(user_id, params.limit),
        graph_service.get_university_recommendations(user_id, params.limit),
        graph_service.get_interest_recommendations(user_id, params.limit)
    );

    // Merge all recommendations
    let mut all_recommendations: Vec<UserRecommendation> = Vec::new();

    if let Ok(recs) = fof {
        all_recommendations.extend(recs);
    }
    if let Ok(recs) = university {
        all_recommendations.extend(recs);
    }
    if let Ok(recs) = interests {
        all_recommendations.extend(recs);
    }

    // Deduplicate by user_id, keeping highest score
    let mut seen: std::collections::HashMap<Uuid, UserRecommendation> = std::collections::HashMap::new();
    for rec in all_recommendations {
        seen.entry(rec.user_id)
            .and_modify(|existing| {
                if rec.score > existing.score {
                    *existing = rec.clone();
                }
            })
            .or_insert(rec);
    }

    // Sort by score descending
    let mut recommendations: Vec<UserRecommendation> = seen.into_values().collect();
    recommendations.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));

    // Limit results
    let recommendations: Vec<UserRecommendationDto> = recommendations
        .into_iter()
        .take(params.limit as usize)
        .map(|r| r.into())
        .collect();

    let total = recommendations.len();

    Ok(Json(RecommendationResponse {
        recommendations,
        source: "postgres".to_string(),
        total,
    }))
}

/// Get fraud analysis for a user (admin only)
/// GET /api/graph/fraud/:user_id
#[instrument(skip(state, _admin))]
pub async fn get_fraud_analysis(
    State(state): State<Arc<AppState>>,
    _admin: AdminClaims, // Requires admin authorization
    Path(user_id): Path<Uuid>,
) -> Result<Json<FraudAnalysisResponse>, AppError> {
    let analysis = state.graph().detect_fraud_patterns(user_id).await?;

    let risk_level = if analysis.fraud_score >= 70.0 {
        "high"
    } else if analysis.fraud_score >= 40.0 {
        "medium"
    } else {
        "low"
    };

    Ok(Json(FraudAnalysisResponse {
        user_id: user_id.to_string(),
        fraud_score: analysis.fraud_score,
        risk_level: risk_level.to_string(),
        indicators: FraudIndicators {
            circular_block_patterns: analysis.circular_block_patterns,
            likes_last_hour: analysis.likes_last_hour,
            suspicious_patterns: analysis.suspicious_patterns,
        },
    }))
}

/// Get social graph statistics for the authenticated user
/// GET /api/graph/stats
#[instrument(skip(state, claims))]
pub async fn get_social_graph_stats(
    State(state): State<Arc<AppState>>,
    claims: Claims,
) -> Result<Json<SocialGraphStats>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::bad_request("Invalid user ID"))?;

    // Query PostgreSQL for stats (more reliable for counts)
    let matches: i64 = sqlx::query_scalar(r#"
        SELECT COUNT(*) FROM matches
        WHERE (user1_id = $1 OR user2_id = $1) AND is_active = true
    "#)
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let likes_sent: i64 = sqlx::query_scalar(r#"
        SELECT COUNT(*) FROM swipes
        WHERE from_user_id = $1 AND action = 'like'
    "#)
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    let likes_received: i64 = sqlx::query_scalar(r#"
        SELECT COUNT(*) FROM swipes
        WHERE to_user_id = $1 AND action = 'like'
    "#)
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    // Profile views from analytics (if tracked)
    let profile_views: i64 = sqlx::query_scalar(r#"
        SELECT COALESCE(
            (SELECT view_count FROM user_analytics WHERE user_id = $1),
            0
        )
    "#)
    .bind(user_id)
    .fetch_one(&state.db)
    .await
    .unwrap_or(0);

    // Calculate connection degree (ratio of matches to likes sent)
    let connection_degree = if likes_sent > 0 {
        (matches as f64 / likes_sent as f64) * 100.0
    } else {
        0.0
    };

    Ok(Json(SocialGraphStats {
        total_matches: matches,
        total_likes_sent: likes_sent,
        total_likes_received: likes_received,
        profile_views,
        connection_degree,
    }))
}

/// Get university network info
/// GET /api/graph/university-network
#[instrument(skip(state, claims))]
pub async fn get_university_network(
    State(state): State<Arc<AppState>>,
    claims: Claims,
) -> Result<Json<UniversityNetworkResponse>, AppError> {
    let user_id = Uuid::parse_str(&claims.sub)
        .map_err(|_| AppError::bad_request("Invalid user ID"))?;

    // Get user's university
    let university: Option<(String, i64, i64)> = sqlx::query_as(r#"
        SELECT u.name,
               (SELECT COUNT(*) FROM student_verifications WHERE university_id = u.id AND verified = true) as total,
               (SELECT COUNT(*) FROM student_verifications sv
                JOIN users usr ON sv.user_id = usr.id
                WHERE sv.university_id = u.id AND sv.verified = true AND usr.is_active = true) as active
        FROM student_verifications sv
        JOIN universities u ON sv.university_id = u.id
        WHERE sv.user_id = $1 AND sv.verified = true
    "#)
    .bind(user_id)
    .fetch_optional(&state.db)
    .await
    .map_err(|e| AppError::internal(&format!("Database error: {}", e)))?;

    let (university_name, total_students, active_students) = university
        .ok_or_else(|| AppError::not_found("User not verified with any university"))?;

    // Get potential matches from same university
    let recommendations = state.graph()
        .get_university_recommendations(user_id, 10)
        .await?;

    let potential_matches: Vec<UserRecommendationDto> = recommendations
        .into_iter()
        .map(|r| r.into())
        .collect();

    Ok(Json(UniversityNetworkResponse {
        university_name,
        total_students,
        active_students,
        potential_matches,
    }))
}

/// Graph health status response
#[derive(Debug, Serialize)]
pub struct GraphHealthResponse {
    pub status: String,
    pub postgres_healthy: bool,
}

/// Get database health status
/// GET /api/graph/health
#[instrument(skip(state))]
pub async fn get_graph_health(
    State(state): State<Arc<AppState>>,
) -> Result<Json<GraphHealthResponse>, AppError> {
    let health = state.graph().health_check().await;
    Ok(Json(GraphHealthResponse {
        status: if health.postgres_healthy { "healthy".to_string() } else { "degraded".to_string() },
        postgres_healthy: health.postgres_healthy,
    }))
}

/// Trigger manual sync (admin only) — no-op now that Neo4j is removed
/// POST /api/graph/sync
#[derive(Debug, Serialize)]
pub struct SyncResponse {
    pub message: String,
}

#[instrument(skip(_state, _admin))]
pub async fn trigger_sync(
    State(_state): State<Arc<AppState>>,
    _admin: AdminClaims,
) -> Result<Json<SyncResponse>, AppError> {
    info!("Sync endpoint called — no-op (Neo4j removed, Postgres is source of truth)");
    Ok(Json(SyncResponse {
        message: "No sync needed — all data is in PostgreSQL".to_string(),
    }))
}

// -----------------------------------------------------------------------------
// Router Configuration
// -----------------------------------------------------------------------------

pub fn graph_routes() -> Router<Arc<AppState>> {
    Router::new()
        // Recommendations
        .route("/recommendations/fof", get(get_fof_recommendations))
        .route("/recommendations/university", get(get_university_recommendations))
        .route("/recommendations/interests", get(get_interest_recommendations))
        .route("/recommendations/combined", get(get_combined_recommendations))
        // Analytics
        .route("/stats", get(get_social_graph_stats))
        .route("/university-network", get(get_university_network))
        // Admin
        .route("/fraud/{user_id}", get(get_fraud_analysis))
        .route("/health", get(get_graph_health))
        .route("/sync", post(trigger_sync))
}
