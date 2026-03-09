use std::collections::HashMap;

use serde_json::Value;

use super::features::USER_FEATURE_DIM;
use super::math::laplace_noise;

/// Dimension of the on-device personalization head.
/// Small enough to train on-device (32 weights + 1 bias = 33 params).
pub const PERSONALIZATION_HEAD_DIM: usize = 33;

/// Number of affinity bias features for cold-start (interests, languages, intent).
pub const COLD_START_DIM: usize = 5;

/// Number of features for the on-device notification click predictor.
pub const NOTIF_PREDICTOR_DIM: usize = 6;

/// FedAvg aggregation with differential privacy.
/// Performs actual weighted averaging of client model weights.
/// The model operates on 28-dim state vectors (14 user features + 14 candidate features)
/// covering profile attributes, swipe behavior, and engagement signals.

pub struct FederatedCoordinator {
    /// Global learning rate for aggregation
    pub global_lr: f64,
    /// Minimum clients required to aggregate
    pub min_clients: usize,
    /// Differential privacy noise scale
    pub dp_noise_scale: f64,
    /// Whether DP is enabled
    pub dp_enabled: bool,
    /// Global personalization head weights (FedAvg'd across devices).
    /// Devices fine-tune a copy locally then upload deltas.
    pub global_head: PersonalizationHead,
    /// Aggregated cold-start affinity biases (per-cohort, keyed by intent bucket).
    pub cold_start_biases: HashMap<String, ColdStartBias>,
    /// Global notification click predictor (FedAvg'd from device priors).
    pub notif_predictor: NotifClickPredictor,
    /// Number of aggregation rounds completed
    pub rounds_completed: u64,
}

impl FederatedCoordinator {
    pub fn new() -> Self {
        Self {
            global_lr: 0.1,
            min_clients: 2,
            dp_noise_scale: 0.1,
            dp_enabled: true,
            global_head: PersonalizationHead::new(),
            cold_start_biases: HashMap::new(),
            notif_predictor: NotifClickPredictor::new(),
            rounds_completed: 0,
        }
    }

    /// Perform FedAvg aggregation on client weight updates.
    ///
    /// Each client provides `local_weights` as a JSON array of f64,
    /// weighted by `num_samples`.
    ///
    /// Returns aggregated weights as JSON + stats.
    pub fn aggregate(
        &self,
        client_updates: &[(Value, i32, f64, Option<f64>)], // (local_weights, num_samples, local_loss, local_accuracy)
    ) -> Result<AggregationResult, String> {
        if client_updates.len() < self.min_clients {
            return Err(format!(
                "Not enough clients: {} / {} required",
                client_updates.len(),
                self.min_clients
            ));
        }

        // Parse all client weights
        let mut parsed_weights: Vec<(Vec<f64>, f64)> = Vec::new(); // (weights, weight_factor)
        let total_samples: i32 = client_updates.iter().map(|(_, n, _, _)| *n).sum();

        if total_samples == 0 {
            return Err("Total samples is zero".into());
        }

        for (weights_json, num_samples, _, _) in client_updates {
            let weights: Vec<f64> = serde_json::from_value(weights_json.clone())
                .map_err(|e| format!("Failed to parse client weights: {e}"))?;
            let weight_factor = *num_samples as f64 / total_samples as f64;
            parsed_weights.push((weights, weight_factor));
        }

        // All clients must have same weight dimension
        let dim = parsed_weights[0].0.len();
        for (w, _) in &parsed_weights {
            if w.len() != dim {
                return Err(format!(
                    "Weight dimension mismatch: expected {}, got {}",
                    dim,
                    w.len()
                ));
            }
        }

        // Weighted average
        let mut aggregated = vec![0.0; dim];
        for (weights, factor) in &parsed_weights {
            for i in 0..dim {
                aggregated[i] += weights[i] * factor;
            }
        }

        // Apply differential privacy noise
        if self.dp_enabled {
            for val in aggregated.iter_mut() {
                *val += laplace_noise(self.dp_noise_scale);
            }
        }

        // Compute average loss and accuracy
        let avg_loss: f64 = client_updates
            .iter()
            .map(|(_, n, loss, _)| loss * *n as f64)
            .sum::<f64>()
            / total_samples as f64;

        let avg_accuracy: f64 = client_updates
            .iter()
            .filter_map(|(_, n, _, acc)| acc.map(|a| a * *n as f64))
            .sum::<f64>()
            / total_samples as f64;

        let aggregated_json = serde_json::to_value(&aggregated)
            .map_err(|e| format!("Failed to serialize aggregated weights: {e}"))?;

        Ok(AggregationResult {
            aggregated_weights: aggregated_json,
            avg_loss,
            avg_accuracy,
            num_clients: client_updates.len(),
            total_samples,
            dp_applied: self.dp_enabled,
        })
    }

    // ─── Personalization Head FedAvg ────────────────────────────────────────

    /// Receive device-local personalization head deltas and FedAvg them
    /// into the global head. Clients send weight deltas (not absolute weights)
    /// so we apply `global += lr * avg(deltas)` with DP noise.
    pub fn aggregate_head_deltas(
        &mut self,
        deltas: &[(Vec<f64>, i32)], // (delta_weights, num_local_swipes)
    ) -> Result<(), String> {
        if deltas.len() < self.min_clients {
            return Err(format!(
                "Not enough head deltas: {} / {} required",
                deltas.len(), self.min_clients
            ));
        }

        let total_samples: i32 = deltas.iter().map(|(_, n)| *n).sum();
        if total_samples == 0 {
            return Err("Total samples is zero".into());
        }

        // Validate dimensions
        for (d, _) in deltas {
            if d.len() != PERSONALIZATION_HEAD_DIM {
                return Err(format!(
                    "Head delta dim mismatch: expected {}, got {}",
                    PERSONALIZATION_HEAD_DIM, d.len()
                ));
            }
        }

        // Weighted average of deltas
        let mut avg_delta = vec![0.0; PERSONALIZATION_HEAD_DIM];
        for (delta, n) in deltas {
            let w = *n as f64 / total_samples as f64;
            for i in 0..PERSONALIZATION_HEAD_DIM {
                avg_delta[i] += delta[i] * w;
            }
        }

        // Apply DP noise to the aggregated delta
        if self.dp_enabled {
            for val in avg_delta.iter_mut() {
                *val += laplace_noise(self.dp_noise_scale);
            }
        }

        // Update global head: weights += lr * avg_delta
        for i in 0..PERSONALIZATION_HEAD_DIM {
            self.global_head.weights[i] += self.global_lr * avg_delta[i];
        }

        self.rounds_completed += 1;
        Ok(())
    }

    /// Return the current global head weights for a device to download
    /// as its starting point for local fine-tuning.
    pub fn get_global_head(&self) -> &[f64] {
        &self.global_head.weights
    }

    // ─── Cold-Start Bias FedAvg ─────────────────────────────────────────

    /// Aggregate cold-start affinity biases from devices that completed
    /// their first N swipes. Keyed by intent bucket (e.g. "relationship",
    /// "casual", "unknown") so cohorts learn separately.
    ///
    /// Each device sends: (intent_bucket, bias_weights[5], num_swipes).
    /// bias_weights encode local adjustments to:
    ///   [interest_weight_delta, language_weight_delta, intent_weight_delta,
    ///    cf_weight_delta, geo_weight_delta]
    pub fn aggregate_cold_start(
        &mut self,
        updates: &[(String, Vec<f64>, i32)], // (intent_bucket, bias_delta, num_swipes)
    ) -> Result<usize, String> {
        // Group by intent bucket
        let mut by_bucket: HashMap<&str, Vec<(&Vec<f64>, i32)>> = HashMap::new();
        for (bucket, delta, n) in updates {
            if delta.len() != COLD_START_DIM {
                return Err(format!(
                    "Cold-start dim mismatch: expected {}, got {}",
                    COLD_START_DIM, delta.len()
                ));
            }
            by_bucket.entry(bucket.as_str()).or_default().push((delta, *n));
        }

        let mut buckets_updated = 0;

        for (bucket, entries) in &by_bucket {
            if entries.len() < self.min_clients {
                continue;
            }

            let total: i32 = entries.iter().map(|(_, n)| *n).sum();
            if total == 0 { continue; }

            let mut avg = vec![0.0; COLD_START_DIM];
            for (delta, n) in entries {
                let w = *n as f64 / total as f64;
                for i in 0..COLD_START_DIM {
                    avg[i] += delta[i] * w;
                }
            }

            if self.dp_enabled {
                for val in avg.iter_mut() {
                    *val += laplace_noise(self.dp_noise_scale * 2.0); // Extra noise for sparse data
                }
            }

            // Clamp biases to ±0.3 to prevent wild swings from few users
            for val in avg.iter_mut() {
                *val = val.clamp(-0.3, 0.3);
            }

            let bias = self.cold_start_biases
                .entry(bucket.to_string())
                .or_insert_with(ColdStartBias::new);

            for i in 0..COLD_START_DIM {
                // Exponential moving average to smooth over rounds
                bias.weights[i] = 0.7 * bias.weights[i] + 0.3 * avg[i];
            }
            bias.num_contributors += entries.len() as u64;
            buckets_updated += 1;
        }

        Ok(buckets_updated)
    }

    /// Get cold-start affinity bias for a user's intent bucket.
    /// Returns weight deltas to apply on top of the default AffinityScorer weights.
    pub fn cold_start_for(&self, intent: &str) -> Option<&ColdStartBias> {
        self.cold_start_biases.get(intent)
    }

    // ─── Notification Click Predictor FedAvg ────────────────────────────

    /// Aggregate on-device notification click predictor updates.
    /// Each device trains a tiny logistic model on local open/ignore outcomes
    /// with features: [hour_of_day_norm, day_of_week_norm, category_enc,
    ///                  time_since_last_open_norm, daily_notif_count_norm, bias].
    ///
    /// Returns updated global predictor weights as bandit priors.
    pub fn aggregate_notif_predictor(
        &mut self,
        updates: &[(Vec<f64>, i32)], // (weight_delta, num_opens+ignores)
    ) -> Result<(), String> {
        if updates.len() < self.min_clients {
            return Err(format!(
                "Not enough notif predictor updates: {} / {}",
                updates.len(), self.min_clients
            ));
        }

        let total: i32 = updates.iter().map(|(_, n)| *n).sum();
        if total == 0 {
            return Err("Total notif samples is zero".into());
        }

        for (d, _) in updates {
            if d.len() != NOTIF_PREDICTOR_DIM {
                return Err(format!(
                    "Notif predictor dim mismatch: expected {}, got {}",
                    NOTIF_PREDICTOR_DIM, d.len()
                ));
            }
        }

        let mut avg = vec![0.0; NOTIF_PREDICTOR_DIM];
        for (delta, n) in updates {
            let w = *n as f64 / total as f64;
            for i in 0..NOTIF_PREDICTOR_DIM {
                avg[i] += delta[i] * w;
            }
        }

        if self.dp_enabled {
            for val in avg.iter_mut() {
                *val += laplace_noise(self.dp_noise_scale);
            }
        }

        for i in 0..NOTIF_PREDICTOR_DIM {
            self.notif_predictor.weights[i] += self.global_lr * avg[i];
        }

        Ok(())
    }

    /// Predict click probability for a notification given context features.
    /// Used to set bandit priors: if predicted click prob is high, boost
    /// the arm's alpha prior.
    pub fn predict_notif_click(&self, features: &[f64]) -> f64 {
        self.notif_predictor.predict(features)
    }

    // ─── Stats ──────────────────────────────────────────────────────────

    /// Stats for monitoring, including feature dimensions the FL model uses.
    pub fn stats(&self) -> Value {
        let cold_start_stats: Vec<Value> = self.cold_start_biases
            .iter()
            .map(|(bucket, bias)| {
                serde_json::json!({
                    "intent": bucket,
                    "weights": bias.weights.to_vec(),
                    "contributors": bias.num_contributors,
                })
            })
            .collect();

        serde_json::json!({
            "global_lr": self.global_lr,
            "min_clients": self.min_clients,
            "dp_enabled": self.dp_enabled,
            "dp_noise_scale": self.dp_noise_scale,
            "rounds_completed": self.rounds_completed,
            "user_feature_dim": USER_FEATURE_DIM,
            "combined_state_dim": USER_FEATURE_DIM * 2,
            "personalization_head": {
                "dim": PERSONALIZATION_HEAD_DIM,
                "weights_sample": &self.global_head.weights[..5.min(PERSONALIZATION_HEAD_DIM)],
            },
            "cold_start": {
                "dim": COLD_START_DIM,
                "buckets": cold_start_stats,
            },
            "notif_predictor": {
                "dim": NOTIF_PREDICTOR_DIM,
                "weights": self.notif_predictor.weights.to_vec(),
            },
            "safety_boundary": FederationSafety::describe(),
            "feature_categories": {
                "profile": ["age_norm", "attractiveness", "profile_completeness", "verification_score", "photo_count", "height_norm", "has_profession", "gender_enc"],
                "richness": ["intent_enc", "language_count", "interest_count"],
                "engagement": ["activity_score", "like_rate", "match_rate"]
            }
        })
    }
}

impl Default for FederatedCoordinator {
    fn default() -> Self {
        Self::new()
    }
}

// ─── On-Device Personalization Head ─────────────────────────────────────────

/// Small last-layer that devices fine-tune on local swipe outcomes.
/// Input: 32 features from the frozen global model's penultimate layer.
/// Output: scalar compatibility score (dot product + bias).
///
/// On-device training: binary cross-entropy on like/pass with SGD,
/// then upload delta = (local_weights - downloaded_weights) to server.
#[derive(Debug, Clone)]
pub struct PersonalizationHead {
    /// 32 weights + 1 bias = 33 params
    pub weights: Vec<f64>,
}

impl PersonalizationHead {
    pub fn new() -> Self {
        // Xavier-ish init: small random-like values centered at 0
        let weights = vec![0.0; PERSONALIZATION_HEAD_DIM];
        // Set bias (last element) to 0
        // Weights start at 0 — device will fine-tune from here
        Self { weights }
    }

    /// Compute a score from intermediate features.
    /// features.len() should be 32 (PERSONALIZATION_HEAD_DIM - 1).
    pub fn score(&self, features: &[f64]) -> f64 {
        let feat_dim = PERSONALIZATION_HEAD_DIM - 1;
        if features.len() != feat_dim {
            return 0.0;
        }
        let mut dot = 0.0;
        for i in 0..feat_dim {
            dot += self.weights[i] * features[i];
        }
        dot += self.weights[feat_dim]; // bias
        // Sigmoid
        1.0 / (1.0 + (-dot).exp())
    }
}

impl Default for PersonalizationHead {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Cold-Start Affinity Bias ───────────────────────────────────────────────

/// Per-cohort affinity weight adjustments learned from early swipe behavior.
/// Applied on top of the default AffinityScorer weights for users with < 30 swipes.
///
/// Indices: [interest_delta, language_delta, intent_delta, cf_delta, geo_delta]
#[derive(Debug, Clone)]
pub struct ColdStartBias {
    pub weights: [f64; COLD_START_DIM],
    pub num_contributors: u64,
}

impl ColdStartBias {
    pub fn new() -> Self {
        Self {
            weights: [0.0; COLD_START_DIM],
            num_contributors: 0,
        }
    }

    /// Apply this bias to default affinity weights.
    /// Returns adjusted weights: (interest, language, intent, cf, geo).
    pub fn apply(
        &self,
        interest_w: f64,
        language_w: f64,
        intent_w: f64,
        cf_w: f64,
        geo_w: f64,
    ) -> (f64, f64, f64, f64, f64) {
        (
            (interest_w + self.weights[0]).clamp(0.05, 0.8),
            (language_w + self.weights[1]).clamp(0.05, 0.8),
            (intent_w + self.weights[2]).clamp(0.05, 0.8),
            (cf_w + self.weights[3]).clamp(0.0, 0.8),
            (geo_w + self.weights[4]).clamp(0.05, 0.8),
        )
    }
}

impl Default for ColdStartBias {
    fn default() -> Self {
        Self::new()
    }
}

// ─── On-Device Notification Click Predictor ─────────────────────────────────

/// Tiny logistic regression predicting P(open | context) for notifications.
/// Trained on-device from local push open/ignore outcomes.
/// Federated updates let the server learn per-cohort timing patterns
/// without centralizing raw open behavior data.
///
/// Features (6):
///   0: hour_of_day / 24.0
///   1: day_of_week / 7.0
///   2: category encoding (match=1.0, re_engage=0.5, like=0.75, message=0.25)
///   3: hours_since_last_open / 24.0 (capped at 1.0)
///   4: daily_notif_count / 12.0 (capped at 1.0)
///   5: bias (always 1.0)
#[derive(Debug, Clone)]
pub struct NotifClickPredictor {
    pub weights: Vec<f64>,
}

impl NotifClickPredictor {
    pub fn new() -> Self {
        // Initial weights: slight positive bias for evening hours, negative for high notif count
        Self {
            weights: vec![
                0.3,  // hour — positive means later hours → higher click prob
                0.0,  // day_of_week — neutral
                0.2,  // category — matches get clicked more
                -0.1, // hours_since_last_open — more recency → less likely (fatigue)
                -0.3, // daily_notif_count — more notifs → less likely (fatigue)
                0.0,  // bias
            ],
        }
    }

    /// Predict click probability given context features.
    pub fn predict(&self, features: &[f64]) -> f64 {
        if features.len() != NOTIF_PREDICTOR_DIM {
            return 0.5; // Neutral prior
        }
        let mut logit = 0.0;
        for (w, f) in self.weights.iter().zip(features.iter()) {
            logit += w * f;
        }
        1.0 / (1.0 + (-logit).exp())
    }

    /// Compute bandit prior adjustment from predicted click probability.
    /// Returns (alpha_boost, beta_boost) to add to the variant arm's prior.
    /// High predicted P(click) → boost alpha (encourage sending).
    /// Low predicted P(click) → boost beta (discourage sending).
    pub fn to_bandit_prior(&self, features: &[f64]) -> (f64, f64) {
        let p = self.predict(features);
        // Scale to a mild prior: max 2.0 pseudo-observations
        let strength = 2.0;
        (strength * p, strength * (1.0 - p))
    }
}

impl Default for NotifClickPredictor {
    fn default() -> Self {
        Self::new()
    }
}

// ─── Federation Safety Boundary ─────────────────────────────────────────────

/// Explicit policy on what data may and may not be federated.
/// This struct is informational — it documents the safety contract
/// and provides validation for incoming device updates.
pub struct FederationSafety;

impl FederationSafety {
    /// Data that IS safe to federate (behavioral gradients only):
    /// - Personalization head weight deltas (32 + 1 params)
    /// - Affinity bias deltas (5 params)
    /// - Notification click predictor deltas (6 params)
    /// - Swipe count per training round (scalar)
    ///
    /// Data that must NEVER leave the device or be federated:
    /// - Face embeddings (ArcFace vectors)
    /// - Raw photo data or NSFW scores
    /// - Exact GPS coordinates
    /// - Device risk/fingerprint signals
    /// - Message content or conversation data
    /// - Specific user IDs that were liked/passed
    pub fn describe() -> Value {
        serde_json::json!({
            "allowed": {
                "personalization_head_delta": {
                    "dim": PERSONALIZATION_HEAD_DIM,
                    "type": "weight gradients from local swipe training"
                },
                "cold_start_bias_delta": {
                    "dim": COLD_START_DIM,
                    "type": "affinity weight adjustments from early swipes"
                },
                "notif_click_delta": {
                    "dim": NOTIF_PREDICTOR_DIM,
                    "type": "notification open predictor gradients"
                },
                "scalar_counts": "num_swipes, num_opens, num_ignores (aggregated)"
            },
            "forbidden": [
                "face_embeddings",
                "photo_data",
                "nsfw_scores",
                "exact_gps",
                "device_fingerprints",
                "message_content",
                "specific_liked_user_ids"
            ],
            "dp_guarantee": "All federated updates have Laplace noise with configurable scale"
        })
    }

    /// Validate that an incoming device update doesn't exceed allowed dimensions.
    /// Returns Err if the update is suspiciously large (possible data exfiltration).
    pub fn validate_update(
        update_type: &str,
        data: &[f64],
    ) -> Result<(), String> {
        let max_dim = match update_type {
            "head" => PERSONALIZATION_HEAD_DIM,
            "cold_start" => COLD_START_DIM,
            "notif" => NOTIF_PREDICTOR_DIM,
            _ => return Err(format!("Unknown update type: {}", update_type)),
        };

        if data.len() != max_dim {
            return Err(format!(
                "Update dimension {} doesn't match expected {} for type '{}'",
                data.len(), max_dim, update_type
            ));
        }

        // Check for NaN/Inf (could indicate adversarial client)
        for (i, val) in data.iter().enumerate() {
            if val.is_nan() || val.is_infinite() {
                return Err(format!("Invalid value at index {}: {}", i, val));
            }
            // Gradient clipping: reject absurdly large deltas
            if val.abs() > 100.0 {
                return Err(format!(
                    "Value at index {} exceeds gradient clip threshold: {}",
                    i, val
                ));
            }
        }

        Ok(())
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct AggregationResult {
    pub aggregated_weights: Value,
    pub avg_loss: f64,
    pub avg_accuracy: f64,
    pub num_clients: usize,
    pub total_samples: i32,
    pub dp_applied: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fedavg_basic() {
        let coord = FederatedCoordinator {
            dp_enabled: false, // Disable noise for deterministic test
            ..FederatedCoordinator::new()
        };

        let w1 = serde_json::to_value(vec![1.0, 2.0, 3.0]).unwrap();
        let w2 = serde_json::to_value(vec![3.0, 2.0, 1.0]).unwrap();

        let updates = vec![
            (w1, 100, 0.5, Some(0.8)),
            (w2, 100, 0.3, Some(0.9)),
        ];

        let result = coord.aggregate(&updates).unwrap();
        let weights: Vec<f64> = serde_json::from_value(result.aggregated_weights).unwrap();
        // Equal samples => simple average
        assert!((weights[0] - 2.0).abs() < 1e-6);
        assert!((weights[1] - 2.0).abs() < 1e-6);
        assert!((weights[2] - 2.0).abs() < 1e-6);
    }

    #[test]
    fn test_fedavg_weighted() {
        let coord = FederatedCoordinator {
            dp_enabled: false,
            ..FederatedCoordinator::new()
        };

        let w1 = serde_json::to_value(vec![0.0, 0.0]).unwrap();
        let w2 = serde_json::to_value(vec![10.0, 10.0]).unwrap();

        let updates = vec![
            (w1, 100, 0.5, None),
            (w2, 300, 0.3, None), // 3x more samples
        ];

        let result = coord.aggregate(&updates).unwrap();
        let weights: Vec<f64> = serde_json::from_value(result.aggregated_weights).unwrap();
        // Weighted: (0*100 + 10*300)/400 = 7.5
        assert!((weights[0] - 7.5).abs() < 1e-6);
    }

    #[test]
    fn test_fedavg_with_dp() {
        let coord = FederatedCoordinator::new(); // DP enabled

        let w1 = serde_json::to_value(vec![1.0; 10]).unwrap();
        let w2 = serde_json::to_value(vec![1.0; 10]).unwrap();

        let updates = vec![
            (w1, 50, 0.5, Some(0.7)),
            (w2, 50, 0.4, Some(0.8)),
        ];

        let result = coord.aggregate(&updates).unwrap();
        assert!(result.dp_applied);
        let weights: Vec<f64> = serde_json::from_value(result.aggregated_weights).unwrap();
        // With noise, weights should be close to 1.0 but not exact
        for w in &weights {
            assert!((w - 1.0).abs() < 5.0); // Very loose bound due to noise
        }
    }

    #[test]
    fn test_fedavg_too_few_clients() {
        let coord = FederatedCoordinator::new();
        let w1 = serde_json::to_value(vec![1.0]).unwrap();
        let updates = vec![(w1, 10, 0.5, None)];
        assert!(coord.aggregate(&updates).is_err());
    }

    #[test]
    fn test_personalization_head_score() {
        let mut head = PersonalizationHead::new();
        // Set weights to known values
        for i in 0..32 {
            head.weights[i] = 0.1;
        }
        head.weights[32] = 0.0; // bias

        let features = vec![1.0; 32];
        let score = head.score(&features);
        // dot = 32 * 0.1 = 3.2, sigmoid(3.2) ≈ 0.961
        assert!(score > 0.9, "Score should be high: {}", score);

        // Negative features → low score
        let neg_features = vec![-1.0; 32];
        let neg_score = head.score(&neg_features);
        assert!(neg_score < 0.1, "Neg score should be low: {}", neg_score);
    }

    #[test]
    fn test_aggregate_head_deltas() {
        let mut coord = FederatedCoordinator {
            dp_enabled: false,
            ..FederatedCoordinator::new()
        };

        let delta1 = vec![0.1; PERSONALIZATION_HEAD_DIM];
        let delta2 = vec![0.3; PERSONALIZATION_HEAD_DIM];

        let deltas = vec![
            (delta1, 50),
            (delta2, 50),
        ];

        coord.aggregate_head_deltas(&deltas).unwrap();

        // avg_delta = [0.2; 33], applied with lr=0.1
        // global_head += 0.1 * 0.2 = 0.02
        for w in &coord.global_head.weights {
            assert!((*w - 0.02).abs() < 1e-6, "Weight should be 0.02: {}", w);
        }
        assert_eq!(coord.rounds_completed, 1);
    }

    #[test]
    fn test_cold_start_bias() {
        let mut coord = FederatedCoordinator {
            dp_enabled: false,
            ..FederatedCoordinator::new()
        };

        let updates = vec![
            ("relationship".into(), vec![0.1, -0.05, 0.0, 0.05, -0.1], 20),
            ("relationship".into(), vec![0.1, -0.05, 0.0, 0.05, -0.1], 20),
        ];

        let buckets = coord.aggregate_cold_start(&updates).unwrap();
        assert_eq!(buckets, 1);

        let bias = coord.cold_start_for("relationship").unwrap();
        // EMA: 0.7 * 0 + 0.3 * avg = 0.3 * [0.1, -0.05, 0.0, 0.05, -0.1]
        assert!((bias.weights[0] - 0.03).abs() < 0.01);
        assert!((bias.weights[1] - (-0.015)).abs() < 0.01);

        // Apply to default affinity weights
        let (iw, lw, _itw, _cfw, _gw) = bias.apply(0.35, 0.15, 0.20, 0.30, 0.20);
        assert!(iw > 0.35); // interest boosted
        assert!(lw < 0.15); // language reduced
    }

    #[test]
    fn test_notif_click_predictor() {
        let pred = NotifClickPredictor::new();

        // Evening hour, match category, recent open, low daily count
        let evening_match = vec![20.0 / 24.0, 5.0 / 7.0, 1.0, 0.1, 2.0 / 12.0, 1.0];
        let p = pred.predict(&evening_match);
        assert!(p > 0.4, "Evening match should have decent click prob: {}", p);

        // Late night, re-engage, stale, many notifs today
        let bad_timing = vec![3.0 / 24.0, 1.0 / 7.0, 0.5, 1.0, 1.0, 1.0];
        let p_bad = pred.predict(&bad_timing);
        assert!(p_bad < p, "Bad timing should have lower prob: {} vs {}", p_bad, p);

        // Bandit prior
        let (alpha, beta) = pred.to_bandit_prior(&evening_match);
        assert!(alpha > beta, "Good context should boost alpha: {}/{}", alpha, beta);
    }

    #[test]
    fn test_federation_safety_validation() {
        // Valid head update
        let valid = vec![0.01; PERSONALIZATION_HEAD_DIM];
        assert!(FederationSafety::validate_update("head", &valid).is_ok());

        // Wrong dimension
        let wrong_dim = vec![0.01; 10];
        assert!(FederationSafety::validate_update("head", &wrong_dim).is_err());

        // NaN value
        let with_nan = {
            let mut v = vec![0.01; PERSONALIZATION_HEAD_DIM];
            v[5] = f64::NAN;
            v
        };
        assert!(FederationSafety::validate_update("head", &with_nan).is_err());

        // Gradient too large
        let too_large = {
            let mut v = vec![0.01; PERSONALIZATION_HEAD_DIM];
            v[0] = 200.0;
            v
        };
        assert!(FederationSafety::validate_update("head", &too_large).is_err());

        // Unknown type
        assert!(FederationSafety::validate_update("photos", &[]).is_err());
    }

    #[test]
    fn test_notif_predictor_fedavg() {
        let mut coord = FederatedCoordinator {
            dp_enabled: false,
            ..FederatedCoordinator::new()
        };

        let initial_w0 = coord.notif_predictor.weights[0];

        let delta1 = vec![0.1; NOTIF_PREDICTOR_DIM];
        let delta2 = vec![0.1; NOTIF_PREDICTOR_DIM];

        coord.aggregate_notif_predictor(&[
            (delta1, 100),
            (delta2, 100),
        ]).unwrap();

        // avg = [0.1; 6], applied with lr=0.1 → +0.01
        let expected = initial_w0 + 0.01;
        assert!(
            (coord.notif_predictor.weights[0] - expected).abs() < 1e-6,
            "w[0] should be {}: {}",
            expected, coord.notif_predictor.weights[0]
        );
    }

    #[test]
    fn test_fl_stats_shape_for_metrics_export() {
        // Verifies the stats() JSON has all fields the /metrics endpoint extracts
        let mut coord = FederatedCoordinator::new();
        // Simulate a round so rounds_completed > 0
        coord.rounds_completed = 3;
        // Add a cold-start bucket
        coord.cold_start_biases.insert("casual".into(), ColdStartBias {
            weights: [0.1, -0.05, 0.0, 0.05, -0.1],
            num_contributors: 10,
        });

        let stats = coord.stats();

        // rounds_completed
        assert_eq!(stats["rounds_completed"].as_u64().unwrap(), 3);

        // personalization_head.weights_sample exists and is an array
        let head_sample = stats["personalization_head"]["weights_sample"]
            .as_array()
            .expect("weights_sample should be array");
        assert!(!head_sample.is_empty(), "weights_sample should not be empty");

        // cold_start.buckets exists and has our bucket
        let buckets = stats["cold_start"]["buckets"]
            .as_array()
            .expect("buckets should be array");
        assert_eq!(buckets.len(), 1);
        assert_eq!(buckets[0]["intent"].as_str().unwrap(), "casual");
        assert_eq!(buckets[0]["contributors"].as_u64().unwrap(), 10);

        // notif_predictor.weights
        let notif_w = stats["notif_predictor"]["weights"]
            .as_array()
            .expect("notif weights should be array");
        assert_eq!(notif_w.len(), NOTIF_PREDICTOR_DIM);

        // dp_enabled
        assert!(stats["dp_enabled"].is_boolean());

        // safety_boundary exists
        assert!(stats["safety_boundary"].is_object());
    }

    #[test]
    fn test_fl_gauge_norm_calculation() {
        // Mirrors the L2 norm logic in prometheus_metrics() for head weights
        let weights = vec![3.0, 4.0]; // L2 = 5.0
        let norm: f64 = weights.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((norm - 5.0).abs() < 1e-10);

        // Zero weights → norm 0
        let zeros = vec![0.0; PERSONALIZATION_HEAD_DIM];
        let zero_norm: f64 = zeros.iter().map(|x| x * x).sum::<f64>().sqrt();
        assert!((zero_norm).abs() < 1e-10);

        // After aggregation, norm should be small but non-zero
        let mut coord = FederatedCoordinator {
            dp_enabled: false,
            ..FederatedCoordinator::new()
        };
        let deltas = vec![
            (vec![0.5; PERSONALIZATION_HEAD_DIM], 100),
            (vec![0.5; PERSONALIZATION_HEAD_DIM], 100),
        ];
        coord.aggregate_head_deltas(&deltas).unwrap();
        let head_norm: f64 = coord.global_head.weights.iter()
            .map(|x| x * x).sum::<f64>().sqrt();
        // lr=0.1 * avg_delta=0.5 = 0.05 per weight, norm = sqrt(33 * 0.05^2) ≈ 0.287
        assert!(head_norm > 0.2 && head_norm < 0.4,
            "Head norm after 1 round should be ~0.287: {}", head_norm);

        // Notif predictor norm
        let notif_norm: f64 = coord.notif_predictor.weights.iter()
            .map(|x| x * x).sum::<f64>().sqrt();
        // Initial weights are small but non-zero
        assert!(notif_norm < 5.0, "Initial notif norm should be small: {}", notif_norm);
    }

    #[test]
    fn test_fl_stability_thresholds() {
        // Verify that alert thresholds are reasonable vs actual norms
        let mut coord = FederatedCoordinator {
            dp_enabled: false,
            ..FederatedCoordinator::new()
        };

        // Simulate 50 rounds of normal deltas
        for _ in 0..50 {
            let deltas = vec![
                (vec![0.1; PERSONALIZATION_HEAD_DIM], 100),
                (vec![0.1; PERSONALIZATION_HEAD_DIM], 100),
            ];
            coord.aggregate_head_deltas(&deltas).unwrap();
        }

        let head_norm: f64 = coord.global_head.weights.iter()
            .map(|x| x * x).sum::<f64>().sqrt();
        // After 50 rounds: weight ≈ 50 * 0.1 * 0.1 = 0.5 each
        // norm = sqrt(33 * 0.25) ≈ 2.87
        // Alert threshold is 10.0 — should NOT fire for normal training
        assert!(head_norm < 10.0,
            "Head norm after 50 normal rounds ({}) should be below alert threshold 10.0", head_norm);

        // Simulate adversarial deltas (large values)
        let adversarial = vec![
            (vec![90.0; PERSONALIZATION_HEAD_DIM], 100),
            (vec![90.0; PERSONALIZATION_HEAD_DIM], 100),
        ];
        coord.aggregate_head_deltas(&adversarial).unwrap();
        let post_attack_norm: f64 = coord.global_head.weights.iter()
            .map(|x| x * x).sum::<f64>().sqrt();
        // 0.5 + 0.1*90 = 9.5 each → norm = sqrt(33 * 90.25) ≈ 54.6
        // This WOULD fire the alert at threshold 10.0 — correct behavior
        assert!(post_attack_norm > 10.0,
            "Head norm after adversarial round ({}) should exceed alert threshold", post_attack_norm);
    }
}
