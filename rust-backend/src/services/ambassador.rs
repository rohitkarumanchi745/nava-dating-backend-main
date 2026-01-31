//! Ambassador Program Service
//!
//! Tracks ambassador performance and referral signups:
//! - Daily/weekly/monthly signup tracking
//! - Referral code management
//! - Earnings calculation
//! - Leaderboards and analytics

use chrono::{NaiveDate, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;

use crate::error::AppError;

/// Ambassador tier levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum AmbassadorTier {
    Campus,    // College-level ambassador
    Regional,  // Regional coordinator
    City,      // City-level lead
}

impl AmbassadorTier {
    pub fn as_str(&self) -> &'static str {
        match self {
            AmbassadorTier::Campus => "campus",
            AmbassadorTier::Regional => "regional",
            AmbassadorTier::City => "city",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "regional" => AmbassadorTier::Regional,
            "city" => AmbassadorTier::City,
            _ => AmbassadorTier::Campus,
        }
    }

    /// Monthly target signups by tier
    pub fn monthly_target(&self) -> i32 {
        match self {
            AmbassadorTier::Campus => 50,
            AmbassadorTier::Regional => 200,
            AmbassadorTier::City => 500,
        }
    }

    /// Monthly stipend in paise (₹)
    pub fn monthly_stipend_paise(&self) -> i64 {
        match self {
            AmbassadorTier::Campus => 1000000,   // ₹10,000
            AmbassadorTier::Regional => 2500000, // ₹25,000
            AmbassadorTier::City => 5000000,     // ₹50,000
        }
    }

    /// Bonus per signup above target (in paise)
    pub fn bonus_per_signup_paise(&self) -> i64 {
        match self {
            AmbassadorTier::Campus => 5000,   // ₹50
            AmbassadorTier::Regional => 4000, // ₹40
            AmbassadorTier::City => 3000,     // ₹30
        }
    }
}

/// Ambassador profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ambassador {
    pub id: i32,
    pub user_id: i32,
    pub user_name: String,
    pub tier: String,
    pub university: Option<String>,
    pub city: Option<String>,
    pub region: Option<String>,
    pub referral_code: String,
    pub status: String,
    pub joined_at: chrono::DateTime<Utc>,
}

/// Daily signup stats for an ambassador
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailySignupStats {
    pub date: NaiveDate,
    pub total_signups: i64,
    pub verified_signups: i64,
    pub pending_signups: i64,
}

/// Ambassador performance summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorPerformance {
    pub ambassador_id: i32,
    pub ambassador_name: String,
    pub tier: String,
    pub university: Option<String>,
    pub city: Option<String>,

    // Current period stats
    pub today_signups: i64,
    pub this_week_signups: i64,
    pub this_month_signups: i64,
    pub this_month_verified: i64,

    // Targets
    pub monthly_target: i32,
    pub target_achievement_pct: f64,

    // Earnings
    pub base_stipend_inr: f64,
    pub bonus_earned_inr: f64,
    pub total_earnings_inr: f64,

    // All-time stats
    pub total_signups: i64,
    pub total_verified_signups: i64,
    pub total_earnings_all_time_inr: f64,
}

/// Daily breakdown for detailed analytics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DailyBreakdown {
    pub date: NaiveDate,
    pub signups: i64,
    pub verified: i64,
    pub conversion_rate: f64,
}

/// Ambassador leaderboard entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderboardEntry {
    pub rank: i32,
    pub ambassador_id: i32,
    pub ambassador_name: String,
    pub university: Option<String>,
    pub city: Option<String>,
    pub tier: String,
    pub signups: i64,
    pub verified_signups: i64,
}

/// Ambassador service
pub struct AmbassadorService {
    pool: PgPool,
}

impl AmbassadorService {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }

    /// Get ambassador by user ID
    pub async fn get_ambassador_by_user(&self, user_id: i32) -> Result<Option<Ambassador>, AppError> {
        let ambassador = sqlx::query_as::<_, (
            i32, i32, String, String, Option<String>, Option<String>, Option<String>,
            String, String, chrono::DateTime<Utc>
        )>(
            r#"
            SELECT a.id, a.user_id, u.name, a.tier, a.university, a.city, a.region,
                   r.code, a.status, a.joined_at
            FROM ambassadors a
            JOIN users u ON a.user_id = u.id
            JOIN referral_codes r ON r.user_id = a.user_id AND r.code_type = 'ambassador'
            WHERE a.user_id = $1
            "#
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        Ok(ambassador.map(|a| Ambassador {
            id: a.0,
            user_id: a.1,
            user_name: a.2,
            tier: a.3,
            university: a.4,
            city: a.5,
            region: a.6,
            referral_code: a.7,
            status: a.8,
            joined_at: a.9,
        }))
    }

    /// Get daily signup stats for an ambassador
    pub async fn get_daily_signups(
        &self,
        ambassador_user_id: i32,
        start_date: NaiveDate,
        end_date: NaiveDate,
    ) -> Result<Vec<DailyBreakdown>, AppError> {
        let stats = sqlx::query_as::<_, (NaiveDate, i64, i64)>(
            r#"
            SELECT
                DATE(ru.created_at) as signup_date,
                COUNT(*) as total_signups,
                COUNT(*) FILTER (WHERE ru.status = 'verified') as verified_signups
            FROM referral_uses ru
            JOIN referral_codes rc ON ru.referral_code_id = rc.id
            WHERE rc.user_id = $1
              AND DATE(ru.created_at) BETWEEN $2 AND $3
            GROUP BY DATE(ru.created_at)
            ORDER BY signup_date DESC
            "#
        )
        .bind(ambassador_user_id)
        .bind(start_date)
        .bind(end_date)
        .fetch_all(&self.pool)
        .await?;

        Ok(stats.into_iter().map(|(date, signups, verified)| {
            DailyBreakdown {
                date,
                signups,
                verified,
                conversion_rate: if signups > 0 {
                    (verified as f64 / signups as f64) * 100.0
                } else {
                    0.0
                },
            }
        }).collect())
    }

    /// Get ambassador performance summary
    pub async fn get_performance(&self, ambassador_user_id: i32) -> Result<Option<AmbassadorPerformance>, AppError> {
        let ambassador = self.get_ambassador_by_user(ambassador_user_id).await?;
        let ambassador = match ambassador {
            Some(a) => a,
            None => return Ok(None),
        };

        let tier = AmbassadorTier::from_str(&ambassador.tier);

        // Get today's signups
        let today_stats: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM referral_uses ru
            JOIN referral_codes rc ON ru.referral_code_id = rc.id
            WHERE rc.user_id = $1 AND DATE(ru.created_at) = CURRENT_DATE
            "#
        )
        .bind(ambassador_user_id)
        .fetch_one(&self.pool)
        .await?;

        // Get this week's signups
        let week_stats: (i64,) = sqlx::query_as(
            r#"
            SELECT COUNT(*)
            FROM referral_uses ru
            JOIN referral_codes rc ON ru.referral_code_id = rc.id
            WHERE rc.user_id = $1
              AND ru.created_at >= DATE_TRUNC('week', CURRENT_DATE)
            "#
        )
        .bind(ambassador_user_id)
        .fetch_one(&self.pool)
        .await?;

        // Get this month's signups (total and verified)
        let month_stats: (i64, i64) = sqlx::query_as(
            r#"
            SELECT
                COUNT(*) as total,
                COUNT(*) FILTER (WHERE ru.status = 'verified') as verified
            FROM referral_uses ru
            JOIN referral_codes rc ON ru.referral_code_id = rc.id
            WHERE rc.user_id = $1
              AND ru.created_at >= DATE_TRUNC('month', CURRENT_DATE)
            "#
        )
        .bind(ambassador_user_id)
        .fetch_one(&self.pool)
        .await?;

        // Get all-time stats
        let all_time_stats: (i64, i64, i64) = sqlx::query_as(
            r#"
            SELECT
                a.total_signups,
                a.total_verified_signups,
                a.total_earnings_cents
            FROM ambassadors a
            WHERE a.user_id = $1
            "#
        )
        .bind(ambassador_user_id)
        .fetch_one(&self.pool)
        .await?;

        // Calculate earnings
        let monthly_target = tier.monthly_target();
        let verified_this_month = month_stats.1;
        let above_target = (verified_this_month as i32 - monthly_target).max(0);

        let base_stipend = tier.monthly_stipend_paise() as f64 / 100.0;
        let bonus = (above_target as i64 * tier.bonus_per_signup_paise()) as f64 / 100.0;

        let target_pct = if monthly_target > 0 {
            (verified_this_month as f64 / monthly_target as f64) * 100.0
        } else {
            0.0
        };

        Ok(Some(AmbassadorPerformance {
            ambassador_id: ambassador.id,
            ambassador_name: ambassador.user_name,
            tier: ambassador.tier,
            university: ambassador.university,
            city: ambassador.city,
            today_signups: today_stats.0,
            this_week_signups: week_stats.0,
            this_month_signups: month_stats.0,
            this_month_verified: month_stats.1,
            monthly_target,
            target_achievement_pct: target_pct,
            base_stipend_inr: base_stipend,
            bonus_earned_inr: bonus,
            total_earnings_inr: base_stipend + bonus,
            total_signups: all_time_stats.0,
            total_verified_signups: all_time_stats.1,
            total_earnings_all_time_inr: all_time_stats.2 as f64 / 100.0,
        }))
    }

    /// Get all ambassadors' daily signups for admin dashboard
    pub async fn get_all_daily_signups(&self, date: NaiveDate) -> Result<Vec<AmbassadorDailyReport>, AppError> {
        let reports = sqlx::query_as::<_, (
            i32, String, String, Option<String>, Option<String>, i64, i64
        )>(
            r#"
            SELECT
                a.id,
                u.name,
                a.tier,
                a.university,
                a.city,
                COUNT(ru.id) as total_signups,
                COUNT(ru.id) FILTER (WHERE ru.status = 'verified') as verified_signups
            FROM ambassadors a
            JOIN users u ON a.user_id = u.id
            LEFT JOIN referral_codes rc ON rc.user_id = a.user_id AND rc.code_type = 'ambassador'
            LEFT JOIN referral_uses ru ON ru.referral_code_id = rc.id
                AND DATE(ru.created_at) = $1
            WHERE a.status = 'active'
            GROUP BY a.id, u.name, a.tier, a.university, a.city
            ORDER BY total_signups DESC
            "#
        )
        .bind(date)
        .fetch_all(&self.pool)
        .await?;

        Ok(reports.into_iter().map(|r| AmbassadorDailyReport {
            ambassador_id: r.0,
            ambassador_name: r.1,
            tier: r.2,
            university: r.3,
            city: r.4,
            date,
            total_signups: r.5,
            verified_signups: r.6,
        }).collect())
    }

    /// Get leaderboard for a period
    pub async fn get_leaderboard(
        &self,
        period: LeaderboardPeriod,
        limit: i32,
    ) -> Result<Vec<LeaderboardEntry>, AppError> {
        let date_filter = match period {
            LeaderboardPeriod::Today => "DATE(ru.created_at) = CURRENT_DATE",
            LeaderboardPeriod::ThisWeek => "ru.created_at >= DATE_TRUNC('week', CURRENT_DATE)",
            LeaderboardPeriod::ThisMonth => "ru.created_at >= DATE_TRUNC('month', CURRENT_DATE)",
            LeaderboardPeriod::AllTime => "1=1",
        };

        let query = format!(
            r#"
            SELECT
                ROW_NUMBER() OVER (ORDER BY COUNT(ru.id) FILTER (WHERE ru.status = 'verified') DESC) as rank,
                a.id,
                u.name,
                a.university,
                a.city,
                a.tier,
                COUNT(ru.id) as total_signups,
                COUNT(ru.id) FILTER (WHERE ru.status = 'verified') as verified_signups
            FROM ambassadors a
            JOIN users u ON a.user_id = u.id
            LEFT JOIN referral_codes rc ON rc.user_id = a.user_id AND rc.code_type = 'ambassador'
            LEFT JOIN referral_uses ru ON ru.referral_code_id = rc.id AND {}
            WHERE a.status = 'active'
            GROUP BY a.id, u.name, a.university, a.city, a.tier
            HAVING COUNT(ru.id) > 0
            ORDER BY verified_signups DESC
            LIMIT $1
            "#,
            date_filter
        );

        let entries = sqlx::query_as::<_, (i64, i32, String, Option<String>, Option<String>, String, i64, i64)>(
            &query
        )
        .bind(limit)
        .fetch_all(&self.pool)
        .await?;

        Ok(entries.into_iter().map(|e| LeaderboardEntry {
            rank: e.0 as i32,
            ambassador_id: e.1,
            ambassador_name: e.2,
            university: e.3,
            city: e.4,
            tier: e.5,
            signups: e.6,
            verified_signups: e.7,
        }).collect())
    }

    /// Record a signup via referral code
    pub async fn record_signup(
        &self,
        referral_code: &str,
        new_user_id: i32,
    ) -> Result<Option<i32>, AppError> {
        // Find the referral code
        let code = sqlx::query_as::<_, (i32, i32, bool, Option<i32>, i32)>(
            r#"
            SELECT id, user_id, is_active, max_uses, current_uses
            FROM referral_codes
            WHERE code = $1
            "#
        )
        .bind(referral_code)
        .fetch_optional(&self.pool)
        .await?;

        let (code_id, referrer_user_id, is_active, max_uses, current_uses) = match code {
            Some(c) => c,
            None => return Ok(None),
        };

        // Check if code is valid
        if !is_active {
            return Err(AppError::bad_request("Referral code is inactive"));
        }

        if let Some(max) = max_uses {
            if current_uses >= max {
                return Err(AppError::bad_request("Referral code has reached maximum uses"));
            }
        }

        // Check if user was already referred
        let already_referred: Option<(i32,)> = sqlx::query_as(
            "SELECT id FROM referral_uses WHERE referee_user_id = $1"
        )
        .bind(new_user_id)
        .fetch_optional(&self.pool)
        .await?;

        if already_referred.is_some() {
            return Err(AppError::bad_request("User was already referred"));
        }

        // Record the referral
        let mut tx = self.pool.begin().await?;

        sqlx::query(
            r#"
            INSERT INTO referral_uses (referral_code_id, referrer_user_id, referee_user_id, status)
            VALUES ($1, $2, $3, 'pending')
            "#
        )
        .bind(code_id)
        .bind(referrer_user_id)
        .bind(new_user_id)
        .execute(&mut *tx)
        .await?;

        // Update code usage count
        sqlx::query(
            "UPDATE referral_codes SET current_uses = current_uses + 1 WHERE id = $1"
        )
        .bind(code_id)
        .execute(&mut *tx)
        .await?;

        // Update ambassador stats if this is an ambassador code
        sqlx::query(
            r#"
            UPDATE ambassadors
            SET total_signups = total_signups + 1
            WHERE user_id = $1
            "#
        )
        .bind(referrer_user_id)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        tracing::info!(
            referral_code = referral_code,
            referrer_id = referrer_user_id,
            new_user_id = new_user_id,
            "Recorded referral signup"
        );

        Ok(Some(referrer_user_id))
    }

    /// Verify a referral (user completed profile, etc.)
    pub async fn verify_referral(&self, referee_user_id: i32) -> Result<bool, AppError> {
        let mut tx = self.pool.begin().await?;

        // Update referral status
        let result = sqlx::query(
            r#"
            UPDATE referral_uses
            SET status = 'verified', verified_at = NOW()
            WHERE referee_user_id = $1 AND status = 'pending'
            RETURNING referrer_user_id
            "#
        )
        .bind(referee_user_id)
        .fetch_optional(&mut *tx)
        .await?;

        if result.is_none() {
            return Ok(false);
        }

        // Get referrer user ID
        let referrer: (i32,) = sqlx::query_as(
            "SELECT referrer_user_id FROM referral_uses WHERE referee_user_id = $1"
        )
        .bind(referee_user_id)
        .fetch_one(&mut *tx)
        .await?;

        // Update ambassador verified count
        sqlx::query(
            r#"
            UPDATE ambassadors
            SET total_verified_signups = total_verified_signups + 1
            WHERE user_id = $1
            "#
        )
        .bind(referrer.0)
        .execute(&mut *tx)
        .await?;

        tx.commit().await?;

        tracing::info!(
            referee_id = referee_user_id,
            referrer_id = referrer.0,
            "Verified referral"
        );

        Ok(true)
    }

    /// Authenticate ambassador by email and password
    pub async fn authenticate_ambassador(
        &self,
        email: &str,
        password: &str,
    ) -> Result<Option<AmbassadorCredentials>, AppError> {
        // First, find the user by email
        let user = sqlx::query_as::<_, (i32, String, String)>(
            r#"
            SELECT id, name, password_hash
            FROM users
            WHERE email = $1
            "#
        )
        .bind(email)
        .fetch_optional(&self.pool)
        .await?;

        let (user_id, user_name, password_hash) = match user {
            Some(u) => u,
            None => return Ok(None),
        };

        // Verify password using bcrypt
        let valid = bcrypt::verify(password, &password_hash).unwrap_or(false);
        if !valid {
            return Ok(None);
        }

        // Check if user is an ambassador
        let ambassador = sqlx::query_as::<_, (i32, String, String)>(
            r#"
            SELECT id, tier, status
            FROM ambassadors
            WHERE user_id = $1 AND status = 'active'
            "#
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await?;

        match ambassador {
            Some((amb_id, tier, status)) => {
                Ok(Some(AmbassadorCredentials {
                    ambassador_id: amb_id,
                    user_id,
                    email: email.to_string(),
                    password_hash,
                    name: user_name,
                    tier,
                    status,
                }))
            }
            None => Ok(None),
        }
    }

    /// Get hourly signup breakdown for a specific date
    pub async fn get_hourly_signups(
        &self,
        ambassador_user_id: i32,
        date: NaiveDate,
    ) -> Result<Vec<HourlyBreakdown>, AppError> {
        let stats = sqlx::query_as::<_, (i32, i64, i64)>(
            r#"
            SELECT
                EXTRACT(HOUR FROM ru.created_at)::int as signup_hour,
                COUNT(*) as total_signups,
                COUNT(*) FILTER (WHERE ru.status = 'verified') as verified_signups
            FROM referral_uses ru
            JOIN referral_codes rc ON ru.referral_code_id = rc.id
            WHERE rc.user_id = $1
              AND DATE(ru.created_at) = $2
            GROUP BY EXTRACT(HOUR FROM ru.created_at)
            ORDER BY signup_hour
            "#
        )
        .bind(ambassador_user_id)
        .bind(date)
        .fetch_all(&self.pool)
        .await?;

        // Fill in all 24 hours with zeros for missing hours
        let mut hourly_map: std::collections::HashMap<i32, (i64, i64)> = std::collections::HashMap::new();
        for (hour, signups, verified) in stats {
            hourly_map.insert(hour, (signups, verified));
        }

        let result: Vec<HourlyBreakdown> = (0..24)
            .map(|hour| {
                let (signups, verified) = hourly_map.get(&hour).copied().unwrap_or((0, 0));
                HourlyBreakdown {
                    hour,
                    signups,
                    verified,
                }
            })
            .collect();

        Ok(result)
    }

    /// Get paginated list of subscribers for an ambassador
    pub async fn get_subscribers(
        &self,
        ambassador_user_id: i32,
        page: i32,
        per_page: i32,
        status_filter: Option<&str>,
    ) -> Result<SubscribersPage, AppError> {
        let offset = (page - 1) * per_page;

        // Build status filter clause
        let status_clause = match status_filter {
            Some("pending") => "AND ru.status = 'pending'",
            Some("verified") => "AND ru.status = 'verified'",
            Some("churned") => "AND ru.status = 'churned'",
            _ => "",
        };

        // Get total count
        let count_query = format!(
            r#"
            SELECT COUNT(*)
            FROM referral_uses ru
            JOIN referral_codes rc ON ru.referral_code_id = rc.id
            WHERE rc.user_id = $1
            {}
            "#,
            status_clause
        );

        let total: (i64,) = sqlx::query_as(&count_query)
            .bind(ambassador_user_id)
            .fetch_one(&self.pool)
            .await?;

        // Get subscribers with user info
        let query = format!(
            r#"
            SELECT
                u.id,
                u.name,
                u.email,
                u.phone,
                ru.status,
                ru.created_at,
                ru.verified_at,
                s.tier as subscription_tier,
                u.last_active_at,
                CASE
                    WHEN u.last_active_at > NOW() - INTERVAL '15 minutes' THEN true
                    ELSE false
                END as is_active
            FROM referral_uses ru
            JOIN referral_codes rc ON ru.referral_code_id = rc.id
            JOIN users u ON ru.referee_user_id = u.id
            LEFT JOIN subscriptions s ON s.user_id = u.id AND s.status = 'active'
            WHERE rc.user_id = $1
            {}
            ORDER BY ru.created_at DESC
            LIMIT $2 OFFSET $3
            "#,
            status_clause
        );

        let subscribers = sqlx::query_as::<_, (
            i32, String, Option<String>, Option<String>, String,
            chrono::DateTime<Utc>, Option<chrono::DateTime<Utc>>,
            Option<String>, Option<chrono::DateTime<Utc>>, bool
        )>(&query)
        .bind(ambassador_user_id)
        .bind(per_page)
        .bind(offset)
        .fetch_all(&self.pool)
        .await?;

        let subscribers: Vec<SubscriberInfo> = subscribers
            .into_iter()
            .map(|(id, name, email, phone, status, signed_up_at, verified_at, sub_tier, last_active, is_active)| {
                SubscriberInfo {
                    id,
                    name,
                    email,
                    phone,
                    status,
                    signed_up_at,
                    verified_at,
                    subscription_tier: sub_tier,
                    last_active_at: last_active,
                    is_active,
                }
            })
            .collect();

        let total_pages = ((total.0 as f64) / (per_page as f64)).ceil() as i32;

        Ok(SubscribersPage {
            subscribers,
            total: total.0,
            page,
            per_page,
            total_pages,
        })
    }

    /// Get currently active subscribers (online in last 15 minutes)
    pub async fn get_active_subscribers(
        &self,
        ambassador_user_id: i32,
    ) -> Result<Vec<SubscriberInfo>, AppError> {
        let subscribers = sqlx::query_as::<_, (
            i32, String, Option<String>, Option<String>, String,
            chrono::DateTime<Utc>, Option<chrono::DateTime<Utc>>,
            Option<String>, Option<chrono::DateTime<Utc>>
        )>(
            r#"
            SELECT
                u.id,
                u.name,
                u.email,
                u.phone,
                ru.status,
                ru.created_at,
                ru.verified_at,
                s.tier as subscription_tier,
                u.last_active_at
            FROM referral_uses ru
            JOIN referral_codes rc ON ru.referral_code_id = rc.id
            JOIN users u ON ru.referee_user_id = u.id
            LEFT JOIN subscriptions s ON s.user_id = u.id AND s.status = 'active'
            WHERE rc.user_id = $1
              AND u.last_active_at > NOW() - INTERVAL '15 minutes'
            ORDER BY u.last_active_at DESC
            "#
        )
        .bind(ambassador_user_id)
        .fetch_all(&self.pool)
        .await?;

        let subscribers: Vec<SubscriberInfo> = subscribers
            .into_iter()
            .map(|(id, name, email, phone, status, signed_up_at, verified_at, sub_tier, last_active)| {
                SubscriberInfo {
                    id,
                    name,
                    email,
                    phone,
                    status,
                    signed_up_at,
                    verified_at,
                    subscription_tier: sub_tier,
                    last_active_at: last_active,
                    is_active: true,
                }
            })
            .collect();

        Ok(subscribers)
    }
}

/// Ambassador daily report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorDailyReport {
    pub ambassador_id: i32,
    pub ambassador_name: String,
    pub tier: String,
    pub university: Option<String>,
    pub city: Option<String>,
    pub date: NaiveDate,
    pub total_signups: i64,
    pub verified_signups: i64,
}

/// Leaderboard period
#[derive(Debug, Clone, Copy)]
pub enum LeaderboardPeriod {
    Today,
    ThisWeek,
    ThisMonth,
    AllTime,
}

impl LeaderboardPeriod {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "today" => LeaderboardPeriod::Today,
            "week" | "this_week" => LeaderboardPeriod::ThisWeek,
            "month" | "this_month" => LeaderboardPeriod::ThisMonth,
            _ => LeaderboardPeriod::AllTime,
        }
    }
}

/// Hourly signup breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HourlyBreakdown {
    pub hour: i32,
    pub signups: i64,
    pub verified: i64,
}

/// Subscriber info (user who signed up via referral)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriberInfo {
    pub id: i32,
    pub name: String,
    pub email: Option<String>,
    pub phone: Option<String>,
    pub status: String,  // pending, verified, churned
    pub signed_up_at: chrono::DateTime<Utc>,
    pub verified_at: Option<chrono::DateTime<Utc>>,
    pub subscription_tier: Option<String>,
    pub last_active_at: Option<chrono::DateTime<Utc>>,
    pub is_active: bool,
}

/// Paginated subscribers response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscribersPage {
    pub subscribers: Vec<SubscriberInfo>,
    pub total: i64,
    pub page: i32,
    pub per_page: i32,
    pub total_pages: i32,
}

/// Ambassador credentials for login
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AmbassadorCredentials {
    pub ambassador_id: i32,
    pub user_id: i32,
    pub email: String,
    pub password_hash: String,
    pub name: String,
    pub tier: String,
    pub status: String,
}
