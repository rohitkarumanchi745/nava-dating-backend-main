//! Ambassador Service - Handles ambassador program
//!
//! Endpoints:
//! - POST /apply              - Apply to become ambassador
//! - GET  /status             - Get ambassador status
//! - GET  /referrals          - Get referral stats
//! - GET  /earnings           - Get earnings info
//! - POST /withdraw           - Request withdrawal

use axum::{
    extract::State,
    routing::{get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use tower_http::trace::TraceLayer;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use shared::config::{get_env, get_env_port};
use shared::{AppError, AppResult};

#[derive(Clone)]
struct AppState {
    db: PgPool,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder().with_max_level(Level::INFO).finish();
    tracing::subscriber::set_global_default(subscriber)?;
    dotenvy::dotenv().ok();

    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&get_env("DATABASE_URL"))
        .await?;

    info!("Connected to database");

    let app = Router::new()
        .route("/health", get(health))
        .route("/apply", post(apply_ambassador))
        .route("/status", get(get_status))
        .route("/referrals", get(get_referrals))
        .route("/earnings", get(get_earnings))
        .route("/withdraw", post(request_withdrawal))
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::new(AppState { db }));

    let port = get_env_port("AMBASSADOR_SERVICE_PORT", 8006);
    info!("Ambassador Service starting on 0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"service": "ambassador-service", "status": "healthy"}))
}

#[derive(Debug, Deserialize)]
struct ApplyRequest {
    college_name: String,
    student_id: Option<String>,
    why_ambassador: String,
    social_media_handles: Option<serde_json::Value>,
}

#[derive(Debug, Serialize)]
struct ApplyResponse {
    application_id: i32,
    status: String,
    referral_code: String,
}

async fn apply_ambassador(
    State(state): State<Arc<AppState>>,
    Json(req): Json<ApplyRequest>,
) -> AppResult<Json<ApplyResponse>> {
    let user_id = 1; // Placeholder - extract from JWT

    // Generate referral code
    let referral_code = format!("NAVA{:06}", user_id);

    let application_id = sqlx::query_scalar::<_, i32>(
        r#"
        INSERT INTO ambassadors (user_id, college_name, student_id, why_ambassador,
                                social_media_handles, referral_code, status, created_at)
        VALUES ($1, $2, $3, $4, $5, $6, 'pending', NOW())
        ON CONFLICT (user_id) DO UPDATE SET
            college_name = $2,
            why_ambassador = $4,
            social_media_handles = $5
        RETURNING id
        "#
    )
    .bind(user_id)
    .bind(&req.college_name)
    .bind(&req.student_id)
    .bind(&req.why_ambassador)
    .bind(&req.social_media_handles)
    .bind(&referral_code)
    .fetch_one(&state.db)
    .await?;

    Ok(Json(ApplyResponse {
        application_id,
        status: "pending".to_string(),
        referral_code,
    }))
}

#[derive(Debug, Serialize)]
struct AmbassadorStatus {
    is_ambassador: bool,
    status: String,
    referral_code: Option<String>,
    tier: Option<String>,
    total_referrals: i32,
    total_earnings: i32,
}

async fn get_status(
    State(state): State<Arc<AppState>>,
) -> AppResult<Json<AmbassadorStatus>> {
    let user_id = 1; // Placeholder

    let ambassador = sqlx::query_as::<_, (String, String, Option<String>, i32, i32)>(
        r#"
        SELECT status, referral_code, tier,
               COALESCE(total_referrals, 0), COALESCE(total_earnings, 0)
        FROM ambassadors
        WHERE user_id = $1
        "#
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    match ambassador {
        Some((status, referral_code, tier, total_referrals, total_earnings)) => {
            Ok(Json(AmbassadorStatus {
                is_ambassador: status == "approved",
                status,
                referral_code: Some(referral_code),
                tier,
                total_referrals,
                total_earnings,
            }))
        }
        None => {
            Ok(Json(AmbassadorStatus {
                is_ambassador: false,
                status: "not_applied".to_string(),
                referral_code: None,
                tier: None,
                total_referrals: 0,
                total_earnings: 0,
            }))
        }
    }
}

#[derive(Debug, Serialize)]
struct ReferralInfo {
    total_referrals: i32,
    active_referrals: i32,
    converted_referrals: i32,
    recent_referrals: Vec<RecentReferral>,
}

#[derive(Debug, Serialize)]
struct RecentReferral {
    user_id: i32,
    joined_at: String,
    converted: bool,
}

async fn get_referrals(
    State(state): State<Arc<AppState>>,
) -> AppResult<Json<ReferralInfo>> {
    let user_id = 1; // Placeholder

    // Get referral code for this ambassador
    let referral_code = sqlx::query_scalar::<_, String>(
        "SELECT referral_code FROM ambassadors WHERE user_id = $1"
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Not an ambassador".into()))?;

    // Get referral stats
    let stats = sqlx::query_as::<_, (i64, i64, i64)>(
        r#"
        SELECT
            COUNT(*) as total,
            COUNT(*) FILTER (WHERE is_active = true) as active,
            COUNT(*) FILTER (WHERE has_subscription = true) as converted
        FROM users
        WHERE referral_code = $1
        "#
    )
    .bind(&referral_code)
    .fetch_one(&state.db)
    .await
    .unwrap_or((0, 0, 0));

    // Get recent referrals
    let recent = sqlx::query_as::<_, (i32, chrono::DateTime<chrono::Utc>, bool)>(
        r#"
        SELECT id, created_at, COALESCE(has_subscription, false)
        FROM users
        WHERE referral_code = $1
        ORDER BY created_at DESC
        LIMIT 10
        "#
    )
    .bind(&referral_code)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    Ok(Json(ReferralInfo {
        total_referrals: stats.0 as i32,
        active_referrals: stats.1 as i32,
        converted_referrals: stats.2 as i32,
        recent_referrals: recent
            .into_iter()
            .map(|(user_id, joined_at, converted)| RecentReferral {
                user_id,
                joined_at: joined_at.to_rfc3339(),
                converted,
            })
            .collect(),
    }))
}

#[derive(Debug, Serialize)]
struct EarningsInfo {
    total_earnings: i32,
    pending_earnings: i32,
    withdrawn_earnings: i32,
    commission_rate: i32,
    recent_earnings: Vec<EarningEntry>,
}

#[derive(Debug, Serialize)]
struct EarningEntry {
    amount: i32,
    source: String,
    earned_at: String,
}

async fn get_earnings(
    State(state): State<Arc<AppState>>,
) -> AppResult<Json<EarningsInfo>> {
    let user_id = 1; // Placeholder

    let ambassador = sqlx::query_as::<_, (i32, i32, i32, i32)>(
        r#"
        SELECT total_earnings, pending_earnings, withdrawn_earnings, commission_rate
        FROM ambassadors
        WHERE user_id = $1
        "#
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Not an ambassador".into()))?;

    // Get recent earning entries
    let recent = sqlx::query_as::<_, (i32, String, chrono::DateTime<chrono::Utc>)>(
        r#"
        SELECT amount, source, created_at
        FROM ambassador_earnings
        WHERE ambassador_id = (SELECT id FROM ambassadors WHERE user_id = $1)
        ORDER BY created_at DESC
        LIMIT 20
        "#
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await
    .unwrap_or_default();

    Ok(Json(EarningsInfo {
        total_earnings: ambassador.0,
        pending_earnings: ambassador.1,
        withdrawn_earnings: ambassador.2,
        commission_rate: ambassador.3,
        recent_earnings: recent
            .into_iter()
            .map(|(amount, source, earned_at)| EarningEntry {
                amount,
                source,
                earned_at: earned_at.to_rfc3339(),
            })
            .collect(),
    }))
}

#[derive(Debug, Deserialize)]
struct WithdrawRequest {
    amount: i32,
    upi_id: Option<String>,
    bank_account: Option<String>,
}

#[derive(Debug, Serialize)]
struct WithdrawResponse {
    withdrawal_id: i32,
    amount: i32,
    status: String,
}

async fn request_withdrawal(
    State(state): State<Arc<AppState>>,
    Json(req): Json<WithdrawRequest>,
) -> AppResult<Json<WithdrawResponse>> {
    let user_id = 1; // Placeholder

    // Check if user has enough pending earnings
    let pending = sqlx::query_scalar::<_, i32>(
        "SELECT pending_earnings FROM ambassadors WHERE user_id = $1"
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::NotFound("Not an ambassador".into()))?;

    if pending < req.amount {
        return Err(AppError::BadRequest("Insufficient balance".into()));
    }

    if req.amount < 100 {
        return Err(AppError::BadRequest("Minimum withdrawal is Rs. 100".into()));
    }

    // Create withdrawal request
    let withdrawal_id = sqlx::query_scalar::<_, i32>(
        r#"
        INSERT INTO ambassador_withdrawals (ambassador_id, amount, upi_id, bank_account, status, created_at)
        VALUES ((SELECT id FROM ambassadors WHERE user_id = $1), $2, $3, $4, 'pending', NOW())
        RETURNING id
        "#
    )
    .bind(user_id)
    .bind(req.amount)
    .bind(&req.upi_id)
    .bind(&req.bank_account)
    .fetch_one(&state.db)
    .await?;

    // Update pending earnings
    sqlx::query(
        "UPDATE ambassadors SET pending_earnings = pending_earnings - $1 WHERE user_id = $2"
    )
    .bind(req.amount)
    .bind(user_id)
    .execute(&state.db)
    .await?;

    Ok(Json(WithdrawResponse {
        withdrawal_id,
        amount: req.amount,
        status: "pending".to_string(),
    }))
}
