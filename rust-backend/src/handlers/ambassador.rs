//! Ambassador program endpoints
//!
//! Endpoints:
//! - POST /api/ambassador/login - Ambassador email/password login
//! - GET /api/ambassador/me - Get current ambassador's profile
//! - GET /api/ambassador/performance - Get performance stats
//! - GET /api/ambassador/daily - Get daily signup breakdown
//! - GET /api/ambassador/hourly - Get hourly signup breakdown
//! - GET /api/ambassador/leaderboard - Get leaderboard
//! - GET /api/ambassador/subscribers - Get paginated subscriber list
//! - GET /api/ambassador/subscribers/active - Get currently active subscribers
//!
//! Admin endpoints:
//! - GET /api/admin/ambassadors/daily - Get all ambassadors' daily signups
//! - GET /api/admin/ambassadors/report - Get detailed report

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use chrono::NaiveDate;
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    auth::{Claims, create_token_pair},
    error::AppError,
    services::ambassador::{AmbassadorService, LeaderboardPeriod},
    state::AppState,
};

// ============================================================================
// Request/Response Types
// ============================================================================

#[derive(Deserialize)]
pub struct LoginRequest {
    pub email: String,
    pub password: String,
}

#[derive(Serialize)]
pub struct LoginResponse {
    pub access_token: String,
    pub refresh_token: String,
    pub ambassador: AmbassadorInfo,
}

#[derive(Serialize)]
pub struct AmbassadorInfo {
    pub id: i32,
    pub user_id: i32,
    pub name: String,
    pub email: String,
    pub tier: String,
    pub referral_code: String,
}

#[derive(Deserialize)]
pub struct DailySignupsQuery {
    /// Start date (YYYY-MM-DD), defaults to 30 days ago
    pub start_date: Option<String>,
    /// End date (YYYY-MM-DD), defaults to today
    pub end_date: Option<String>,
}

#[derive(Deserialize)]
pub struct HourlySignupsQuery {
    /// Date (YYYY-MM-DD), defaults to today
    pub date: Option<String>,
}

#[derive(Serialize)]
pub struct HourlyBreakdownResponse {
    pub date: String,
    pub hours: Vec<HourlyStats>,
    pub total_signups: i64,
    pub total_verified: i64,
    pub peak_hour: i32,
}

#[derive(Serialize)]
pub struct HourlyStats {
    pub hour: i32,
    pub hour_label: String,
    pub signups: i64,
    pub verified: i64,
}

#[derive(Deserialize)]
pub struct SubscribersQuery {
    /// Page number (1-indexed)
    pub page: Option<i32>,
    /// Items per page (default 20, max 100)
    pub per_page: Option<i32>,
    /// Filter by status: pending, verified, churned
    pub status: Option<String>,
}

#[derive(Serialize)]
pub struct SubscribersResponse {
    pub subscribers: Vec<SubscriberResponse>,
    pub total: i64,
    pub page: i32,
    pub per_page: i32,
    pub total_pages: i32,
}

#[derive(Serialize)]
pub struct SubscriberResponse {
    pub id: i32,
    pub name: String,
    pub email: Option<String>,
    pub status: String,
    pub signed_up_at: String,
    pub verified_at: Option<String>,
    pub subscription_tier: Option<String>,
    pub is_active: bool,
    pub last_active: Option<String>,
}

#[derive(Serialize)]
pub struct ActiveSubscribersResponse {
    pub count: usize,
    pub subscribers: Vec<SubscriberResponse>,
}

#[derive(Deserialize)]
pub struct LeaderboardQuery {
    /// Period: "today", "week", "month", "all_time"
    pub period: Option<String>,
    /// Limit results (default 20)
    pub limit: Option<i32>,
}

#[derive(Deserialize)]
pub struct AdminDailyQuery {
    /// Date (YYYY-MM-DD), defaults to today
    pub date: Option<String>,
}

#[derive(Serialize)]
pub struct AmbassadorProfileResponse {
    pub id: i32,
    pub user_id: i32,
    pub name: String,
    pub tier: String,
    pub university: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub referral_code: String,
    pub referral_link: String,
    pub status: String,
}

#[derive(Serialize)]
pub struct PerformanceResponse {
    pub ambassador_id: i32,
    pub ambassador_name: String,
    pub tier: String,

    // Stats
    pub today_signups: i64,
    pub this_week_signups: i64,
    pub this_month_signups: i64,
    pub this_month_verified: i64,

    // Target progress
    pub monthly_target: i32,
    pub target_progress_pct: f64,
    pub signups_to_target: i32,

    // Earnings (in INR)
    pub base_stipend: String,
    pub bonus_earned: String,
    pub total_this_month: String,
    pub total_all_time: String,

    // All-time
    pub total_signups: i64,
    pub total_verified: i64,
}

#[derive(Serialize)]
pub struct DailyBreakdownResponse {
    pub days: Vec<DayStats>,
    pub total_signups: i64,
    pub total_verified: i64,
    pub avg_daily_signups: f64,
}

#[derive(Serialize)]
pub struct DayStats {
    pub date: String,
    pub signups: i64,
    pub verified: i64,
    pub conversion_rate: f64,
}

#[derive(Serialize)]
pub struct LeaderboardResponse {
    pub period: String,
    pub entries: Vec<LeaderboardEntryResponse>,
    pub my_rank: Option<i32>,
}

#[derive(Serialize)]
pub struct LeaderboardEntryResponse {
    pub rank: i32,
    pub ambassador_name: String,
    pub university: Option<String>,
    pub city: Option<String>,
    pub tier: String,
    pub signups: i64,
    pub verified: i64,
}

#[derive(Serialize)]
pub struct AdminDailyResponse {
    pub date: String,
    pub total_ambassadors: i32,
    pub total_signups: i64,
    pub total_verified: i64,
    pub ambassadors: Vec<AmbassadorDailyRow>,
}

#[derive(Serialize)]
pub struct AmbassadorDailyRow {
    pub ambassador_id: i32,
    pub name: String,
    pub tier: String,
    pub university: Option<String>,
    pub city: Option<String>,
    pub signups: i64,
    pub verified: i64,
}

// ============================================================================
// Handlers
// ============================================================================

/// Get current ambassador's profile
/// GET /api/ambassador/me
pub async fn get_ambassador_profile(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<AmbassadorProfileResponse>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let service = AmbassadorService::new(state.db.clone());
    let ambassador = service.get_ambassador_by_user(user_id).await?
        .ok_or_else(|| AppError::not_found("You are not an ambassador"))?;

    let base_url = std::env::var("APP_BASE_URL").unwrap_or_else(|_| "https://nava.app".to_string());
    let referral_link = format!("{}/join?ref={}", base_url, ambassador.referral_code);

    Ok(Json(AmbassadorProfileResponse {
        id: ambassador.id,
        user_id: ambassador.user_id,
        name: ambassador.user_name,
        tier: ambassador.tier,
        university: ambassador.university,
        city: ambassador.city,
        region: ambassador.region,
        referral_code: ambassador.referral_code,
        referral_link,
        status: ambassador.status,
    }))
}

/// Get ambassador performance stats
/// GET /api/ambassador/performance
pub async fn get_performance(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<PerformanceResponse>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let service = AmbassadorService::new(state.db.clone());
    let perf = service.get_performance(user_id).await?
        .ok_or_else(|| AppError::not_found("You are not an ambassador"))?;

    let signups_to_target = (perf.monthly_target as i64 - perf.this_month_verified).max(0) as i32;

    Ok(Json(PerformanceResponse {
        ambassador_id: perf.ambassador_id,
        ambassador_name: perf.ambassador_name,
        tier: perf.tier,
        today_signups: perf.today_signups,
        this_week_signups: perf.this_week_signups,
        this_month_signups: perf.this_month_signups,
        this_month_verified: perf.this_month_verified,
        monthly_target: perf.monthly_target,
        target_progress_pct: perf.target_achievement_pct,
        signups_to_target,
        base_stipend: format!("₹{:.0}", perf.base_stipend_inr),
        bonus_earned: format!("₹{:.0}", perf.bonus_earned_inr),
        total_this_month: format!("₹{:.0}", perf.total_earnings_inr),
        total_all_time: format!("₹{:.0}", perf.total_earnings_all_time_inr),
        total_signups: perf.total_signups,
        total_verified: perf.total_verified_signups,
    }))
}

/// Get daily signup breakdown
/// GET /api/ambassador/daily
pub async fn get_daily_breakdown(
    State(state): State<AppState>,
    claims: Claims,
    Query(query): Query<DailySignupsQuery>,
) -> Result<Json<DailyBreakdownResponse>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let today = chrono::Utc::now().date_naive();
    let end_date = query.end_date
        .and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok())
        .unwrap_or(today);
    let start_date = query.start_date
        .and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok())
        .unwrap_or_else(|| today - chrono::Duration::days(30));

    let service = AmbassadorService::new(state.db.clone());
    let days = service.get_daily_signups(user_id, start_date, end_date).await?;

    let total_signups: i64 = days.iter().map(|d| d.signups).sum();
    let total_verified: i64 = days.iter().map(|d| d.verified).sum();
    let num_days = (end_date - start_date).num_days().max(1) as f64;
    let avg_daily = total_signups as f64 / num_days;

    let day_stats: Vec<DayStats> = days.into_iter().map(|d| DayStats {
        date: d.date.format("%Y-%m-%d").to_string(),
        signups: d.signups,
        verified: d.verified,
        conversion_rate: d.conversion_rate,
    }).collect();

    Ok(Json(DailyBreakdownResponse {
        days: day_stats,
        total_signups,
        total_verified,
        avg_daily_signups: avg_daily,
    }))
}

/// Get leaderboard
/// GET /api/ambassador/leaderboard
pub async fn get_leaderboard(
    State(state): State<AppState>,
    claims: Claims,
    Query(query): Query<LeaderboardQuery>,
) -> Result<Json<LeaderboardResponse>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let period_str = query.period.as_deref().unwrap_or("month");
    let period = LeaderboardPeriod::from_str(period_str);
    let limit = query.limit.unwrap_or(20).min(100);

    let service = AmbassadorService::new(state.db.clone());
    let entries = service.get_leaderboard(period, limit).await?;

    // Find current user's rank
    let _my_rank = entries.iter()
        .find(|_e| {
            // We need to check if this ambassador belongs to the current user
            // This is a simplified check - in production you'd query the ambassador ID
            false // Will be set properly below
        })
        .map(|e| e.rank);

    // Get user's actual rank
    let my_ambassador = service.get_ambassador_by_user(user_id).await?;
    let my_rank = my_ambassador.and_then(|a| {
        entries.iter().find(|e| e.ambassador_id == a.id).map(|e| e.rank)
    });

    let response_entries: Vec<LeaderboardEntryResponse> = entries.into_iter().map(|e| {
        LeaderboardEntryResponse {
            rank: e.rank,
            ambassador_name: e.ambassador_name,
            university: e.university,
            city: e.city,
            tier: e.tier,
            signups: e.signups,
            verified: e.verified_signups,
        }
    }).collect();

    Ok(Json(LeaderboardResponse {
        period: period_str.to_string(),
        entries: response_entries,
        my_rank,
    }))
}

/// Ambassador login
/// POST /api/ambassador/login
pub async fn ambassador_login(
    State(state): State<AppState>,
    Json(payload): Json<LoginRequest>,
) -> Result<Json<LoginResponse>, AppError> {
    let service = AmbassadorService::new(state.db.clone());

    let credentials = service.authenticate_ambassador(&payload.email, &payload.password).await?
        .ok_or_else(|| AppError::unauthorized("Invalid email or password"))?;

    // Check if ambassador is active
    if credentials.status != "active" {
        return Err(AppError::forbidden("Ambassador account is not active"));
    }

    // Generate JWT tokens
    let (access_token, refresh_token) = create_token_pair(
        credentials.user_id.into(),
        &state.config.secret_key,
    )?;

    // Get referral code
    let referral_code: Option<(String,)> = sqlx::query_as(
        "SELECT code FROM referral_codes WHERE user_id = $1 AND code_type = 'ambassador'"
    )
    .bind(credentials.user_id)
    .fetch_optional(&state.db)
    .await?;

    let referral_code = referral_code.map(|r| r.0).unwrap_or_default();

    tracing::info!(
        ambassador_id = credentials.ambassador_id,
        user_id = credentials.user_id,
        email = %payload.email,
        "Ambassador logged in"
    );

    Ok(Json(LoginResponse {
        access_token,
        refresh_token,
        ambassador: AmbassadorInfo {
            id: credentials.ambassador_id,
            user_id: credentials.user_id,
            name: credentials.name,
            email: credentials.email,
            tier: credentials.tier,
            referral_code,
        },
    }))
}

/// Get hourly signup breakdown
/// GET /api/ambassador/hourly
pub async fn get_hourly_breakdown(
    State(state): State<AppState>,
    claims: Claims,
    Query(query): Query<HourlySignupsQuery>,
) -> Result<Json<HourlyBreakdownResponse>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let today = chrono::Utc::now().date_naive();
    let date = query.date
        .and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok())
        .unwrap_or(today);

    let service = AmbassadorService::new(state.db.clone());
    let hours = service.get_hourly_signups(user_id, date).await?;

    let total_signups: i64 = hours.iter().map(|h| h.signups).sum();
    let total_verified: i64 = hours.iter().map(|h| h.verified).sum();

    // Find peak hour
    let peak_hour = hours.iter()
        .max_by_key(|h| h.signups)
        .map(|h| h.hour)
        .unwrap_or(0);

    let hourly_stats: Vec<HourlyStats> = hours.into_iter().map(|h| {
        let hour_label = format!("{:02}:00", h.hour);
        HourlyStats {
            hour: h.hour,
            hour_label,
            signups: h.signups,
            verified: h.verified,
        }
    }).collect();

    Ok(Json(HourlyBreakdownResponse {
        date: date.format("%Y-%m-%d").to_string(),
        hours: hourly_stats,
        total_signups,
        total_verified,
        peak_hour,
    }))
}

/// Get paginated subscriber list
/// GET /api/ambassador/subscribers
pub async fn get_subscribers(
    State(state): State<AppState>,
    claims: Claims,
    Query(query): Query<SubscribersQuery>,
) -> Result<Json<SubscribersResponse>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let page = query.page.unwrap_or(1).max(1);
    let per_page = query.per_page.unwrap_or(20).clamp(1, 100);
    let status_filter = query.status.as_deref();

    let service = AmbassadorService::new(state.db.clone());
    let result = service.get_subscribers(user_id, page, per_page, status_filter).await?;

    let subscribers: Vec<SubscriberResponse> = result.subscribers.into_iter().map(|s| {
        SubscriberResponse {
            id: s.id,
            name: s.name,
            email: s.email,
            status: s.status,
            signed_up_at: s.signed_up_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            verified_at: s.verified_at.map(|v| v.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            subscription_tier: s.subscription_tier,
            is_active: s.is_active,
            last_active: s.last_active_at.map(|l| l.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        }
    }).collect();

    Ok(Json(SubscribersResponse {
        subscribers,
        total: result.total,
        page: result.page,
        per_page: result.per_page,
        total_pages: result.total_pages,
    }))
}

/// Get currently active subscribers
/// GET /api/ambassador/subscribers/active
pub async fn get_active_subscribers(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<ActiveSubscribersResponse>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let service = AmbassadorService::new(state.db.clone());
    let active = service.get_active_subscribers(user_id).await?;

    let count = active.len();
    let subscribers: Vec<SubscriberResponse> = active.into_iter().map(|s| {
        SubscriberResponse {
            id: s.id,
            name: s.name,
            email: s.email,
            status: s.status,
            signed_up_at: s.signed_up_at.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
            verified_at: s.verified_at.map(|v| v.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
            subscription_tier: s.subscription_tier,
            is_active: true,
            last_active: s.last_active_at.map(|l| l.format("%Y-%m-%dT%H:%M:%SZ").to_string()),
        }
    }).collect();

    Ok(Json(ActiveSubscribersResponse {
        count,
        subscribers,
    }))
}

/// Get all ambassadors' daily signups (Admin only)
/// GET /api/admin/ambassadors/daily
pub async fn admin_get_daily_signups(
    State(state): State<AppState>,
    claims: Claims,
    Query(query): Query<AdminDailyQuery>,
) -> Result<Json<AdminDailyResponse>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    // Check if user is admin
    let is_admin: Option<(bool,)> = sqlx::query_as(
        "SELECT is_admin FROM users WHERE id = $1"
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    if !matches!(is_admin, Some((true,))) {
        return Err(AppError::forbidden("Admin access required"));
    }

    let today = chrono::Utc::now().date_naive();
    let date = query.date
        .and_then(|d| NaiveDate::parse_from_str(&d, "%Y-%m-%d").ok())
        .unwrap_or(today);

    let service = AmbassadorService::new(state.db.clone());
    let reports = service.get_all_daily_signups(date).await?;

    let total_signups: i64 = reports.iter().map(|r| r.total_signups).sum();
    let total_verified: i64 = reports.iter().map(|r| r.verified_signups).sum();
    let total_ambassadors = reports.len() as i32;

    let ambassador_rows: Vec<AmbassadorDailyRow> = reports.into_iter().map(|r| {
        AmbassadorDailyRow {
            ambassador_id: r.ambassador_id,
            name: r.ambassador_name,
            tier: r.tier,
            university: r.university,
            city: r.city,
            signups: r.total_signups,
            verified: r.verified_signups,
        }
    }).collect();

    Ok(Json(AdminDailyResponse {
        date: date.format("%Y-%m-%d").to_string(),
        total_ambassadors,
        total_signups,
        total_verified,
        ambassadors: ambassador_rows,
    }))
}

/// Record a referral signup (called during user registration)
/// POST /api/referral/record
pub async fn record_referral(
    State(state): State<AppState>,
    Json(payload): Json<RecordReferralPayload>,
) -> Result<Json<serde_json::Value>, AppError> {
    let service = AmbassadorService::new(state.db.clone());

    match service.record_signup(&payload.referral_code, payload.user_id).await? {
        Some(referrer_id) => {
            Ok(Json(json!({
                "success": true,
                "referrer_user_id": referrer_id,
                "message": "Referral recorded successfully"
            })))
        }
        None => {
            Ok(Json(json!({
                "success": false,
                "message": "Invalid referral code"
            })))
        }
    }
}

#[derive(Deserialize)]
pub struct RecordReferralPayload {
    pub referral_code: String,
    pub user_id: i32,
}

/// Verify a referral (called when user completes profile)
/// POST /api/referral/verify
pub async fn verify_referral(
    State(state): State<AppState>,
    claims: Claims,
) -> Result<Json<serde_json::Value>, AppError> {
    let user_id = claims.sub.parse::<i32>()
        .map_err(|_| AppError::unauthorized("Invalid token"))?;

    let service = AmbassadorService::new(state.db.clone());
    let verified = service.verify_referral(user_id).await?;

    Ok(Json(json!({
        "success": verified,
        "message": if verified { "Referral verified" } else { "No pending referral found" }
    })))
}

/// GET /api/referral/my-code
/// Returns the user's personal referral code and invite link (creates one if none exists).
/// iOS uses the invite_url to generate a QR code via CIQRCodeGenerator.
pub async fn get_my_referral_code(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<serde_json::Value>, AppError> {
    use crate::handlers::extract_bearer_token;
    use crate::handlers::decode_access_token;

    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Look up existing active code
    let existing = sqlx::query_as::<_, (String, i32, i32)>(
        "SELECT code, COALESCE(current_uses, 0), reward_value FROM referral_codes WHERE user_id = $1 AND is_active = TRUE LIMIT 1"
    )
    .bind(user_id)
    .fetch_optional(&state.db)
    .await?;

    let (code, uses, reward_days) = if let Some(row) = existing {
        row
    } else {
        // Generate a unique 8-char code: first 4 chars of user name hash + 4 random
        let raw = format!("{}{}", user_id, uuid::Uuid::new_v4());
        let code = raw.chars()
            .filter(|c| c.is_alphanumeric())
            .take(8)
            .collect::<String>()
            .to_uppercase();

        sqlx::query(r#"
            INSERT INTO referral_codes (user_id, code, code_type, reward_type, reward_value, referee_reward_value, is_active)
            VALUES ($1, $2, 'user', 'premium_days', 7, 7, TRUE)
            ON CONFLICT (code) DO NOTHING
        "#)
        .bind(user_id)
        .bind(&code)
        .execute(&state.db)
        .await?;

        (code, 0, 7)
    };

    let invite_url = format!("https://nava.app/invite/{}", code);

    Ok(Json(json!({
        "code": code,
        "invite_url": invite_url,
        "uses": uses,
        "reward_days": reward_days,
        "share_text": format!("Join me on Nava! Use my invite code {} and get {} days of premium free. {}", code, reward_days, invite_url),
    })))
}
