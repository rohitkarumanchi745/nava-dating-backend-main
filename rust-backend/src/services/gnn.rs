//! Graph neural network (GNN) user embeddings.
//!
//! Trained offline (`scripts/gnn_trainer.py`) on the interaction graph, these
//! embeddings capture higher-order structure (multi-hop, community) that the
//! first-order `CoLikeMatrix` CF can't. Served here as a cheap pairwise score
//! that blends into the reciprocal matcher — precomputed, so zero model compute
//! at request time.
//!
//! Gracefully inert: if a user has no embedding (cold start, or the trainer
//! hasn't run), `score` returns `None` and the caller falls back to its base
//! score.

use std::collections::HashMap;

use sqlx::PgPool;

/// Batch-load embeddings for a set of users in one query.
pub async fn embeddings_for(pool: &PgPool, ids: &[i64]) -> HashMap<i64, Vec<f64>> {
    if ids.is_empty() {
        return HashMap::new();
    }
    sqlx::query_as::<_, (i64, Vec<f64>)>(
        "SELECT user_id, embedding FROM user_graph_embeddings WHERE user_id = ANY($1)",
    )
    .bind(ids)
    .fetch_all(pool)
    .await
    .unwrap_or_default()
    .into_iter()
    .collect()
}

/// Cosine similarity mapped to [0,1] so it blends with our other [0,1] scores.
/// Returns `None` for empty/mismatched/zero vectors.
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

/// Pairwise GNN score in [0,1], or `None` if either embedding is missing.
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
    fn cosine01_maps_to_unit_interval() {
        // identical -> 1.0, opposite -> 0.0, orthogonal -> 0.5
        assert!((cosine01(&[1.0, 0.0], &[1.0, 0.0]).unwrap() - 1.0).abs() < 1e-9);
        assert!((cosine01(&[1.0, 0.0], &[-1.0, 0.0]).unwrap() - 0.0).abs() < 1e-9);
        assert!((cosine01(&[1.0, 0.0], &[0.0, 1.0]).unwrap() - 0.5).abs() < 1e-9);
    }

    #[test]
    fn cosine01_rejects_degenerate() {
        assert!(cosine01(&[], &[]).is_none());
        assert!(cosine01(&[1.0, 2.0], &[1.0]).is_none());
        assert!(cosine01(&[0.0, 0.0], &[1.0, 1.0]).is_none());
    }
}
