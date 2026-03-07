/// Math utilities for ML computations.

/// Softmax over f64 slice — returns probability distribution.
pub fn softmax(logits: &[f64]) -> Vec<f64> {
    if logits.is_empty() {
        return vec![];
    }
    let max_val = logits.iter().cloned().fold(f64::NEG_INFINITY, f64::max);
    let exps: Vec<f64> = logits.iter().map(|x| (x - max_val).exp()).collect();
    let sum: f64 = exps.iter().sum();
    exps.iter().map(|e| e / sum).collect()
}

/// Laplace noise for differential privacy.
pub fn laplace_noise(scale: f64) -> f64 {
    use rand::Rng;
    let mut rng = rand::thread_rng();
    let u: f64 = rng.gen_range(-0.5..0.5);
    -scale * u.abs().ln() * u.signum()
}

/// Cosine similarity between two f64 slices.
pub fn cosine_similarity(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let mut dot = 0.0;
    let mut norm_a = 0.0;
    let mut norm_b = 0.0;
    for (x, y) in a.iter().zip(b.iter()) {
        dot += x * y;
        norm_a += x * x;
        norm_b += y * y;
    }
    let denom = norm_a.sqrt() * norm_b.sqrt();
    if denom == 0.0 { None } else { Some((dot / denom).clamp(-1.0, 1.0)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_softmax() {
        let result = softmax(&[1.0, 2.0, 3.0]);
        assert!((result.iter().sum::<f64>() - 1.0).abs() < 1e-6);
        assert!(result[2] > result[1] && result[1] > result[0]);
    }

    #[test]
    fn test_cosine_similarity() {
        let a = vec![1.0, 0.0, 0.0];
        let b = vec![0.0, 1.0, 0.0];
        assert!((cosine_similarity(&a, &b).unwrap() - 0.0).abs() < 1e-6);
        assert!((cosine_similarity(&a, &a).unwrap() - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_laplace_noise() {
        let noise = laplace_noise(0.1);
        assert!(noise.abs() < 100.0); // just verify it runs
    }
}
