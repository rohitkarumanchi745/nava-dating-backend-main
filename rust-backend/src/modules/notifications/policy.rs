//! Notification policy layer: per-user caps, quiet hours, variant selection,
//! send-time gating, and outcome logging.
//!
//! Sits between Kafka event handlers and the actual send calls. Every
//! notification passes through `NotificationPolicy::gate()` before delivery.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

use chrono::{Timelike, Utc};
use rand::Rng;
use sqlx::PgPool;
use tracing::{info, warn};

// ─── Policy Metrics ─────────────────────────────────────────────────────────

/// Counters for monitoring policy gate decisions.
/// Exported via the /metrics endpoint for Prometheus scraping.
pub struct PolicyMetrics {
    /// Notifications that passed the gate and were sent.
    pub notif_sent_total: AtomicU64,
    /// Notifications blocked by daily cap.
    pub notif_blocked_cap: AtomicU64,
    /// Notifications blocked by cooldown.
    pub notif_blocked_cooldown: AtomicU64,
    /// Notifications blocked by quiet hours (deferred).
    pub notif_deferred_quiet: AtomicU64,
    /// Notifications deferred due to low send-time activity.
    pub notif_deferred_timing: AtomicU64,
    /// Notifications suppressed because user opted out.
    pub notif_blocked_optout: AtomicU64,
    /// Per-variant send counts (variant_id → count), updated atomically.
    /// Use bandit_stats() for detailed per-variant breakdown.
    pub notif_engagement_success: AtomicU64,
    pub notif_engagement_failure: AtomicU64,
}

impl PolicyMetrics {
    pub fn new() -> Self {
        Self {
            notif_sent_total: AtomicU64::new(0),
            notif_blocked_cap: AtomicU64::new(0),
            notif_blocked_cooldown: AtomicU64::new(0),
            notif_deferred_quiet: AtomicU64::new(0),
            notif_deferred_timing: AtomicU64::new(0),
            notif_blocked_optout: AtomicU64::new(0),
            notif_engagement_success: AtomicU64::new(0),
            notif_engagement_failure: AtomicU64::new(0),
        }
    }
}

// ─── Configuration ──────────────────────────────────────────────────────────

/// Policy configuration (tunable at runtime via env or config reload).
#[derive(Debug, Clone)]
pub struct PolicyConfig {
    /// Max notifications per user per 24h window.
    pub daily_cap: u32,
    /// Quiet hours start (inclusive, 0-23 in user's local time).
    pub quiet_start_hour: u8,
    /// Quiet hours end (exclusive, 0-23 in user's local time).
    pub quiet_end_hour: u8,
    /// Minimum seconds between notifications to the same user.
    pub cooldown_secs: u32,
    /// Whether the bandit is in shadow mode (log variant but send default).
    pub bandit_shadow_mode: bool,
}

impl Default for PolicyConfig {
    fn default() -> Self {
        Self {
            daily_cap: 12,
            quiet_start_hour: 22,
            quiet_end_hour: 7,
            cooldown_secs: 300, // 5 minutes
            bandit_shadow_mode: false,
        }
    }
}

// ─── Variant Definitions ────────────────────────────────────────────────────

/// Notification category that supports variant selection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NotifCategory {
    NewMatch,
    ReEngage,
    Like,
    Message,
}

impl NotifCategory {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::NewMatch => "new_match",
            Self::ReEngage => "re_engage",
            Self::Like => "like",
            Self::Message => "message",
        }
    }
}

/// A notification variant with title/body copy.
#[derive(Debug, Clone)]
pub struct NotifVariant {
    pub variant_id: String,
    pub title: String,
    pub body: String,
}

/// Returns the pre-registered variants for a category.
pub fn variants_for(category: NotifCategory) -> Vec<NotifVariant> {
    match category {
        NotifCategory::NewMatch => vec![
            NotifVariant {
                variant_id: "new_match_standard".into(),
                title: "It's a Match!".into(),
                body: "You have a new match! Start a conversation now.".into(),
            },
            NotifVariant {
                variant_id: "new_match_enthusiastic".into(),
                title: "You matched! 🎉".into(),
                body: "Someone special is waiting to hear from you.".into(),
            },
            NotifVariant {
                variant_id: "new_match_minimal".into(),
                title: "New match".into(),
                body: "Say hello to your latest match.".into(),
            },
        ],
        NotifCategory::ReEngage => vec![
            NotifVariant {
                variant_id: "re_engage_standard".into(),
                title: "We miss you!".into(),
                body: "New people are waiting to meet you.".into(),
            },
            NotifVariant {
                variant_id: "re_engage_fomo".into(),
                title: "Don't miss out".into(),
                body: "Several people liked your profile recently.".into(),
            },
            NotifVariant {
                variant_id: "re_engage_value_prop".into(),
                title: "Your matches are waiting".into(),
                body: "Open Nava and see who's interested.".into(),
            },
        ],
        NotifCategory::Like => vec![
            NotifVariant {
                variant_id: "like_standard".into(),
                title: "Someone Likes You!".into(),
                body: "Check out who likes you.".into(),
            },
        ],
        NotifCategory::Message => vec![
            NotifVariant {
                variant_id: "message_standard".into(),
                title: "New Message".into(),
                body: "You have a new message.".into(),
            },
        ],
    }
}

// ─── Bandit (inline, avoids cross-crate dependency) ─────────────────────────

/// Lightweight Beta arm for Thompson Sampling (mirrors engagement.rs).
#[derive(Debug, Clone)]
struct BetaArm {
    alpha: f64,
    beta: f64,
    sends: u64,
}

impl BetaArm {
    fn new() -> Self {
        Self { alpha: 1.0, beta: 1.0, sends: 0 }
    }

    fn sample(&self) -> f64 {
        let mut rng = rand::thread_rng();
        let x = gamma_sample(&mut rng, self.alpha);
        let y = gamma_sample(&mut rng, self.beta);
        if x + y == 0.0 { 0.5 } else { x / (x + y) }
    }

    fn expected(&self) -> f64 {
        self.alpha / (self.alpha + self.beta)
    }
}

/// Marsaglia & Tsang gamma variate for Beta sampling.
fn gamma_sample(rng: &mut impl Rng, alpha: f64) -> f64 {
    if alpha < 1.0 {
        let u: f64 = rng.gen_range(0.0..1.0);
        return gamma_sample(rng, alpha + 1.0) * u.powf(1.0 / alpha);
    }
    let d = alpha - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let x: f64 = rng.gen_range(-5.0..5.0);
        let v = (1.0 + c * x).powi(3);
        if v <= 0.0 { continue; }
        let u: f64 = rng.gen_range(0.0..1.0);
        if u < 1.0 - 0.0331 * x.powi(4) { return d * v; }
        if u.ln() < 0.5 * x * x + d * (1.0 - v + v.ln()) { return d * v; }
    }
}

// ─── Send-Time (inline) ─────────────────────────────────────────────────────

/// Per-user hourly activity histogram for optimal send timing.
struct SendTimeData {
    user_histograms: HashMap<i32, [f64; 24]>,
    global_histogram: [f64; 24],
}

impl SendTimeData {
    fn new() -> Self {
        let mut global = [1.0; 24];
        for h in 0..6 { global[h] = 0.2; }
        for h in 6..9 { global[h] = 0.6; }
        for h in 9..12 { global[h] = 0.8; }
        for h in 12..14 { global[h] = 1.0; }
        for h in 14..17 { global[h] = 0.7; }
        for h in 17..20 { global[h] = 1.2; }
        for h in 20..23 { global[h] = 1.5; }
        global[23] = 0.8;
        Self { user_histograms: HashMap::new(), global_histogram: global }
    }

    async fn build(pool: &PgPool, days: i32) -> Self {
        #[derive(sqlx::FromRow)]
        struct HourRow {
            user_id: i32,
            hour: Option<f64>,
            cnt: Option<i64>,
        }

        let rows = sqlx::query_as::<_, HourRow>(
            r#"SELECT user_id,
                      EXTRACT(HOUR FROM created_at) AS hour,
                      COUNT(*) AS cnt
               FROM interaction_events
               WHERE created_at > NOW() - ($1 || ' days')::interval
               GROUP BY user_id, hour
               ORDER BY user_id, hour"#,
        )
        .bind(days)
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        let mut data = Self::new();
        let mut global_counts = [0.0_f64; 24];

        for row in &rows {
            let h = row.hour.unwrap_or(0.0) as usize;
            if h >= 24 { continue; }
            let count = row.cnt.unwrap_or(0) as f64;
            let hist = data.user_histograms.entry(row.user_id).or_insert([0.0; 24]);
            hist[h] += count;
            global_counts[h] += count;
        }

        let max_global = global_counts.iter().cloned().fold(1.0_f64, f64::max);
        for h in 0..24 {
            data.global_histogram[h] = global_counts[h] / max_global;
        }
        data
    }

    fn activity_at_hour(&self, user_id: i32, hour: usize) -> f64 {
        if hour >= 24 { return 0.0; }
        let hist = self.user_histograms
            .get(&user_id)
            .map(|h| h.as_slice())
            .unwrap_or(&self.global_histogram);
        let max = hist.iter().cloned().fold(1.0_f64, f64::max);
        if max > 0.0 { hist[hour] / max } else { 0.5 }
    }

    fn best_hour(&self, user_id: i32) -> (usize, f64) {
        let hist = self.user_histograms
            .get(&user_id)
            .map(|h| h.as_slice())
            .unwrap_or(&self.global_histogram);
        let mut best_h = 20;
        let mut best_score = 0.0;
        for (h, &count) in hist.iter().enumerate() {
            if count > best_score {
                best_score = count;
                best_h = h;
            }
        }
        let max = hist.iter().cloned().fold(1.0_f64, f64::max);
        (best_h, if max > 0.0 { best_score / max } else { 0.5 })
    }
}

// ─── Gate Decision ──────────────────────────────────────────────────────────

/// Result of the policy gate check.
#[derive(Debug, Clone)]
pub enum GateDecision {
    /// Send now with this variant.
    Send {
        variant: NotifVariant,
        /// True if the variant was chosen by the bandit (vs default/shadow).
        bandit_selected: bool,
    },
    /// Defer until the user's optimal send hour.
    Defer {
        optimal_hour: usize,
        reason: String,
    },
    /// Suppress entirely.
    Suppress {
        reason: String,
    },
}

// ─── NotificationPolicy ────────────────────────────────────────────────────

/// Policy layer that gates every notification through caps, quiet hours,
/// send-time optimization, and variant selection.
pub struct NotificationPolicy {
    pool: PgPool,
    config: PolicyConfig,
    bandits: Mutex<HashMap<String, BetaArm>>,
    send_time: Mutex<SendTimeData>,
    pub metrics: PolicyMetrics,
}

impl NotificationPolicy {
    /// Create a new policy with default config and build send-time data.
    pub async fn new(pool: PgPool) -> Self {
        let send_time = SendTimeData::build(&pool, 30).await;

        let mut bandits = HashMap::new();
        // Register all variant arms
        for cat in [NotifCategory::NewMatch, NotifCategory::ReEngage] {
            for v in variants_for(cat) {
                bandits.insert(v.variant_id, BetaArm::new());
            }
        }

        Self {
            pool,
            config: PolicyConfig::default(),
            bandits: Mutex::new(bandits),
            send_time: Mutex::new(send_time),
            metrics: PolicyMetrics::new(),
        }
    }

    /// Create with a custom config.
    pub async fn with_config(pool: PgPool, config: PolicyConfig) -> Self {
        let mut policy = Self::new(pool).await;
        policy.config = config;
        policy
    }

    /// Rebuild send-time histograms from fresh data (call periodically).
    pub async fn refresh_send_time(&self) {
        let fresh = SendTimeData::build(&self.pool, 30).await;
        if let Ok(mut st) = self.send_time.lock() {
            *st = fresh;
        }
    }

    /// Main gate: check whether a notification should be sent right now.
    ///
    /// Returns `GateDecision::Send` with the chosen variant,
    /// `Defer` if the timing is wrong, or `Suppress` if capped/opted-out.
    pub async fn gate(
        &self,
        user_id: i32,
        category: NotifCategory,
        utc_offset_hours: i32,
    ) -> GateDecision {
        // 1. Opt-out check
        if self.is_opted_out(user_id, category).await {
            self.metrics.notif_blocked_optout.fetch_add(1, Ordering::Relaxed);
            return GateDecision::Suppress {
                reason: "user opted out".into(),
            };
        }

        // 2. Daily cap check
        if self.daily_count(user_id).await >= self.config.daily_cap {
            self.metrics.notif_blocked_cap.fetch_add(1, Ordering::Relaxed);
            return GateDecision::Suppress {
                reason: format!("daily cap ({}) reached", self.config.daily_cap),
            };
        }

        // 3. Cooldown check
        if self.within_cooldown(user_id).await {
            self.metrics.notif_blocked_cooldown.fetch_add(1, Ordering::Relaxed);
            return GateDecision::Suppress {
                reason: format!("cooldown ({}s) active", self.config.cooldown_secs),
            };
        }

        // 4. Quiet hours check
        let local_hour = ((Utc::now().hour() as i32 + utc_offset_hours).rem_euclid(24)) as u8;
        if self.is_quiet_hour(local_hour) {
            self.metrics.notif_deferred_quiet.fetch_add(1, Ordering::Relaxed);
            let (optimal_hour, _) = self.send_time.lock()
                .map(|st| st.best_hour(user_id))
                .unwrap_or((20, 0.5));
            return GateDecision::Defer {
                optimal_hour,
                reason: format!("quiet hours ({:02}:00-{:02}:00 local)",
                    self.config.quiet_start_hour, self.config.quiet_end_hour),
            };
        }

        // 5. Send-time gating: if current hour is low-activity, defer
        let current_activity = self.send_time.lock()
            .map(|st| st.activity_at_hour(user_id, local_hour as usize))
            .unwrap_or(0.5);

        if current_activity < 0.3 {
            self.metrics.notif_deferred_timing.fetch_add(1, Ordering::Relaxed);
            let (optimal_hour, _) = self.send_time.lock()
                .map(|st| st.best_hour(user_id))
                .unwrap_or((20, 0.5));
            return GateDecision::Defer {
                optimal_hour,
                reason: format!("low activity at hour {} (score {:.2})", local_hour, current_activity),
            };
        }

        // 6. Variant selection
        let variants = variants_for(category);
        let variant = if variants.len() > 1 {
            self.select_variant(&variants)
        } else {
            (variants.into_iter().next().unwrap(), false)
        };

        self.metrics.notif_sent_total.fetch_add(1, Ordering::Relaxed);

        GateDecision::Send {
            variant: variant.0,
            bandit_selected: variant.1,
        }
    }

    /// Select a variant using Thompson Sampling.
    /// In shadow mode, picks via bandit but returns the default (index 0).
    fn select_variant(&self, variants: &[NotifVariant]) -> (NotifVariant, bool) {
        let bandits = match self.bandits.lock() {
            Ok(b) => b,
            Err(_) => return (variants[0].clone(), false),
        };

        let mut best_idx = 0;
        let mut best_sample = f64::NEG_INFINITY;

        for (i, v) in variants.iter().enumerate() {
            if let Some(arm) = bandits.get(&v.variant_id) {
                let sample = arm.sample();
                if sample > best_sample {
                    best_sample = sample;
                    best_idx = i;
                }
            }
        }

        if self.config.bandit_shadow_mode {
            // Log the bandit choice but send the default
            info!(
                bandit_choice = %variants[best_idx].variant_id,
                actual_send = %variants[0].variant_id,
                "shadow mode: bandit would have chosen different variant"
            );
            (variants[0].clone(), false)
        } else {
            (variants[best_idx].clone(), true)
        }
    }

    /// Record notification outcome for bandit learning.
    pub fn record_outcome(&self, variant_id: &str, success: bool) {
        if success {
            self.metrics.notif_engagement_success.fetch_add(1, Ordering::Relaxed);
        } else {
            self.metrics.notif_engagement_failure.fetch_add(1, Ordering::Relaxed);
        }
        if let Ok(mut bandits) = self.bandits.lock() {
            if let Some(arm) = bandits.get_mut(variant_id) {
                if success {
                    arm.alpha += 1.0;
                } else {
                    arm.beta += 1.0;
                }
                arm.sends += 1;
            }
        }
    }

    /// Log a sent notification + variant for offline evaluation.
    pub async fn log_outcome(
        &self,
        user_id: i32,
        category: &str,
        variant_id: &str,
        bandit_selected: bool,
        sent_hour: u8,
    ) {
        let _ = sqlx::query(
            r#"INSERT INTO notification_outcomes
               (user_id, category, variant_id, bandit_selected, sent_at_hour, sent_at)
               VALUES ($1, $2, $3, $4, $5, NOW())"#,
        )
        .bind(user_id)
        .bind(category)
        .bind(variant_id)
        .bind(bandit_selected)
        .bind(sent_hour as i16)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            warn!(error = %e, "Failed to log notification outcome");
        });
    }

    /// Record that a user opened/engaged with a notification (called from
    /// analytics pipeline or push-receipt webhook).
    pub async fn record_engagement(
        &self,
        user_id: i32,
        variant_id: &str,
        engaged: bool,
    ) {
        // Update bandit arm
        self.record_outcome(variant_id, engaged);

        // Update outcome row
        let _ = sqlx::query(
            r#"UPDATE notification_outcomes
               SET engaged = $3, engaged_at = CASE WHEN $3 THEN NOW() ELSE NULL END
               WHERE user_id = $1 AND variant_id = $2
                 AND engaged IS NULL
               ORDER BY sent_at DESC LIMIT 1"#,
        )
        .bind(user_id)
        .bind(variant_id)
        .bind(engaged)
        .execute(&self.pool)
        .await
        .map_err(|e| {
            warn!(error = %e, "Failed to record engagement");
        });
    }

    /// Get bandit stats for monitoring.
    pub fn bandit_stats(&self) -> serde_json::Value {
        let bandits = match self.bandits.lock() {
            Ok(b) => b,
            Err(_) => return serde_json::json!({}),
        };

        let arms: Vec<serde_json::Value> = bandits
            .iter()
            .map(|(name, arm)| {
                serde_json::json!({
                    "variant": name,
                    "sends": arm.sends,
                    "alpha": arm.alpha,
                    "beta": arm.beta,
                    "expected_rate": arm.expected(),
                })
            })
            .collect();

        serde_json::json!({ "variants": arms })
    }

    /// Export all policy metrics as Prometheus-compatible text.
    pub fn metrics_prometheus(&self) -> String {
        let m = &self.metrics;
        format!(
            "# HELP notif_sent_total Notifications sent after passing gate\n\
             # TYPE notif_sent_total counter\n\
             notif_sent_total {}\n\
             # HELP notif_blocked_cap Notifications blocked by daily cap\n\
             # TYPE notif_blocked_cap counter\n\
             notif_blocked_cap {}\n\
             # HELP notif_blocked_cooldown Notifications blocked by cooldown\n\
             # TYPE notif_blocked_cooldown counter\n\
             notif_blocked_cooldown {}\n\
             # HELP notif_blocked_optout Notifications blocked by user opt-out\n\
             # TYPE notif_blocked_optout counter\n\
             notif_blocked_optout {}\n\
             # HELP notif_deferred_quiet Notifications deferred by quiet hours\n\
             # TYPE notif_deferred_quiet counter\n\
             notif_deferred_quiet {}\n\
             # HELP notif_deferred_timing Notifications deferred by low activity timing\n\
             # TYPE notif_deferred_timing counter\n\
             notif_deferred_timing {}\n\
             # HELP notif_engagement_success Notification opens/clicks\n\
             # TYPE notif_engagement_success counter\n\
             notif_engagement_success {}\n\
             # HELP notif_engagement_failure Notification ignores\n\
             # TYPE notif_engagement_failure counter\n\
             notif_engagement_failure {}\n",
            m.notif_sent_total.load(Ordering::Relaxed),
            m.notif_blocked_cap.load(Ordering::Relaxed),
            m.notif_blocked_cooldown.load(Ordering::Relaxed),
            m.notif_blocked_optout.load(Ordering::Relaxed),
            m.notif_deferred_quiet.load(Ordering::Relaxed),
            m.notif_deferred_timing.load(Ordering::Relaxed),
            m.notif_engagement_success.load(Ordering::Relaxed),
            m.notif_engagement_failure.load(Ordering::Relaxed),
        )
    }

    // ─── Private helpers ────────────────────────────────────────────────────

    fn is_quiet_hour(&self, local_hour: u8) -> bool {
        if self.config.quiet_start_hour > self.config.quiet_end_hour {
            // Wraps midnight: e.g. 22:00 → 07:00
            local_hour >= self.config.quiet_start_hour || local_hour < self.config.quiet_end_hour
        } else {
            local_hour >= self.config.quiet_start_hour && local_hour < self.config.quiet_end_hour
        }
    }

    async fn is_opted_out(&self, user_id: i32, category: NotifCategory) -> bool {
        let opted_out: Option<bool> = sqlx::query_scalar(
            r#"SELECT EXISTS(
                 SELECT 1 FROM notification_preferences
                 WHERE user_id = $1
                   AND (category = $2 OR category = 'all')
                   AND opted_out = TRUE
               )"#,
        )
        .bind(user_id)
        .bind(category.as_str())
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        opted_out.unwrap_or(false)
    }

    async fn daily_count(&self, user_id: i32) -> u32 {
        let count: Option<i64> = sqlx::query_scalar(
            r#"SELECT COUNT(*) FROM notification_outcomes
               WHERE user_id = $1 AND sent_at > NOW() - INTERVAL '24 hours'"#,
        )
        .bind(user_id)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        count.unwrap_or(0) as u32
    }

    async fn within_cooldown(&self, user_id: i32) -> bool {
        let exists: Option<bool> = sqlx::query_scalar(
            r#"SELECT EXISTS(
                 SELECT 1 FROM notification_outcomes
                 WHERE user_id = $1
                   AND sent_at > NOW() - ($2 || ' seconds')::interval
               )"#,
        )
        .bind(user_id)
        .bind(self.config.cooldown_secs as i32)
        .fetch_optional(&self.pool)
        .await
        .ok()
        .flatten();

        exists.unwrap_or(false)
    }
}

/// Ensure the notification_outcomes and notification_preferences tables exist.
pub async fn ensure_policy_tables(pool: &PgPool) {
    let _ = sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS notification_outcomes (
             id BIGSERIAL PRIMARY KEY,
             user_id INTEGER NOT NULL,
             category VARCHAR(50) NOT NULL,
             variant_id VARCHAR(100) NOT NULL,
             bandit_selected BOOLEAN NOT NULL DEFAULT FALSE,
             sent_at_hour SMALLINT NOT NULL,
             sent_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
             engaged BOOLEAN,
             engaged_at TIMESTAMPTZ
           )"#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        r#"CREATE INDEX IF NOT EXISTS idx_notif_outcomes_user_time
           ON notification_outcomes (user_id, sent_at DESC)"#,
    )
    .execute(pool)
    .await;

    let _ = sqlx::query(
        r#"CREATE TABLE IF NOT EXISTS notification_preferences (
             id SERIAL PRIMARY KEY,
             user_id INTEGER NOT NULL,
             category VARCHAR(50) NOT NULL,
             opted_out BOOLEAN NOT NULL DEFAULT FALSE,
             updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
             UNIQUE(user_id, category)
           )"#,
    )
    .execute(pool)
    .await;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_quiet_hours_wrap_midnight() {
        let config = PolicyConfig::default(); // 22-07
        let policy_config = &config;

        // Helper to check quiet hour with the config
        let is_quiet = |h: u8| -> bool {
            if policy_config.quiet_start_hour > policy_config.quiet_end_hour {
                h >= policy_config.quiet_start_hour || h < policy_config.quiet_end_hour
            } else {
                h >= policy_config.quiet_start_hour && h < policy_config.quiet_end_hour
            }
        };

        assert!(is_quiet(23)); // 11pm → quiet
        assert!(is_quiet(0));  // midnight → quiet
        assert!(is_quiet(3));  // 3am → quiet
        assert!(is_quiet(6));  // 6am → quiet
        assert!(!is_quiet(7)); // 7am → not quiet
        assert!(!is_quiet(12)); // noon → not quiet
        assert!(!is_quiet(21)); // 9pm → not quiet
        assert!(is_quiet(22)); // 10pm → quiet
    }

    #[test]
    fn test_variant_selection_returns_valid() {
        let variants = variants_for(NotifCategory::NewMatch);
        assert_eq!(variants.len(), 3);
        assert_eq!(variants[0].variant_id, "new_match_standard");
        assert_eq!(variants[1].variant_id, "new_match_enthusiastic");
        assert_eq!(variants[2].variant_id, "new_match_minimal");
    }

    #[test]
    fn test_re_engage_variants() {
        let variants = variants_for(NotifCategory::ReEngage);
        assert_eq!(variants.len(), 3);
        assert!(variants.iter().all(|v| v.variant_id.starts_with("re_engage")));
    }

    #[test]
    fn test_beta_arm_sampling() {
        let mut arm = BetaArm::new();
        // After many successes, expected rate should be high
        for _ in 0..100 {
            arm.alpha += 1.0;
            arm.sends += 1;
        }
        assert!(arm.expected() > 0.9);

        // Sampling should also be high
        let samples: Vec<f64> = (0..50).map(|_| arm.sample()).collect();
        let avg: f64 = samples.iter().sum::<f64>() / samples.len() as f64;
        assert!(avg > 0.8, "Average sample should be high: {}", avg);
    }

    #[test]
    fn test_policy_config_defaults() {
        let config = PolicyConfig::default();
        assert_eq!(config.daily_cap, 12);
        assert_eq!(config.quiet_start_hour, 22);
        assert_eq!(config.quiet_end_hour, 7);
        assert_eq!(config.cooldown_secs, 300);
        assert!(!config.bandit_shadow_mode);
    }
}
