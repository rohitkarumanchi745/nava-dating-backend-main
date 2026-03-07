use serde_json::Value;

use super::math::laplace_noise;

/// FedAvg aggregation with differential privacy.
/// Performs actual weighted averaging of client model weights,
/// replacing the stub in handlers/mod.rs.

pub struct FederatedCoordinator {
    /// Global learning rate for aggregation
    pub global_lr: f64,
    /// Minimum clients required to aggregate
    pub min_clients: usize,
    /// Differential privacy noise scale
    pub dp_noise_scale: f64,
    /// Whether DP is enabled
    pub dp_enabled: bool,
}

impl FederatedCoordinator {
    pub fn new() -> Self {
        Self {
            global_lr: 0.1,
            min_clients: 2,
            dp_noise_scale: 0.1,
            dp_enabled: true,
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
}

impl Default for FederatedCoordinator {
    fn default() -> Self {
        Self::new()
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
}
