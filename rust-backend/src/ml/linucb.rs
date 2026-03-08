use std::collections::HashMap;
use serde_json::Value;

/// LinUCB contextual bandit for candidate scoring.
/// Each "arm" is a candidate user; context is the 28-dim feature vector.

const FEATURE_DIM: usize = 28;

/// Per-arm state: A matrix (d×d) and b vector (d×1).
#[derive(Debug, Clone)]
pub struct ArmState {
    /// A = d×d identity + sum of outer products (stored flat, row-major)
    pub a_matrix: Vec<f64>,
    /// b = d×1 reward-weighted feature sum
    pub b_vector: Vec<f64>,
    pub num_pulls: u64,
    pub total_reward: f64,
}

impl ArmState {
    pub fn new(dim: usize) -> Self {
        // Initialize A as identity matrix
        let mut a_matrix = vec![0.0; dim * dim];
        for i in 0..dim {
            a_matrix[i * dim + i] = 1.0;
        }
        Self {
            a_matrix,
            b_vector: vec![0.0; dim],
            num_pulls: 0,
            total_reward: 0.0,
        }
    }

    /// Serialize A matrix and b vector to JSON for DB storage.
    pub fn to_json(&self) -> (Value, Value) {
        let a = serde_json::to_value(&self.a_matrix).unwrap_or(Value::Null);
        let b = serde_json::to_value(&self.b_vector).unwrap_or(Value::Null);
        (a, b)
    }

    /// Deserialize from JSON (loaded from `bandit_arm_stats` table).
    pub fn from_json(a_json: &Value, b_json: &Value, num_pulls: u64, total_reward: f64) -> Option<Self> {
        let a_matrix: Vec<f64> = serde_json::from_value(a_json.clone()).ok()?;
        let b_vector: Vec<f64> = serde_json::from_value(b_json.clone()).ok()?;
        if a_matrix.len() != FEATURE_DIM * FEATURE_DIM || b_vector.len() != FEATURE_DIM {
            return None;
        }
        Some(Self {
            a_matrix,
            b_vector,
            num_pulls,
            total_reward,
        })
    }
}

pub struct LinUcbBandit {
    /// Exploration parameter
    alpha: f64,
    /// Per-arm states keyed by arm_id (e.g., "user_123")
    arms: HashMap<String, ArmState>,
    /// Observation decay factor
    decay: f64,
    // --- Metrics ---
    cumulative_reward: f64,
}

impl LinUcbBandit {
    pub fn new() -> Self {
        Self {
            alpha: 0.6,
            arms: HashMap::new(),
            decay: 0.995,
            cumulative_reward: 0.0,
        }
    }

    /// Score a candidate arm given context features.
    /// Returns UCB = theta^T * x + alpha * sqrt(x^T * A^{-1} * x)
    pub fn score(&self, arm_id: &str, context: &[f64]) -> f64 {
        let arm = match self.arms.get(arm_id) {
            Some(a) => a,
            None => return self.alpha, // Unexplored arm gets exploration bonus
        };

        let d = FEATURE_DIM;
        if context.len() != d {
            return 0.0;
        }

        // Compute A^{-1} using simple Gauss-Jordan for small matrix
        let a_inv = match pseudo_inverse(&arm.a_matrix, d) {
            Some(inv) => inv,
            None => return 0.0,
        };

        // theta = A^{-1} * b
        let theta = mat_vec_mul(&a_inv, &arm.b_vector, d);

        // UCB = theta^T * x + alpha * sqrt(x^T * A^{-1} * x)
        let exploitation: f64 = theta.iter().zip(context.iter()).map(|(t, x)| t * x).sum();

        // x^T * A^{-1} * x
        let a_inv_x = mat_vec_mul(&a_inv, context, d);
        let exploration_var: f64 = context.iter().zip(a_inv_x.iter()).map(|(x, ax)| x * ax).sum();
        let exploration = self.alpha * exploration_var.max(0.0).sqrt();

        exploitation + exploration
    }

    /// Update arm after observing reward.
    pub fn update(&mut self, arm_id: &str, context: &[f64], reward: f64) {
        let d = FEATURE_DIM;
        if context.len() != d {
            return;
        }

        let arm = self.arms.entry(arm_id.to_string()).or_insert_with(|| ArmState::new(d));

        // Apply decay to existing observations
        for val in arm.a_matrix.iter_mut() {
            *val *= self.decay;
        }
        for val in arm.b_vector.iter_mut() {
            *val *= self.decay;
        }
        // Re-add identity after decay
        for i in 0..d {
            arm.a_matrix[i * d + i] += 1.0 - self.decay;
        }

        // A += x * x^T (outer product)
        for i in 0..d {
            for j in 0..d {
                arm.a_matrix[i * d + j] += context[i] * context[j];
            }
        }

        // b += reward * x
        for i in 0..d {
            arm.b_vector[i] += reward * context[i];
        }

        arm.num_pulls += 1;
        arm.total_reward += reward;
        self.cumulative_reward += reward;
    }

    /// Rank a list of candidate arm_ids by UCB score (descending).
    pub fn rank(&self, candidates: &[(String, Vec<f64>)]) -> Vec<(String, f64)> {
        let mut scored: Vec<(String, f64)> = candidates
            .iter()
            .map(|(id, ctx)| (id.clone(), self.score(id, ctx)))
            .collect();
        scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        scored
    }

    /// Load an arm state (from DB).
    pub fn load_arm(&mut self, arm_id: &str, state: ArmState) {
        self.arms.insert(arm_id.to_string(), state);
    }

    /// Get arm state for persistence.
    pub fn get_arm(&self, arm_id: &str) -> Option<&ArmState> {
        self.arms.get(arm_id)
    }

    /// Get all arm IDs (for checkpoint persistence).
    pub fn arm_ids(&self) -> Vec<String> {
        self.arms.keys().cloned().collect()
    }

    /// Stats for monitoring.
    pub fn stats(&self) -> LinUcbStats {
        LinUcbStats {
            alpha: self.alpha,
            num_arms: self.arms.len(),
            total_pulls: self.arms.values().map(|a| a.num_pulls).sum(),
            cumulative_reward: self.cumulative_reward,
        }
    }
}

impl Default for LinUcbBandit {
    fn default() -> Self {
        Self::new()
    }
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct LinUcbStats {
    pub alpha: f64,
    pub num_arms: usize,
    pub total_pulls: u64,
    pub cumulative_reward: f64,
}

/// Gauss-Jordan pseudo-inverse for small d×d matrix (flat, row-major).
fn pseudo_inverse(mat: &[f64], d: usize) -> Option<Vec<f64>> {
    let mut augmented = vec![0.0; d * 2 * d];
    // [A | I]
    for i in 0..d {
        for j in 0..d {
            augmented[i * 2 * d + j] = mat[i * d + j];
        }
        augmented[i * 2 * d + d + i] = 1.0;
    }

    for col in 0..d {
        // Find pivot
        let mut max_row = col;
        let mut max_val = augmented[col * 2 * d + col].abs();
        for row in (col + 1)..d {
            let val = augmented[row * 2 * d + col].abs();
            if val > max_val {
                max_val = val;
                max_row = row;
            }
        }
        if max_val < 1e-12 {
            return None; // Singular
        }

        // Swap rows
        if max_row != col {
            for j in 0..(2 * d) {
                let a = col * 2 * d + j;
                let b = max_row * 2 * d + j;
                augmented.swap(a, b);
            }
        }

        // Scale pivot row
        let pivot = augmented[col * 2 * d + col];
        for j in 0..(2 * d) {
            augmented[col * 2 * d + j] /= pivot;
        }

        // Eliminate column
        for row in 0..d {
            if row == col {
                continue;
            }
            let factor = augmented[row * 2 * d + col];
            for j in 0..(2 * d) {
                augmented[row * 2 * d + j] -= factor * augmented[col * 2 * d + j];
            }
        }
    }

    // Extract right half
    let mut result = vec![0.0; d * d];
    for i in 0..d {
        for j in 0..d {
            result[i * d + j] = augmented[i * 2 * d + d + j];
        }
    }
    Some(result)
}

/// Matrix-vector multiply: result = mat * vec (mat is d×d flat row-major).
fn mat_vec_mul(mat: &[f64], vec: &[f64], d: usize) -> Vec<f64> {
    (0..d)
        .map(|i| (0..d).map(|j| mat[i * d + j] * vec[j]).sum())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_linucb_basic() {
        let mut bandit = LinUcbBandit::new();
        let ctx = vec![0.5; FEATURE_DIM];

        // Unexplored arm should get exploration bonus
        let score = bandit.score("arm_1", &ctx);
        assert!(score > 0.0);

        // Update and re-score
        bandit.update("arm_1", &ctx, 1.0);
        let score2 = bandit.score("arm_1", &ctx);
        assert!(score2.is_finite());
    }

    #[test]
    fn test_linucb_ranking() {
        let mut bandit = LinUcbBandit::new();
        let ctx = vec![0.5; FEATURE_DIM];

        // Train arm_good with high rewards
        for _ in 0..10 {
            bandit.update("arm_good", &ctx, 1.0);
        }
        // Train arm_bad with low rewards
        for _ in 0..10 {
            bandit.update("arm_bad", &ctx, -0.5);
        }

        let ranked = bandit.rank(&[
            ("arm_good".into(), ctx.clone()),
            ("arm_bad".into(), ctx.clone()),
        ]);
        assert_eq!(ranked[0].0, "arm_good");
    }

    #[test]
    fn test_arm_state_json_roundtrip() {
        let arm = ArmState::new(FEATURE_DIM);
        let (a_json, b_json) = arm.to_json();
        let loaded = ArmState::from_json(&a_json, &b_json, 0, 0.0).unwrap();
        assert_eq!(loaded.a_matrix.len(), FEATURE_DIM * FEATURE_DIM);
        assert_eq!(loaded.b_vector.len(), FEATURE_DIM);
    }
}
