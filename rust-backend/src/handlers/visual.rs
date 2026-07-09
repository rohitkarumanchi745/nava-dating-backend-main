//! Visual embedding worker endpoints.
//!
//! `recompute` produces embeddings entirely in Rust: it fetches each user's
//! profile photo and runs the ImageNet-pretrained backbone in the existing
//! vision pipeline (`VisionAnalyzer::embed_image`) — no Python. `upsert` is an
//! alternative write path for an external producer. Both are admin-scoped.

use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::{auth::AdminClaims, error::AppError, state::AppState};

/// POST /admin/visual/recompute?limit= — compute + store visual embeddings for
/// users who have a profile photo but no embedding yet. Best-effort: skips any
/// photo it can't fetch/decode, and no-ops if no embedding model is configured.
pub async fn recompute_embeddings(
    State(state): State<AppState>,
    _admin: AdminClaims,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(100)
        .clamp(1, 1000);

    let vision = state
        .vision
        .clone()
        .ok_or_else(|| AppError::service_unavailable("Vision pipeline not enabled"))?;

    let rows = sqlx::query_as::<_, (i64, String)>(
        "SELECT u.id, u.profile_photo_url FROM users u \
         WHERE u.profile_photo_url IS NOT NULL AND u.profile_photo_url <> '' \
           AND NOT EXISTS (SELECT 1 FROM user_visual_embeddings e WHERE e.user_id = u.id) \
         LIMIT $1",
    )
    .bind(limit)
    .fetch_all(state.read_pool())
    .await?;

    let client = reqwest::Client::new();
    let mut processed = 0i64;

    for (uid, url) in rows {
        if !url.starts_with("http") {
            continue;
        }
        let bytes = match client.get(&url).send().await {
            Ok(resp) => match resp.bytes().await {
                Ok(b) => b,
                Err(_) => continue,
            },
            Err(_) => continue,
        };
        let image = match image::load_from_memory(&bytes) {
            Ok(img) => img,
            Err(_) => continue,
        };
        let embedding = {
            let analyzer = vision.lock().await;
            analyzer.embed_image(&image)
        };
        let Some(embedding) = embedding else {
            // No embedding model configured — nothing to do for anyone.
            break;
        };

        let emb64: Vec<f64> = embedding.iter().map(|x| *x as f64).collect();
        let dim = emb64.len() as i32;
        let _ = sqlx::query(
            "INSERT INTO user_visual_embeddings (user_id, embedding, dim, model_version, updated_at) \
             VALUES ($1,$2,$3,1,NOW()) \
             ON CONFLICT (user_id) DO UPDATE SET \
                embedding = EXCLUDED.embedding, dim = EXCLUDED.dim, updated_at = NOW()",
        )
        .bind(uid)
        .bind(&emb64)
        .bind(dim)
        .execute(&state.db)
        .await;
        processed += 1;
    }

    Ok(Json(json!({ "processed": processed })))
}

#[derive(Deserialize)]
pub struct EmbeddingItem {
    pub user_id: i64,
    pub embedding: Vec<f64>,
}

#[derive(Deserialize)]
pub struct UpsertEmbeddingsPayload {
    pub model_version: i32,
    pub embeddings: Vec<EmbeddingItem>,
}

/// POST /admin/visual/embeddings — external producer upserts a batch.
pub async fn upsert_embeddings(
    State(state): State<AppState>,
    _admin: AdminClaims,
    Json(body): Json<UpsertEmbeddingsPayload>,
) -> Result<Json<Value>, AppError> {
    let mut upserted = 0i64;
    for item in &body.embeddings {
        if item.embedding.is_empty() {
            continue;
        }
        let dim = item.embedding.len() as i32;
        sqlx::query(
            "INSERT INTO user_visual_embeddings (user_id, embedding, dim, model_version, updated_at) \
             VALUES ($1,$2,$3,$4,NOW()) \
             ON CONFLICT (user_id) DO UPDATE SET \
                embedding = EXCLUDED.embedding, dim = EXCLUDED.dim, \
                model_version = EXCLUDED.model_version, updated_at = NOW()",
        )
        .bind(item.user_id)
        .bind(&item.embedding)
        .bind(dim)
        .bind(body.model_version)
        .execute(&state.db)
        .await?;
        upserted += 1;
    }
    Ok(Json(json!({ "upserted": upserted, "model_version": body.model_version })))
}
