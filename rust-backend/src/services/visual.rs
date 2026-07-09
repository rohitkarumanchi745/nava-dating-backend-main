//! Visual (photo) compatibility scoring.
//!
//! Per-user photo embeddings from the ImageNet-pretrained backbone in the vision
//! pipeline (`VisionAnalyzer::embed_image`). Served here as a cheap pairwise
//! cosine that fills `matches.visual_compatibility_score` and blends into the
//! reciprocal matcher — precomputed, so no image model runs at match time.
//!
//! Gracefully inert: users without a photo embedding return `None` and the
//! caller falls back to its base score.

use std::collections::HashMap;

use sqlx::PgPool;

/// Batch-load photo embeddings for a set of users.
pub async fn embeddings_for(pool: &PgPool, ids: &[i64]) -> HashMap<i64, Vec<f64>> {
    if ids.is_empty() {
        return HashMap::new();
    }
    sqlx::query_as::<_, (i64, Vec<f64>)>(
        "SELECT user_id, embedding FROM user_visual_embeddings WHERE user_id = ANY($1)",
    )
    .bind(ids)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .collect()
}

/// Cosine similarity mapped to [0,1]. `None` for empty/mismatched/zero vectors.
pub fn cosine01(a: &[f64], b: &[f64]) -> Option<f64> {
    if a.is_empty() || a.len() != b.len() {
        return None;
    }
    let mut dot = 0.0;
    let mut na = 0.0;
    let mut nb = 0.0;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = na.sqrt() * nb.sqrt();
    if denom <= 0.0 {
        return None;
    }
    Some(((dot / denom) + 1.0) / 2.0)
}

/// Pairwise visual-compatibility score in [0,1], or `None` if either user has
/// no photo embedding.
pub async fn score(pool: &PgPool, a: i64, b: i64) -> Option<f64> {
    let map = embeddings_for(pool, &[a, b]).await;
    let ea = map.get(&a)?;
    let eb = map.get(&b)?;
    cosine01(ea, eb)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cosine01_unit_interval() {
        assert!((cosine01(&[1.0, 0.0], &[1.0, 0.0]).unwrap() - 1.0).abs() < 1e-9);
        assert!((cosine01(&[1.0, 0.0], &[-1.0, 0.0]).unwrap() - 0.0).abs() < 1e-9);
        assert!((cosine01(&[1.0, 0.0], &[0.0, 1.0]).unwrap() - 0.5).abs() < 1e-9);
        assert!(cosine01(&[], &[]).is_none());
        assert!(cosine01(&[1.0], &[1.0, 2.0]).is_none());
    }
}
