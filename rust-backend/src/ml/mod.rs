pub mod features;
pub mod federated;
pub mod linucb;
pub mod math;
pub mod rl_agent;

use sqlx::PgPool;

use self::features::{UserFeatures, combine_features, USER_FEATURE_DIM};
use self::federated::FederatedCoordinator;
use self::linucb::LinUcbBandit;
use self::rl_agent::{RlAgent, SwipeExperience};

/// Unified ML service combining RL, LinUCB, and Federated Learning.
/// Lives in `AppState` behind `Arc<RwLock<MlService>>`.
pub struct MlService {
    pub rl_agent: RlAgent,
    pub linucb: LinUcbBandit,
    pub federated: FederatedCoordinator,
    train_counter: u64,
}

impl MlService {
    pub fn new() -> Self {
        Self {
            rl_agent: RlAgent::new(),
            linucb: LinUcbBandit::new(),
            federated: FederatedCoordinator::new(),
            train_counter: 0,
        }
    }

    /// Score and rank candidate user IDs for a given user.
    /// Fetches features from DB, scores via RL agent, returns sorted (candidate_id, score).
    pub async fn rank_candidates(
        &self,
        pool: &PgPool,
        user_id: i32,
        candidate_ids: &[i32],
    ) -> Vec<(i32, f64)> {
        let user_features = match UserFeatures::from_db(pool, user_id).await {
            Ok(f) => f,
            Err(_) => return candidate_ids.iter().map(|&id| (id, 0.0)).collect(),
        };

        let mut scored: Vec<(i32, f64)> = Vec::with_capacity(candidate_ids.len());
        for &cid in candidate_ids {
            let candidate_features = match UserFeatures::from_db(pool, cid).await {
                Ok(f) => f,
                Err(_) => {
                    scored.push((cid, 0.0));
                    continue;
                }
            };

            let state = combine_features(&user_features, &candidate_features);
            let score = self.rl_agent.score_candidate(user_id, &state);
            scored.push((cid, score));
        }

        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    /// Record a swipe (like/pass) and trigger periodic training.
    pub async fn record_swipe(
        &mut self,
        pool: &PgPool,
        user_id: i32,
        candidate_id: i32,
        liked: bool,
    ) {
        let reward = if liked { 1.0 } else { -0.5 };
        let action = if liked { 1 } else { 0 };

        // Try to get features for the state vector
        let state = match (
            UserFeatures::from_db(pool, user_id).await,
            UserFeatures::from_db(pool, candidate_id).await,
        ) {
            (Ok(uf), Ok(cf)) => combine_features(&uf, &cf),
            _ => vec![0.5; USER_FEATURE_DIM * 2], // Fallback: 28-dim neutral
        };

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

        // Train every 10 swipes
        self.train_counter += 1;
        if self.train_counter % 10 == 0 {
            self.rl_agent.train_batch(32);
        }
    }

    /// Get combined ML stats for the /ml/stats endpoint.
    pub fn ml_stats(&self) -> serde_json::Value {
        serde_json::json!({
            "rl_agent": self.rl_agent.stats(),
            "linucb": self.linucb.stats(),
            "federated": self.federated.stats(),
            "train_counter": self.train_counter,
            "feature_dim": USER_FEATURE_DIM,
            "state_dim": USER_FEATURE_DIM * 2,
        })
    }
}

impl Default for MlService {
    fn default() -> Self {
        Self::new()
    }
}
