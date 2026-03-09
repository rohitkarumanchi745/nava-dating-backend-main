//! Engagement optimization: churn propensity, send-time optimization, and
//! notification variant bandit.
//!
//! - Churn model: logistic regression on engagement features to predict 7-day
//!   inactivity. High-risk users get boosted in discover to re-engage them.
//! - Send-time optimizer: per-user hourly activity histogram to find optimal
//!   push notification timing.
//! - Notification bandit: Thompson sampling over copy/offer variants to
//!   maximize notification open rates.

use std::collections::HashMap;
use rand::Rng;
use sqlx::PgPool;

// ─── Churn / Propensity Model ───────────────────────────────────────────────

/// Logistic regression churn predictor.
/// Features: days_since_last_active, swipe_rate_7d, match_rate_7d,
///           message_rate_7d, session_count_7d, profile_completeness.
#[derive(Debug, Clone)]
pub struct ChurnPredictor {
    /// Logistic regression weights (6 features + bias)
    weights: Vec<f64>,
    /// Threshold above which a user is considered at-risk
    pub churn_threshold: f64,
}

impl Default for ChurnPredictor {
    fn default() -> Self {
        // Sensible initial weights learned from typical dating app patterns:
        // Higher weight on days_inactive and low session_count = high churn risk.
        Self {
            weights: vec![
                0.8,   // days_since_last_active (strong positive → more churn)
                -0.5,  // swipe_rate_7d (negative → more swipes = less churn)
                -0.6,  // match_rate_7d (matches reduce churn)
                -0.4,  // message_rate_7d (messaging reduces churn)
                -0.7,  // session_count_7d (sessions reduce churn)
                -0.3,  // profile_completeness (complete profiles churn less)
                -1.0,  // bias
            ],
            churn_threshold: 0.6,
        }
    }
}

/// Churn prediction result.
#[derive(Debug, Clone)]
pub struct ChurnPrediction {
    /// Probability of churning in next 7 days (0.0-1.0)
    pub churn_probability: f64,
    /// Whether user is at risk (above threshold)
    pub at_risk: bool,
    /// Feature values used for prediction
    pub features: ChurnFeatures,
}

/// Features for churn prediction.
#[derive(Debug, Clone)]
pub struct ChurnFeatures {
    pub days_since_last_active: f64,
    pub swipe_rate_7d: f64,
    pub match_rate_7d: f64,
    pub message_rate_7d: f64,
    pub session_count_7d: f64,
    pub profile_completeness: f64,
}

impl ChurnPredictor {
    /// Sigmoid function.
    fn sigmoid(x: f64) -> f64 {
        1.0 / (1.0 + (-x).exp())
    }

    /// Predict churn probability from features.
    pub fn predict(&self, features: &ChurnFeatures) -> ChurnPrediction {
        let x = vec![
            (features.days_since_last_active / 30.0).min(1.0), // Normalize to 0-1
            features.swipe_rate_7d,
            features.match_rate_7d,
            features.message_rate_7d,
            features.session_count_7d,
            features.profile_completeness,
        ];

        let mut logit = 0.0;
        for (w, v) in self.weights.iter().zip(x.iter().chain(std::iter::once(&1.0))) {
            logit += w * v;
        }

        let prob = Self::sigmoid(logit);
        ChurnPrediction {
            churn_probability: prob,
            at_risk: prob >= self.churn_threshold,
            features: features.clone(),
        }
    }

    /// Extract churn features from the database for a user.
    pub async fn extract_features(pool: &PgPool, user_id: i32) -> ChurnFeatures {
        #[derive(sqlx::FromRow)]
        struct ActivityRow {
            days_inactive: Option<f64>,
            swipes_7d: Option<i64>,
            likes_7d: Option<i64>,
            matches_7d: Option<i64>,
            messages_7d: Option<i64>,
            sessions_7d: Option<i64>,
            profile_completeness: Option<f64>,
        }

        let row = sqlx::query_as::<_, ActivityRow>(
            r#"SELECT
                 EXTRACT(EPOCH FROM (NOW() - COALESCE(
                     (SELECT MAX(created_at) FROM interaction_events WHERE user_id = $1),
                     (SELECT created_at FROM users WHERE id = $1)
                 ))) / 86400.0 AS days_inactive,
                 (SELECT COUNT(*) FROM interaction_events
                  WHERE user_id = $1 AND event_type IN ('like', 'pass')
                    AND created_at > NOW() - INTERVAL '7 days') AS swipes_7d,
                 (SELECT COUNT(*) FROM interaction_events
                  WHERE user_id = $1 AND event_type = 'like'
                    AND created_at > NOW() - INTERVAL '7 days') AS likes_7d,
                 (SELECT COUNT(*) FROM matches
                  WHERE (user1_id = $1 OR user2_id = $1) AND is_mutual_match = TRUE
                    AND matched_at > NOW() - INTERVAL '7 days') AS matches_7d,
                 (SELECT COUNT(*) FROM messages
                  WHERE sender_id = $1
                    AND created_at > NOW() - INTERVAL '7 days') AS messages_7d,
                 (SELECT COUNT(DISTINCT DATE(created_at)) FROM interaction_events
                  WHERE user_id = $1
                    AND created_at > NOW() - INTERVAL '7 days') AS sessions_7d,
                 (SELECT
                    (CASE WHEN name IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN bio IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN profile_photo_url IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN looking_for IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN height_cm IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN profession_category IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN gender IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN interests IS NOT NULL THEN 1 ELSE 0 END +
                     CASE WHEN languages IS NOT NULL THEN 1 ELSE 0 END)::float / 9.0
                  FROM users WHERE id = $1) AS profile_completeness"#,
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .ok()
        .flatten();

        match row {
            Some(r) => {
                let swipes = r.swipes_7d.unwrap_or(0) as f64;
                let likes = r.likes_7d.unwrap_or(0) as f64;
                ChurnFeatures {
                    days_since_last_active: r.days_inactive.unwrap_or(30.0),
                    swipe_rate_7d: (swipes / 50.0).min(1.0),
                    match_rate_7d: if likes > 0.0 {
                        (r.matches_7d.unwrap_or(0) as f64 / likes).min(1.0)
                    } else {
                        0.0
                    },
                    message_rate_7d: (r.messages_7d.unwrap_or(0) as f64 / 20.0).min(1.0),
                    session_count_7d: (r.sessions_7d.unwrap_or(0) as f64 / 7.0).min(1.0),
                    profile_completeness: r.profile_completeness.unwrap_or(0.3),
                }
            }
            None => ChurnFeatures {
                days_since_last_active: 30.0,
                swipe_rate_7d: 0.0,
                match_rate_7d: 0.0,
                message_rate_7d: 0.0,
                session_count_7d: 0.0,
                profile_completeness: 0.3,
            },
        }
    }

    /// Online weight update using a single observation (SGD step).
    /// `y` = 1.0 if the user actually churned, 0.0 if retained.
    pub fn update(&mut self, features: &ChurnFeatures, y: f64, lr: f64) {
        let pred = self.predict(features);
        let error = pred.churn_probability - y;

        let x = vec![
            (features.days_since_last_active / 30.0).min(1.0),
            features.swipe_rate_7d,
            features.match_rate_7d,
            features.message_rate_7d,
            features.session_count_7d,
            features.profile_completeness,
            1.0, // bias
        ];

        for (w, xi) in self.weights.iter_mut().zip(x.iter()) {
            *w -= lr * error * xi;
        }
    }
}

// ─── Send-Time Optimizer ────────────────────────────────────────────────────

/// Per-user hourly activity histogram for optimal notification timing.
#[derive(Debug, Clone)]
pub struct SendTimeOptimizer {
    /// hour_counts\[user_id\]\[hour_of_day\] = interaction count
    user_histograms: HashMap<i32, [f64; 24]>,
    /// Global fallback histogram (population average)
    global_histogram: [f64; 24],
}

impl SendTimeOptimizer {
    pub fn new() -> Self {
        // Default: peak at 8pm-10pm, trough at 3am-6am
        let mut global = [1.0; 24];
        for h in 0..6 { global[h] = 0.2; }    // Late night / early morning
        for h in 6..9 { global[h] = 0.6; }    // Morning
        for h in 9..12 { global[h] = 0.8; }   // Late morning
        for h in 12..14 { global[h] = 1.0; }  // Lunch
        for h in 14..17 { global[h] = 0.7; }  // Afternoon
        for h in 17..20 { global[h] = 1.2; }  // Evening
        for h in 20..23 { global[h] = 1.5; }  // Prime time
        global[23] = 0.8;                       // Late night
        Self {
            user_histograms: HashMap::new(),
            global_histogram: global,
        }
    }

    /// Build user activity histograms from the database.
    pub async fn build(pool: &PgPool, days: i32) -> Self {
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

        let mut opt = Self::new();
        let mut global_counts = [0.0_f64; 24];

        for row in &rows {
            let h = row.hour.unwrap_or(0.0) as usize;
            if h >= 24 { continue; }
            let count = row.cnt.unwrap_or(0) as f64;

            let hist = opt.user_histograms
                .entry(row.user_id)
                .or_insert([0.0; 24]);
            hist[h] += count;
            global_counts[h] += count;
        }

        // Normalize global histogram
        let max_global = global_counts.iter().cloned().fold(1.0_f64, f64::max);
        for h in 0..24 {
            opt.global_histogram[h] = global_counts[h] / max_global;
        }

        opt
    }

    /// Find the best hour to send a notification to a user.
    /// Returns (best_hour, score) where score is the relative activity level.
    pub fn best_hour(&self, user_id: i32) -> (usize, f64) {
        let hist = self.user_histograms
            .get(&user_id)
            .map(|h| h.as_slice())
            .unwrap_or(&self.global_histogram);

        let mut best_h = 20; // Default prime time
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

    /// Get activity score for a specific hour for a user (0.0-1.0).
    pub fn activity_at_hour(&self, user_id: i32, hour: usize) -> f64 {
        if hour >= 24 { return 0.0; }
        let hist = self.user_histograms
            .get(&user_id)
            .map(|h| h.as_slice())
            .unwrap_or(&self.global_histogram);
        let max = hist.iter().cloned().fold(1.0_f64, f64::max);
        if max > 0.0 { hist[hour] / max } else { 0.5 }
    }
}

impl Default for SendTimeOptimizer {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Notification Variant Bandit ────────────────────────────────────────────

/// Thompson Sampling bandit for notification copy/offer variants.
/// Each variant (arm) tracks success/failure counts and samples from a
/// Beta distribution to balance exploration vs exploitation.
#[derive(Debug, Clone)]
pub struct NotificationBandit {
    variants: HashMap<String, BetaArm>,
}

/// Beta-distributed arm for Thompson Sampling.
#[derive(Debug, Clone)]
pub struct BetaArm {
    /// Success count (opens/clicks)
    pub alpha: f64,
    /// Failure count (ignores)
    pub beta: f64,
    /// Total sends
    pub sends: u64,
}

impl BetaArm {
    pub fn new() -> Self {
        Self {
            alpha: 1.0, // Uniform prior
            beta: 1.0,
            sends: 0,
        }
    }

    /// Sample from Beta(alpha, beta) using the Jinks/Kuiper method.
    pub fn sample(&self) -> f64 {
        let mut rng = rand::thread_rng();
        // Approximate Beta sampling via gamma ratio
        let x = gamma_sample(&mut rng, self.alpha);
        let y = gamma_sample(&mut rng, self.beta);
        if x + y == 0.0 { 0.5 } else { x / (x + y) }
    }

    /// Expected value (mean of Beta distribution).
    pub fn expected(&self) -> f64 {
        self.alpha / (self.alpha + self.beta)
    }
}

impl Default for BetaArm {
    fn default() -> Self {
        Self::new()
    }
}

/// Simple gamma variate sampling (Marsaglia & Tsang method for α ≥ 1,
/// with boost for α < 1).
fn gamma_sample(rng: &mut impl Rng, alpha: f64) -> f64 {
    if alpha < 1.0 {
        let u: f64 = rng.gen_range(0.0..1.0);
        return gamma_sample(rng, alpha + 1.0) * u.powf(1.0 / alpha);
    }
    let d = alpha - 1.0 / 3.0;
    let c = 1.0 / (9.0 * d).sqrt();
    loop {
        let x: f64 = rng.gen_range(-5.0..5.0); // Approximate normal
        let v = (1.0 + c * x).powi(3);
        if v <= 0.0 { continue; }
        let u: f64 = rng.gen_range(0.0..1.0);
        if u < 1.0 - 0.0331 * x.powi(4) {
            return d * v;
        }
        if u.ln() < 0.5 * x * x + d * (1.0 - v + v.ln()) {
            return d * v;
        }
    }
}

impl NotificationBandit {
    pub fn new() -> Self {
        Self {
            variants: HashMap::new(),
        }
    }

    /// Register a variant (arm) for the bandit.
    pub fn add_variant(&mut self, name: &str) {
        self.variants.entry(name.to_string()).or_insert_with(BetaArm::new);
    }

    /// Select the best variant using Thompson Sampling.
    /// Returns (variant_name, sampled_value).
    pub fn select(&self) -> Option<(String, f64)> {
        if self.variants.is_empty() {
            return None;
        }
        self.variants
            .iter()
            .map(|(name, arm)| (name.clone(), arm.sample()))
            .max_by(|a, b| a.1.partial_cmp(&b.1).unwrap_or(std::cmp::Ordering::Equal))
    }

    /// Record outcome: success (opened/clicked) or failure (ignored).
    pub fn record(&mut self, variant: &str, success: bool) {
        if let Some(arm) = self.variants.get_mut(variant) {
            if success {
                arm.alpha += 1.0;
            } else {
                arm.beta += 1.0;
            }
            arm.sends += 1;
        }
    }

    /// Get stats for all variants.
    pub fn stats(&self) -> serde_json::Value {
        let arms: Vec<serde_json::Value> = self
            .variants
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
}

impl Default for NotificationBandit {
    fn default() -> Self {
        Self::new()
    }
}

/// Combined engagement scorer that wraps churn + send-time + bandit.
#[derive(Debug, Clone)]
pub struct EngagementScorer {
    pub churn: ChurnPredictor,
    pub send_time: SendTimeOptimizer,
    pub notif_bandit: NotificationBandit,
}

impl EngagementScorer {
    pub fn new() -> Self {
        let mut bandit = NotificationBandit::new();
        // Pre-register common notification variants
        bandit.add_variant("new_match_standard");
        bandit.add_variant("new_match_enthusiastic");
        bandit.add_variant("new_match_minimal");
        bandit.add_variant("re_engage_standard");
        bandit.add_variant("re_engage_fomo");
        bandit.add_variant("re_engage_value_prop");

        Self {
            churn: ChurnPredictor::default(),
            send_time: SendTimeOptimizer::new(),
            notif_bandit: bandit,
        }
    }

    /// Build with data from the database.
    pub async fn build(pool: &PgPool) -> Self {
        let send_time = SendTimeOptimizer::build(pool, 30).await;
        let mut scorer = Self::new();
        scorer.send_time = send_time;
        scorer
    }

    /// Get a churn-adjusted ranking boost for a user.
    /// At-risk users get a small positive boost so they see better matches
    /// and are more likely to engage.
    pub fn churn_boost(&self, prediction: &ChurnPrediction) -> f64 {
        if prediction.at_risk {
            // Boost proportional to churn risk (max 0.15 boost)
            (prediction.churn_probability * 0.15).min(0.15)
        } else {
            0.0
        }
    }

    pub fn stats(&self) -> serde_json::Value {
        serde_json::json!({
            "churn_threshold": self.churn.churn_threshold,
            "notification_bandit": self.notif_bandit.stats(),
            "user_histograms_count": self.send_time.user_histograms.len(),
        })
    }
}

impl Default for EngagementScorer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_churn_prediction() {
        let predictor = ChurnPredictor::default();

        // Active user: low churn
        let active = ChurnFeatures {
            days_since_last_active: 1.0,
            swipe_rate_7d: 0.8,
            match_rate_7d: 0.3,
            message_rate_7d: 0.5,
            session_count_7d: 0.9,
            profile_completeness: 0.8,
        };
        let pred = predictor.predict(&active);
        assert!(pred.churn_probability < 0.4, "Active user should have low churn: {}", pred.churn_probability);

        // Inactive user: high churn
        let inactive = ChurnFeatures {
            days_since_last_active: 25.0,
            swipe_rate_7d: 0.0,
            match_rate_7d: 0.0,
            message_rate_7d: 0.0,
            session_count_7d: 0.0,
            profile_completeness: 0.2,
        };
        let pred = predictor.predict(&inactive);
        assert!(pred.churn_probability > 0.35, "Inactive user should have elevated churn: {}", pred.churn_probability);
        // Inactive user should score higher than active user
        let active_pred = predictor.predict(&active);
        assert!(pred.churn_probability > active_pred.churn_probability,
            "Inactive ({:.3}) should churn more than active ({:.3})",
            pred.churn_probability, active_pred.churn_probability);
    }

    #[test]
    fn test_send_time_optimizer() {
        let opt = SendTimeOptimizer::new();
        let (best_h, score) = opt.best_hour(999); // Unknown user → global histogram
        assert!(best_h >= 20 && best_h <= 22, "Best hour should be evening: {}", best_h);
        assert!(score > 0.5);
    }

    #[test]
    fn test_notification_bandit() {
        let mut bandit = NotificationBandit::new();
        bandit.add_variant("A");
        bandit.add_variant("B");

        // Make A clearly better
        for _ in 0..20 { bandit.record("A", true); }
        for _ in 0..5 { bandit.record("A", false); }
        for _ in 0..5 { bandit.record("B", true); }
        for _ in 0..20 { bandit.record("B", false); }

        // After enough data, A should be selected most of the time
        let mut a_wins = 0;
        for _ in 0..100 {
            if let Some((name, _)) = bandit.select() {
                if name == "A" { a_wins += 1; }
            }
        }
        assert!(a_wins > 70, "A should win most selections: {}/100", a_wins);
    }

    #[test]
    fn test_churn_boost() {
        let scorer = EngagementScorer::new();
        let at_risk = ChurnPrediction {
            churn_probability: 0.8,
            at_risk: true,
            features: ChurnFeatures {
                days_since_last_active: 20.0,
                swipe_rate_7d: 0.0,
                match_rate_7d: 0.0,
                message_rate_7d: 0.0,
                session_count_7d: 0.0,
                profile_completeness: 0.3,
            },
        };
        let boost = scorer.churn_boost(&at_risk);
        assert!(boost > 0.05 && boost <= 0.15, "Boost should be moderate: {}", boost);
    }
}
