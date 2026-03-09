//! Trust & Safety service: device fingerprinting, behavioral scoring, graph anomaly detection.

use sqlx::PgPool;
use serde::{Deserialize, Serialize};
use tracing::warn;

/// Trust score result from combined signal analysis.
#[derive(Debug, Clone, Serialize)]
pub struct TrustScore {
    pub risk_score: f64,
    pub signals: Vec<TrustSignal>,
    pub action: TrustAction,
}

/// Individual trust signal contributing to the overall risk score.
#[derive(Debug, Clone, Serialize)]
pub struct TrustSignal {
    pub signal_type: String,
    pub value: f64,
    pub description: String,
}

/// Action to take based on trust assessment.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub enum TrustAction {
    Allow,
    Throttle,
    FlagForReview,
    AutoBan,
}

/// Device fingerprint extracted from client headers.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceFingerprint {
    pub device_model: Option<String>,
    pub os_version: Option<String>,
    pub screen_resolution: Option<String>,
    pub timezone: Option<String>,
    pub language: Option<String>,
    pub user_agent: Option<String>,
    pub fingerprint_hash: String,
}

impl DeviceFingerprint {
    /// Extract device fingerprint from HTTP headers.
    pub fn from_headers(headers: &axum::http::HeaderMap) -> Self {
        let device_model = headers.get("X-Device-Model").and_then(|v| v.to_str().ok()).map(String::from);
        let os_version = headers.get("X-OS-Version").and_then(|v| v.to_str().ok()).map(String::from);
        let screen_resolution = headers.get("X-Screen-Resolution").and_then(|v| v.to_str().ok()).map(String::from);
        let timezone = headers.get("X-Timezone").and_then(|v| v.to_str().ok()).map(String::from);
        let language = headers.get("Accept-Language").and_then(|v| v.to_str().ok()).map(String::from);
        let user_agent = headers.get("User-Agent").and_then(|v| v.to_str().ok()).map(String::from);

        // Create composite fingerprint hash
        let composite = format!(
            "{}|{}|{}|{}|{}",
            device_model.as_deref().unwrap_or(""),
            os_version.as_deref().unwrap_or(""),
            screen_resolution.as_deref().unwrap_or(""),
            timezone.as_deref().unwrap_or(""),
            user_agent.as_deref().unwrap_or(""),
        );
        let fingerprint_hash = format!("{:x}", md5_hash(composite.as_bytes()));

        Self {
            device_model,
            os_version,
            screen_resolution,
            timezone,
            language,
            user_agent,
            fingerprint_hash,
        }
    }
}

/// Simple hash for fingerprinting (not cryptographic security).
fn md5_hash(data: &[u8]) -> u64 {
    let mut hash: u64 = 5381;
    for &byte in data {
        hash = hash.wrapping_mul(33).wrapping_add(byte as u64);
    }
    hash
}

/// Trust & Safety service for risk assessment.
pub struct TrustSafetyService {
    auto_ban_threshold: f64,
    flag_threshold: f64,
    throttle_threshold: f64,
}

impl TrustSafetyService {
    pub fn new(auto_ban_threshold: f64) -> Self {
        Self {
            auto_ban_threshold,
            flag_threshold: auto_ban_threshold * 0.7,
            throttle_threshold: auto_ban_threshold * 0.5,
        }
    }

    /// Score a user's trustworthiness based on behavioral signals.
    pub async fn score_user(
        &self,
        pool: &PgPool,
        user_id: i32,
        fingerprint: &DeviceFingerprint,
    ) -> TrustScore {
        let mut signals = Vec::new();
        let mut total_risk = 0.0;

        // 1. Swipe velocity check (too many swipes in short time)
        if let Ok(velocity) = self.check_swipe_velocity(pool, user_id).await {
            if velocity > 0.0 {
                signals.push(TrustSignal {
                    signal_type: "swipe_velocity".to_string(),
                    value: velocity,
                    description: "Unusually high swipe rate".to_string(),
                });
                total_risk += velocity * 0.3;
            }
        }

        // 2. Report frequency check
        if let Ok(report_risk) = self.check_report_frequency(pool, user_id).await {
            if report_risk > 0.0 {
                signals.push(TrustSignal {
                    signal_type: "report_frequency".to_string(),
                    value: report_risk,
                    description: "Multiple reports received".to_string(),
                });
                total_risk += report_risk * 0.4;
            }
        }

        // 3. Account age check (new accounts are riskier)
        if let Ok(age_risk) = self.check_account_age(pool, user_id).await {
            if age_risk > 0.0 {
                signals.push(TrustSignal {
                    signal_type: "account_age".to_string(),
                    value: age_risk,
                    description: "New account with high activity".to_string(),
                });
                total_risk += age_risk * 0.15;
            }
        }

        // 4. Device fingerprint check (multi-account detection)
        if let Ok(device_risk) = self.check_device_fingerprint(pool, user_id, fingerprint).await {
            if device_risk > 0.0 {
                signals.push(TrustSignal {
                    signal_type: "device_fingerprint".to_string(),
                    value: device_risk,
                    description: "Device associated with other accounts".to_string(),
                });
                total_risk += device_risk * 0.3;
            }
        }

        // 5. Message pattern check
        if let Ok(msg_risk) = self.check_message_patterns(pool, user_id).await {
            if msg_risk > 0.0 {
                signals.push(TrustSignal {
                    signal_type: "message_pattern".to_string(),
                    value: msg_risk,
                    description: "Suspicious messaging patterns".to_string(),
                });
                total_risk += msg_risk * 0.25;
            }
        }

        let total_risk = total_risk.clamp(0.0, 1.0);
        let action = if total_risk >= self.auto_ban_threshold {
            TrustAction::AutoBan
        } else if total_risk >= self.flag_threshold {
            TrustAction::FlagForReview
        } else if total_risk >= self.throttle_threshold {
            TrustAction::Throttle
        } else {
            TrustAction::Allow
        };

        TrustScore {
            risk_score: total_risk,
            signals,
            action,
        }
    }

    /// Check swipe velocity: > 100 swipes in last 10 minutes is suspicious.
    async fn check_swipe_velocity(&self, pool: &PgPool, user_id: i32) -> Result<f64, sqlx::Error> {
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM interaction_events WHERE user_id = $1 AND event_type IN ('like', 'pass') AND created_at > NOW() - INTERVAL '10 minutes'"
        )
        .bind(user_id)
        .fetch_one(pool)
        .await?;

        // > 100 in 10 min = max risk, < 20 = no risk
        Ok(((count as f64 - 20.0) / 80.0).clamp(0.0, 1.0))
    }

    /// Check how many reports the user has received recently.
    async fn check_report_frequency(&self, pool: &PgPool, user_id: i32) -> Result<f64, sqlx::Error> {
        // Check if reports table exists; gracefully return 0 if not
        let count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM interaction_events WHERE target_user_id = $1 AND event_type = 'report' AND created_at > NOW() - INTERVAL '30 days'"
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        // > 5 reports in 30 days = max risk
        Ok(((count as f64) / 5.0).clamp(0.0, 1.0))
    }

    /// New accounts with very high activity are suspicious.
    async fn check_account_age(&self, pool: &PgPool, user_id: i32) -> Result<f64, sqlx::Error> {
        #[derive(sqlx::FromRow)]
        struct AgeRow {
            days_old: Option<f64>,
            interaction_count: Option<i64>,
        }

        let row = sqlx::query_as::<_, AgeRow>(
            r#"SELECT
                EXTRACT(EPOCH FROM (NOW() - u.created_at)) / 86400.0 AS days_old,
                (SELECT COUNT(*) FROM interaction_events WHERE user_id = $1) AS interaction_count
               FROM users u WHERE u.id = $1"#
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await?;

        match row {
            Some(r) => {
                let days = r.days_old.unwrap_or(365.0);
                let interactions = r.interaction_count.unwrap_or(0) as f64;
                if days < 1.0 && interactions > 50.0 {
                    Ok(0.8)
                } else if days < 3.0 && interactions > 200.0 {
                    Ok(0.6)
                } else if days < 7.0 && interactions > 500.0 {
                    Ok(0.4)
                } else {
                    Ok(0.0)
                }
            }
            None => Ok(0.0),
        }
    }

    /// Check if the device fingerprint is associated with banned or multiple accounts.
    async fn check_device_fingerprint(
        &self,
        pool: &PgPool,
        user_id: i32,
        fingerprint: &DeviceFingerprint,
    ) -> Result<f64, sqlx::Error> {
        // Store fingerprint
        let _ = sqlx::query(
            r#"INSERT INTO device_fingerprints (user_id, fingerprint_hash, features, first_seen, last_seen)
               VALUES ($1, $2, $3, NOW(), NOW())
               ON CONFLICT (user_id, fingerprint_hash) DO UPDATE SET last_seen = NOW()"#
        )
        .bind(user_id)
        .bind(&fingerprint.fingerprint_hash)
        .bind(serde_json::to_value(fingerprint).unwrap_or_default())
        .execute(pool)
        .await;

        // Check how many other users share this fingerprint
        let shared_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT user_id) FROM device_fingerprints WHERE fingerprint_hash = $1 AND user_id != $2"
        )
        .bind(&fingerprint.fingerprint_hash)
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        // > 3 users sharing same device = suspicious
        Ok(((shared_count as f64 - 1.0) / 2.0).clamp(0.0, 1.0))
    }

    /// Check for suspicious messaging patterns (spam-like behavior).
    async fn check_message_patterns(&self, pool: &PgPool, user_id: i32) -> Result<f64, sqlx::Error> {
        // Check messages sent in last hour
        let msg_count: i64 = sqlx::query_scalar(
            "SELECT COUNT(*) FROM messages WHERE sender_id = $1 AND created_at > NOW() - INTERVAL '1 hour'"
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        // Check unique recipients in last hour
        let unique_recipients: i64 = sqlx::query_scalar(
            "SELECT COUNT(DISTINCT receiver_id) FROM messages WHERE sender_id = $1 AND created_at > NOW() - INTERVAL '1 hour'"
        )
        .bind(user_id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        // High message count to many different recipients = spam
        if unique_recipients > 10 && msg_count > 50 {
            Ok(0.9)
        } else if unique_recipients > 5 && msg_count > 30 {
            Ok(0.5)
        } else {
            Ok(0.0)
        }
    }

    /// Log a trust safety event for audit.
    pub async fn log_event(
        &self,
        pool: &PgPool,
        user_id: i32,
        score: &TrustScore,
        trace_id: &str,
    ) {
        if let Err(e) = sqlx::query(
            r#"INSERT INTO trust_safety_events (user_id, risk_score, signals, action_taken, trace_id, created_at)
               VALUES ($1, $2, $3, $4, $5, NOW())"#
        )
        .bind(user_id)
        .bind(score.risk_score)
        .bind(serde_json::to_value(&score.signals).unwrap_or_default())
        .bind(serde_json::to_value(&score.action).unwrap_or_default().to_string())
        .bind(trace_id)
        .execute(pool)
        .await
        {
            warn!("Failed to log trust safety event: {e}");
        }
    }
}

impl Default for TrustSafetyService {
    fn default() -> Self {
        Self::new(0.85) // Auto-ban at 85% risk
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_device_fingerprint_hash() {
        let mut headers = axum::http::HeaderMap::new();
        headers.insert("User-Agent", "TestAgent/1.0".parse().unwrap());
        let fp = DeviceFingerprint::from_headers(&headers);
        assert!(!fp.fingerprint_hash.is_empty());
    }

    #[test]
    fn test_trust_action_thresholds() {
        let svc = TrustSafetyService::new(0.85);
        assert_eq!(svc.auto_ban_threshold, 0.85);
        assert!(svc.flag_threshold < svc.auto_ban_threshold);
        assert!(svc.throttle_threshold < svc.flag_threshold);
    }
}
