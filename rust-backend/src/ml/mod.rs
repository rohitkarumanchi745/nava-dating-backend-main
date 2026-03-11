pub mod affinity;
pub mod engagement;
pub mod features;
pub mod federated;
pub mod geo;
pub mod linucb;
pub mod math;
pub mod rl_agent;

use sqlx::PgPool;
use tracing::{info, warn};

use self::affinity::{AffinityScorer, CoLikeMatrix};
use self::engagement::EngagementScorer;
use self::features::{combine_features, FeatureDefaults, UserFeatures, USER_FEATURE_DIM};
use self::federated::FederatedCoordinator;
use self::geo::GeoScorer;
use self::linucb::{ArmState, LinUcbBandit};
use self::rl_agent::{RlAgent, RlCheckpoint, SwipeExperience};

/// Unified ML service combining RL, LinUCB, Federated Learning, geo scoring,
/// affinity scoring, and engagement optimization.
/// Lives in `AppState` behind `Arc<RwLock<MlService>>`.
pub struct MlService {
    pub rl_agent: RlAgent,
    pub linucb: LinUcbBandit,
    pub federated: FederatedCoordinator,
    train_counter: u64,
    /// Cached population-level feature defaults for fallback
    pub feature_defaults: FeatureDefaults,
    // --- New scoring layers ---
    /// Gravity model distance scoring + density smoothing
    pub geo: GeoScorer,
    /// Interest/language overlap + collaborative filtering
    pub affinity: AffinityScorer,
    /// Co-like matrix for CF signals (rebuilt periodically)
    pub co_likes: CoLikeMatrix,
    /// Churn prediction + send-time optimization + notification bandit
    pub engagement: EngagementScorer,
    // --- Shadow scoring metrics ---
    shadow_agreement_count: u64,
    shadow_total_count: u64,
}

impl MlService {
    /// Create with warm-start: loads RL checkpoint and LinUCB arms from DB.
    pub async fn new(pool: &PgPool) -> Self {
        // Load population-level feature defaults
        let feature_defaults = FeatureDefaults::from_population(pool).await;
        info!("ML feature defaults loaded from population stats");

        // Warm-start RL agent from checkpoint
        let rl_agent = match Self::load_rl_checkpoint(pool).await {
            Some(ckpt) => {
                info!(
                    "RL agent warm-started from checkpoint (epsilon={:.4})",
                    ckpt.epsilon
                );
                RlAgent::from_checkpoint(ckpt)
            }
            None => {
                info!("RL agent initialized fresh (no checkpoint found)");
                RlAgent::new()
            }
        };

        // Warm-start LinUCB arms
        let mut linucb = LinUcbBandit::new();
        let arms_loaded = Self::load_linucb_arms(pool, &mut linucb).await;
        if arms_loaded > 0 {
            info!("LinUCB warm-started with {} arms from DB", arms_loaded);
        }

        // Build co-like matrix and engagement scorer from DB
        let co_likes = CoLikeMatrix::build(pool, 30, 50_000).await;
        info!("Co-like matrix built for collaborative filtering");

        let engagement = EngagementScorer::build(pool).await;
        info!("Engagement scorer initialized with send-time histograms");

        Self {
            rl_agent,
            linucb,
            federated: FederatedCoordinator::new(),
            train_counter: 0,
            feature_defaults,
            geo: GeoScorer::default(),
            affinity: AffinityScorer::default(),
            co_likes,
            engagement,
            shadow_agreement_count: 0,
            shadow_total_count: 0,
        }
    }

    /// Fallback constructor without DB (for tests or when DB is unavailable at init).
    pub fn new_without_db() -> Self {
        Self {
            rl_agent: RlAgent::new(),
            linucb: LinUcbBandit::new(),
            federated: FederatedCoordinator::new(),
            train_counter: 0,
            feature_defaults: FeatureDefaults::hardcoded(),
            geo: GeoScorer::default(),
            affinity: AffinityScorer::default(),
            co_likes: CoLikeMatrix::new(),
            engagement: EngagementScorer::new(),
            shadow_agreement_count: 0,
            shadow_total_count: 0,
        }
    }

    // -------------------------------------------------------------------------
    // Checkpoint persistence
    // -------------------------------------------------------------------------

    async fn load_rl_checkpoint(pool: &PgPool) -> Option<RlCheckpoint> {
        let row = sqlx::query_as::<_, (serde_json::Value,)>(
            "SELECT weights FROM fl_models WHERE model_type = 'rl_global' AND is_active = TRUE ORDER BY version DESC LIMIT 1",
        )
        .fetch_optional(pool)
        .await
        .ok()??;

        serde_json::from_value(row.0).ok()
    }

    async fn load_linucb_arms(pool: &PgPool, bandit: &mut LinUcbBandit) -> usize {
        #[derive(sqlx::FromRow)]
        struct ArmRow {
            arm_id: String,
            a_matrix: Option<serde_json::Value>,
            b_vector: Option<serde_json::Value>,
            num_pulls: Option<i32>,
            total_reward: Option<f64>,
        }

        let rows = sqlx::query_as::<_, ArmRow>(
            "SELECT arm_id, a_matrix, b_vector, num_pulls, total_reward FROM bandit_arm_stats WHERE arm_type = 'global'",
        )
        .fetch_all(pool)
        .await
        .unwrap_or_default();

        let mut count = 0;
        for row in rows {
            if let (Some(a), Some(b)) = (&row.a_matrix, &row.b_vector) {
                if let Some(state) = ArmState::from_json(
                    a,
                    b,
                    row.num_pulls.unwrap_or(0) as u64,
                    row.total_reward.unwrap_or(0.0),
                ) {
                    bandit.load_arm(&row.arm_id, state);
                    count += 1;
                }
            }
        }
        count
    }

    /// Save RL weights and dirty LinUCB arms to DB.
    pub async fn save_checkpoint(&self, pool: &PgPool) {
        // Save RL checkpoint
        let ckpt = self.rl_agent.to_checkpoint();
        let weights_json = match serde_json::to_value(&ckpt) {
            Ok(v) => v,
            Err(e) => {
                warn!("Failed to serialize RL checkpoint: {e}");
                return;
            }
        };

        // Deactivate previous versions and get next version number
        let _ = sqlx::query("UPDATE fl_models SET is_active = FALSE WHERE model_type = 'rl_global'")
            .execute(pool)
            .await;

        let max_version: i32 = sqlx::query_scalar(
            "SELECT COALESCE(MAX(version), 0) FROM fl_models WHERE model_type = 'rl_global'",
        )
        .fetch_one(pool)
        .await
        .unwrap_or(0);

        if let Err(e) = sqlx::query(
            r#"INSERT INTO fl_models (model_type, version, weights, is_active, total_rounds, created_at)
               VALUES ('rl_global', $1, $2, TRUE, $3, NOW())"#,
        )
        .bind(max_version + 1)
        .bind(&weights_json)
        .bind(self.train_counter as i32)
        .execute(pool)
        .await
        {
            warn!("Failed to save RL checkpoint: {e}");
        }

        // Save LinUCB arms with pulls > 0 (delete + insert since no unique constraint)
        for arm_id in self.linucb.arm_ids() {
            if let Some(arm) = self.linucb.get_arm(&arm_id) {
                if arm.num_pulls == 0 {
                    continue;
                }
                let (a_json, b_json) = arm.to_json();
                // Delete existing row for this arm
                let _ = sqlx::query(
                    "DELETE FROM bandit_arm_stats WHERE arm_id = $1 AND arm_type = 'global'",
                )
                .bind(&arm_id)
                .execute(pool)
                .await;
                // Insert fresh
                let _ = sqlx::query(
                    r#"INSERT INTO bandit_arm_stats (arm_id, arm_type, a_matrix, b_vector, num_pulls, total_reward, updated_at)
                       VALUES ($1, 'global', $2, $3, $4, $5, NOW())"#,
                )
                .bind(&arm_id)
                .bind(&a_json)
                .bind(&b_json)
                .bind(arm.num_pulls as i32)
                .bind(arm.total_reward)
                .execute(pool)
                .await;
            }
        }
    }

    // -------------------------------------------------------------------------
    // Scoring & ranking
    // -------------------------------------------------------------------------

    /// Score and rank candidate user IDs for a given user.
    /// Blends RL score with geo, affinity, and engagement boosts.
    /// Shadow-scores via LinUCB for observability.
    ///
    /// Final score = 0.55 * rl_score + 0.20 * geo_score + 0.20 * affinity_score
    ///             + churn_boost (up to 0.15 for at-risk users)
    pub async fn rank_candidates(
        &mut self,
        pool: &PgPool,
        user_id: i32,
        candidate_ids: &[i32],
    ) -> Vec<(i32, f64)> {
        let user_features = match UserFeatures::from_db(pool, user_id).await {
            Ok(f) => f,
            Err(_) => self.feature_defaults.for_user(user_id),
        };

        // Fetch user tags for affinity scoring
        let (user_interests, user_langs, user_intent) =
            affinity::fetch_user_tags(pool, user_id).await;

        // Fetch user location for geo scoring
        let user_loc = Self::fetch_user_location(pool, user_id).await;

        // Churn boost for the requesting user
        let churn_features = engagement::ChurnPredictor::extract_features(pool, user_id).await;
        let churn_pred = self.engagement.churn.predict(&churn_features);
        let churn_boost = self.engagement.churn_boost(&churn_pred);

        // Fetch candidates who have super-liked this user — they get a ranking boost
        let super_likers: std::collections::HashSet<i32> = sqlx::query_scalar::<_, i64>(
            "SELECT from_user_id FROM swipes WHERE to_user_id = $1 AND action = 'superlike'"
        )
        .bind(user_id as i64)
        .fetch_all(pool)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|id| id as i32)
        .collect();

        let mut rl_scored: Vec<(i32, f64)> = Vec::with_capacity(candidate_ids.len());
        let mut linucb_scored: Vec<(i32, f64)> = Vec::with_capacity(candidate_ids.len());
        let mut blended: Vec<(i32, f64)> = Vec::with_capacity(candidate_ids.len());

        for &cid in candidate_ids {
            let candidate_features = match UserFeatures::from_db(pool, cid).await {
                Ok(f) => f,
                Err(_) => self.feature_defaults.for_user(cid),
            };

            let state = combine_features(&user_features, &candidate_features);

            // Primary: RL scoring
            let rl_score = self.rl_agent.score_candidate(user_id, &state);
            rl_scored.push((cid, rl_score));

            // Shadow: LinUCB scoring (for observability, not used for final ranking)
            let arm_id = format!("user_{}", cid);
            let linucb_score = self.linucb.score(&arm_id, &state);
            linucb_scored.push((cid, linucb_score));

            // Geo scoring
            let geo_score = if let Some((ulat, ulng)) = user_loc {
                let cloc = Self::fetch_user_location(pool, cid).await;
                if let Some((clat, clng)) = cloc {
                    let dist = geo::GeoScorer::haversine(ulat, ulng, clat, clng);
                    let density = self.geo.local_density(pool, cid).await;
                    self.geo.score(dist, density).density_adjusted_score
                } else {
                    0.5 // Unknown location — neutral
                }
            } else {
                0.5
            };

            // Affinity scoring
            let (cand_interests, cand_langs, cand_intent) =
                affinity::fetch_user_tags(pool, cid).await;
            let cf_score = self.co_likes.cf_score(user_id, cid);
            let affinity_result = self.affinity.score(
                &user_interests,
                &cand_interests,
                &user_langs,
                &cand_langs,
                user_intent.as_deref(),
                cand_intent.as_deref(),
                cf_score,
            );

            // Super like boost: +0.15 if this candidate super-liked the user
            // This pushes super-likers ~15 percentile points higher in ranking
            let super_like_boost = if super_likers.contains(&cid) { 0.15 } else { 0.0 };

            // Blend: RL 55% + Geo 20% + Affinity 20% + Churn boost + Super Like boost
            let final_score = 0.55 * rl_score
                + 0.20 * geo_score
                + 0.20 * affinity_result.total
                + churn_boost
                + super_like_boost;

            blended.push((cid, final_score));
        }

        // Shadow agreement: compare top-half sets
        if candidate_ids.len() >= 4 {
            let half = candidate_ids.len() / 2;
            let mut rl_sorted = rl_scored.clone();
            let mut linucb_sorted = linucb_scored;
            rl_sorted.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
            linucb_sorted
                .sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

            let rl_top: std::collections::HashSet<i32> =
                rl_sorted.iter().take(half).map(|(id, _)| *id).collect();
            let linucb_top: std::collections::HashSet<i32> =
                linucb_sorted.iter().take(half).map(|(id, _)| *id).collect();
            let overlap = rl_top.intersection(&linucb_top).count();

            self.shadow_total_count += 1;
            if overlap as f64 / half as f64 >= 0.5 {
                self.shadow_agreement_count += 1;
            }
        }

        blended.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        blended
    }

    /// Fetch lat/lng for a user (returns None if not set).
    async fn fetch_user_location(pool: &PgPool, user_id: i32) -> Option<(f64, f64)> {
        #[derive(sqlx::FromRow)]
        struct LocRow {
            latitude: Option<f64>,
            longitude: Option<f64>,
        }
        let row = sqlx::query_as::<_, LocRow>(
            "SELECT latitude, longitude FROM users WHERE id = $1",
        )
        .bind(user_id)
        .fetch_optional(pool)
        .await
        .ok()??;
        match (row.latitude, row.longitude) {
            (Some(lat), Some(lng)) => Some((lat, lng)),
            _ => None,
        }
    }

    /// Record a swipe (like/pass) and trigger periodic training + checkpointing.
    pub async fn record_swipe(
        &mut self,
        pool: &PgPool,
        user_id: i32,
        candidate_id: i32,
        liked: bool,
    ) {
        self.record_swipe_weighted(pool, user_id, candidate_id, liked, false).await;
    }

    /// Record a swipe with optional super-like weighting.
    /// Super likes get 3× reward signal, teaching the model that super-liked
    /// profiles are high-intent matches worth surfacing.
    pub async fn record_swipe_weighted(
        &mut self,
        pool: &PgPool,
        user_id: i32,
        candidate_id: i32,
        liked: bool,
        is_super_like: bool,
    ) {
        let reward = if is_super_like {
            3.0  // 3× stronger positive signal for super likes
        } else if liked {
            1.0
        } else {
            -0.5
        };
        let action = if liked { 1 } else { 0 };

        // Get features with graceful fallback
        let user_f = match UserFeatures::from_db(pool, user_id).await {
            Ok(f) => f,
            Err(_) => self.feature_defaults.for_user(user_id),
        };
        let cand_f = match UserFeatures::from_db(pool, candidate_id).await {
            Ok(f) => f,
            Err(_) => self.feature_defaults.for_user(candidate_id),
        };
        let state = combine_features(&user_f, &cand_f);

        self.rl_agent.record_experience(SwipeExperience {
            user_id,
            candidate_id,
            state: state.clone(),
            action,
            reward,
        });

        // Update LinUCB arm
        let arm_id = format!("user_{}", candidate_id);
        self.linucb.update(&arm_id, &state, reward);

        // Train every 10 swipes, checkpoint after training
        self.train_counter += 1;
        if self.train_counter % 10 == 0 {
            self.rl_agent.train_batch(32);
            self.save_checkpoint(pool).await;
        }
    }

    /// Get combined ML stats for the /ml/stats endpoint.
    pub fn ml_stats(&self) -> serde_json::Value {
        let agreement_rate = if self.shadow_total_count > 0 {
            self.shadow_agreement_count as f64 / self.shadow_total_count as f64
        } else {
            0.0
        };

        serde_json::json!({
            "rl_agent": self.rl_agent.stats(),
            "linucb": self.linucb.stats(),
            "federated": self.federated.stats(),
            "geo": {
                "beta": self.geo.beta,
                "max_distance_km": self.geo.max_distance_km,
                "max_distance_miles": self.geo.max_distance_km * 0.621371,
                "density_bandwidth_km": self.geo.density_bandwidth_km,
                "display_unit": match self.geo.display_unit {
                    geo::DistanceUnit::Kilometers => "km",
                    geo::DistanceUnit::Miles => "miles",
                },
            },
            "affinity": {
                "interest_weight": self.affinity.interest_weight,
                "language_weight": self.affinity.language_weight,
                "intent_weight": self.affinity.intent_weight,
                "cf_weight": self.affinity.cf_weight,
            },
            "engagement": self.engagement.stats(),
            "ranking_blend": {
                "rl_weight": 0.55,
                "geo_weight": 0.20,
                "affinity_weight": 0.20,
                "churn_boost_max": 0.15,
            },
            "train_counter": self.train_counter,
            "feature_dim": USER_FEATURE_DIM,
            "state_dim": USER_FEATURE_DIM * 2,
            "shadow": {
                "agreement_rate": agreement_rate,
                "total_comparisons": self.shadow_total_count,
            },
        })
    }
}

impl Default for MlService {
    fn default() -> Self {
        Self::new_without_db()
    }
}
