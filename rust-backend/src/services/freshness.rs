//! Content freshness & anti-gaming: profile decay scoring, media freshness,
//! edit rate limiting, and periodic score recalculation.

use sqlx::PgPool;
use tracing::{info, warn};

/// Freshness decay parameters.
const PROFILE_DECAY_LAMBDA: f64 = 0.03;  // ~30 days half-life
const MEDIA_STALE_DAYS: f64 = 90.0;       // Photos older than 90 days flagged
const FRESH_CONTENT_BOOST: f64 = 1.15;    // 15% boost for fresh content

/// Profile edit rate limits (sliding window).
const MAX_EDITS_PER_HOUR: i64 = 10;
const MAX_EDITS_PER_DAY: i64 = 50;

/// Freshness service for content decay and anti-gaming.
pub struct FreshnessService;

impl FreshnessService {
    /// Compute freshness decay multiplier for a user.
    /// Returns 0.0-1.0 (1.0 = fully fresh, decays exponentially with inactivity).
    pub fn compute_decay(days_since_active: f64) -> f64 {
        (-PROFILE_DECAY_LAMBDA * days_since_active).exp().clamp(0.0, 1.0)
    }

    /// Compute content freshness boost for recently updated media.
    /// Returns multiplier >= 1.0 for fresh content, 1.0 for normal, < 1.0 for stale.
    pub fn media_freshness_multiplier(days_since_photo_update: f64) -> f64 {
        if days_since_photo_update < 7.0 {
            FRESH_CONTENT_BOOST // Recently updated photos get a boost
        } else if days_since_photo_update > MEDIA_STALE_DAYS {
            0.85 // Stale content gets penalized
        } else {
            1.0
        }
    }

    /// Check if a user is rate-limited from editing their profile.
    /// Returns Some(reason) if rate-limited, None if allowed.
    pub async fn check_edit_rate_limit(
        pool: &PgPool,
        user_id: i32,
    ) -> Option<&'static str> {
        // Check hourly limit
        let hourly_count: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM profile_edit_log
               WHERE user_id = $1 AND edited_at > NOW() - INTERVAL '1 hour'"#
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        if hourly_count >= MAX_EDITS_PER_HOUR {
            return Some("Too many profile edits. Please try again in an hour.");
        }

        // Check daily limit
        let daily_count: i64 = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM profile_edit_log
               WHERE user_id = $1 AND edited_at > NOW() - INTERVAL '24 hours'"#
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        if daily_count >= MAX_EDITS_PER_DAY {
            return Some("Daily profile edit limit reached. Please try again tomorrow.");
        }

        None
    }

    /// Log a profile edit for rate limiting.
    pub async fn log_edit(pool: &PgPool, user_id: i32, edit_type: &str) {
        let _ = sqlx::query(
            "INSERT INTO profile_edit_log (user_id, edit_type, edited_at) VALUES ($1, $2, NOW())"
        )
        .bind(user_id)
        .bind(edit_type)
        .execute(pool)
        .await;
    }

    /// Run freshness decay update for all active users.
    /// Should be called by a background job every 6 hours.
    pub async fn run_decay_update(pool: &PgPool) {
        let result = sqlx::query(
            r#"UPDATE users SET freshness_score =
                EXP(-0.03 * EXTRACT(EPOCH FROM (NOW() - COALESCE(last_active, created_at))) / 86400.0)
               WHERE freshness_score IS NOT NULL OR last_active IS NOT NULL"#
        )
        .execute(pool)
        .await;

        match result {
            Ok(r) => info!("Freshness decay updated for {} users", r.rows_affected()),
            Err(e) => warn!("Failed to update freshness decay: {e}"),
        }
    }

    /// Get a user's freshness score from the database, with fallback.
    pub async fn get_user_freshness(pool: &PgPool, user_id: i32) -> f64 {
        sqlx::query_scalar::<_, Option<f64>>(
            "SELECT freshness_score FROM users WHERE id = $1"
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .ok()
        .flatten()
        .unwrap_or(1.0) // Default to fully fresh
    }
}

/// Background job runner for periodic freshness decay updates.
pub struct FreshnessDecayJob;

impl FreshnessDecayJob {
    /// Spawn the freshness decay background job.
    pub fn spawn(pool: PgPool) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(std::time::Duration::from_secs(6 * 3600)); // 6 hours
            loop {
                interval.tick().await;
                info!("Running freshness decay job...");
                FreshnessService::run_decay_update(&pool).await;
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decay_fresh_user() {
        let score = FreshnessService::compute_decay(0.0);
        assert!((score - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_decay_30_day_inactive() {
        let score = FreshnessService::compute_decay(30.0);
        // e^(-0.03 * 30) ≈ 0.407
        assert!(score > 0.35 && score < 0.45, "30-day decay: {}", score);
    }

    #[test]
    fn test_decay_90_day_inactive() {
        let score = FreshnessService::compute_decay(90.0);
        // Should be quite low
        assert!(score < 0.1, "90-day decay should be very low: {}", score);
    }

    #[test]
    fn test_media_freshness_new() {
        let mult = FreshnessService::media_freshness_multiplier(2.0);
        assert!(mult > 1.0, "New content should get boost: {}", mult);
    }

    #[test]
    fn test_media_freshness_stale() {
        let mult = FreshnessService::media_freshness_multiplier(100.0);
        assert!(mult < 1.0, "Stale content should be penalized: {}", mult);
    }
}
