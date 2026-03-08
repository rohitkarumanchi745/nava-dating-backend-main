use std::collections::{HashMap, VecDeque};
use std::time::Instant;
use rand::Rng;

/// Q-learning RL agent for discovery ranking.
/// State = 28-dim (14 user features + 14 candidate features).
/// Actions: 0 = pass, 1 = like.

const STATE_DIM: usize = 28;
const NUM_ACTIONS: usize = 2;
const REPLAY_BUFFER_MAX: usize = 10_000;
const METRICS_WINDOW: usize = 100;

#[derive(Debug, Clone)]
pub struct SwipeExperience {
    pub user_id: i32,
    pub candidate_id: i32,
    pub state: Vec<f64>,
    pub action: usize,   // 0=pass, 1=like
    pub reward: f64,
}

/// Per-user personalized Q-model weights.
#[derive(Debug, Clone)]
struct UserModel {
    weights: Vec<Vec<f64>>, // [NUM_ACTIONS][STATE_DIM]
    interaction_count: u64,
}

impl UserModel {
    fn new() -> Self {
        Self {
            weights: vec![vec![0.0; STATE_DIM]; NUM_ACTIONS],
            interaction_count: 0,
        }
    }
}

pub struct RlAgent {
    /// Global Q-weights: [NUM_ACTIONS][STATE_DIM]
    global_weights: Vec<Vec<f64>>,
    /// Per-user models for personalization
    user_models: HashMap<i32, UserModel>,
    /// Experience replay buffer
    replay_buffer: Vec<SwipeExperience>,
    /// Exploration rate
    pub epsilon: f64,
    epsilon_min: f64,
    epsilon_decay: f64,
    /// Learning rate
    learning_rate: f64,
    /// Discount factor
    gamma: f64,
    /// Global/personal blend ratio (0.7 = 70% global)
    blend_ratio: f64,
    // --- Metrics ---
    cumulative_reward: f64,
    action_counts: [u64; NUM_ACTIONS],
    reward_history: VecDeque<f64>,
    scoring_latency_us: VecDeque<u128>,
}

impl RlAgent {
    pub fn new() -> Self {
        let mut rng = rand::thread_rng();
        let global_weights: Vec<Vec<f64>> = (0..NUM_ACTIONS)
            .map(|_| (0..STATE_DIM).map(|_| rng.gen_range(-0.1..0.1)).collect())
            .collect();

        Self {
            global_weights,
            user_models: HashMap::new(),
            replay_buffer: Vec::new(),
            epsilon: 0.3,
            epsilon_min: 0.01,
            epsilon_decay: 0.995,
            learning_rate: 0.01,
            gamma: 0.95,
            blend_ratio: 0.7,
            cumulative_reward: 0.0,
            action_counts: [0; NUM_ACTIONS],
            reward_history: VecDeque::with_capacity(METRICS_WINDOW),
            scoring_latency_us: VecDeque::with_capacity(METRICS_WINDOW),
        }
    }

    /// Score a candidate for a user (higher = more likely to be liked).
    /// Returns Q-value for the "like" action.
    pub fn score_candidate(&mut self, user_id: i32, state: &[f64]) -> f64 {
        let start = Instant::now();
        let q_values = self.q_values(user_id, state);
        let elapsed = start.elapsed().as_micros();
        if self.scoring_latency_us.len() >= METRICS_WINDOW {
            self.scoring_latency_us.pop_front();
        }
        self.scoring_latency_us.push_back(elapsed);
        q_values[1] // Q-value for "like" action
    }

    /// Select action using epsilon-greedy policy.
    pub fn select_action(&self, user_id: i32, state: &[f64]) -> usize {
        let mut rng = rand::thread_rng();
        if rng.gen::<f64>() < self.epsilon {
            rng.gen_range(0..NUM_ACTIONS)
        } else {
            let q = self.q_values(user_id, state);
            if q[1] > q[0] { 1 } else { 0 }
        }
    }

    /// Compute Q-values blending global + per-user weights.
    fn q_values(&self, user_id: i32, state: &[f64]) -> Vec<f64> {
        let global_q: Vec<f64> = self
            .global_weights
            .iter()
            .map(|w| dot(w, state))
            .collect();

        if let Some(user_model) = self.user_models.get(&user_id) {
            if user_model.interaction_count >= 5 {
                let personal_q: Vec<f64> = user_model
                    .weights
                    .iter()
                    .map(|w| dot(w, state))
                    .collect();
                // Blend: 70% global + 30% personal
                return global_q
                    .iter()
                    .zip(personal_q.iter())
                    .map(|(g, p)| self.blend_ratio * g + (1.0 - self.blend_ratio) * p)
                    .collect();
            }
        }
        global_q
    }

    /// Record a swipe experience and add to replay buffer.
    pub fn record_experience(&mut self, exp: SwipeExperience) {
        let user_id = exp.user_id;
        // Track metrics
        self.cumulative_reward += exp.reward;
        if exp.action < NUM_ACTIONS {
            self.action_counts[exp.action] += 1;
        }
        if self.reward_history.len() >= METRICS_WINDOW {
            self.reward_history.pop_front();
        }
        self.reward_history.push_back(exp.reward);

        self.replay_buffer.push(exp);
        if self.replay_buffer.len() > REPLAY_BUFFER_MAX {
            self.replay_buffer.remove(0);
        }

        // Initialize user model if needed
        self.user_models.entry(user_id).or_insert_with(UserModel::new);
        if let Some(model) = self.user_models.get_mut(&user_id) {
            model.interaction_count += 1;
        }
    }

    /// Train on a batch from the replay buffer.
    pub fn train_batch(&mut self, batch_size: usize) {
        if self.replay_buffer.is_empty() {
            return;
        }

        let mut rng = rand::thread_rng();
        let n = self.replay_buffer.len();
        let actual_batch = batch_size.min(n);

        for _ in 0..actual_batch {
            let idx = rng.gen_range(0..n);
            let exp = self.replay_buffer[idx].clone();

            // Q-learning update: Q(s,a) += lr * (reward - Q(s,a))
            let current_q = dot(&self.global_weights[exp.action], &exp.state);
            let td_error = exp.reward - current_q;

            // Update global weights
            for j in 0..STATE_DIM.min(exp.state.len()) {
                self.global_weights[exp.action][j] +=
                    self.learning_rate * td_error * exp.state[j];
            }

            // Update per-user weights
            if let Some(model) = self.user_models.get_mut(&exp.user_id) {
                for j in 0..STATE_DIM.min(exp.state.len()) {
                    model.weights[exp.action][j] +=
                        self.learning_rate * 0.5 * td_error * exp.state[j];
                }
            }
        }

        // Decay epsilon
        self.epsilon = (self.epsilon * self.epsilon_decay).max(self.epsilon_min);
    }

    /// Get stats for monitoring.
    pub fn stats(&self) -> RlStats {
        let avg_reward_100 = if self.reward_history.is_empty() {
            0.0
        } else {
            self.reward_history.iter().sum::<f64>() / self.reward_history.len() as f64
        };
        let avg_scoring_latency_us = if self.scoring_latency_us.is_empty() {
            0.0
        } else {
            self.scoring_latency_us.iter().sum::<u128>() as f64
                / self.scoring_latency_us.len() as f64
        };
        RlStats {
            epsilon: self.epsilon,
            replay_buffer_size: self.replay_buffer.len(),
            user_model_count: self.user_models.len(),
            total_experiences: self.replay_buffer.len(),
            cumulative_reward: self.cumulative_reward,
            action_counts: self.action_counts,
            avg_reward_100,
            avg_scoring_latency_us,
        }
    }

    /// Serialize weights for checkpointing.
    pub fn to_checkpoint(&self) -> RlCheckpoint {
        RlCheckpoint {
            global_weights: self.global_weights.clone(),
            epsilon: self.epsilon,
        }
    }

    /// Restore from checkpoint.
    pub fn from_checkpoint(ckpt: RlCheckpoint) -> Self {
        let mut agent = Self::new();
        agent.global_weights = ckpt.global_weights;
        agent.epsilon = ckpt.epsilon;
        agent
    }
}

impl Default for RlAgent {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RlCheckpoint {
    pub global_weights: Vec<Vec<f64>>,
    pub epsilon: f64,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct RlStats {
    pub epsilon: f64,
    pub replay_buffer_size: usize,
    pub user_model_count: usize,
    pub total_experiences: usize,
    pub cumulative_reward: f64,
    pub action_counts: [u64; 2],
    pub avg_reward_100: f64,
    pub avg_scoring_latency_us: f64,
}

/// Dot product of two slices.
fn dot(a: &[f64], b: &[f64]) -> f64 {
    a.iter().zip(b.iter()).map(|(x, y)| x * y).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rl_agent_basic() {
        let mut agent = RlAgent::new();
        let state = vec![0.5; STATE_DIM];
        let score = agent.score_candidate(1, &state);
        assert!(score.is_finite());

        agent.record_experience(SwipeExperience {
            user_id: 1,
            candidate_id: 2,
            state: state.clone(),
            action: 1,
            reward: 1.0,
        });
        agent.train_batch(1);
        assert!(agent.epsilon < 0.3);
        assert_eq!(agent.stats().action_counts[1], 1);
        assert!(agent.stats().cumulative_reward > 0.0);
    }

    #[test]
    fn test_rl_agent_per_user() {
        let mut agent = RlAgent::new();
        let state = vec![0.5; STATE_DIM];
        // Record 5+ experiences to activate per-user model
        for i in 0..6 {
            agent.record_experience(SwipeExperience {
                user_id: 42,
                candidate_id: i + 100,
                state: state.clone(),
                action: 1,
                reward: 1.0,
            });
        }
        agent.train_batch(6);
        let score = agent.score_candidate(42, &state);
        assert!(score.is_finite());
    }

    #[test]
    fn test_rl_checkpoint_roundtrip() {
        let mut agent = RlAgent::new();
        let state = vec![0.5; STATE_DIM];
        agent.record_experience(SwipeExperience {
            user_id: 1, candidate_id: 2, state, action: 1, reward: 1.0,
        });
        agent.train_batch(1);
        let ckpt = agent.to_checkpoint();
        let restored = RlAgent::from_checkpoint(ckpt);
        assert_eq!(restored.epsilon, agent.epsilon);
        assert_eq!(restored.global_weights.len(), NUM_ACTIONS);
    }
}
