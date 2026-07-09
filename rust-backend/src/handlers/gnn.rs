//! GNN worker endpoints: export interaction edges for training, and upsert the
//! trained embeddings. Admin-scoped — driven by `scripts/gnn_trainer.py`.

use axum::{
    extract::{Query, State},
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;

use crate::{auth::AdminClaims, error::AppError, state::AppState};

/// GET /admin/gnn/edges?since_id=&limit= — export positive interaction edges
/// (likes/superlikes) for the trainer to build the graph. Paginated by swipe id.
pub async fn export_edges(
    State(state): State<AppState>,
    _admin: AdminClaims,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let since = params.get("since_id").and_then(|v| v.parse::<i64>().ok()).unwrap_or(0);
    let limit = params
        .get("limit")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(5000)
        .clamp(1, 50000);

    let rows = sqlx::query_as::<_, (i64, i64, i64, String)>(
        "SELECT id, from_user_id, to_user_id, action FROM swipes \
         WHERE id > $1 AND action IN ('like','superlike') ORDER BY id LIMIT $2",
    )
    .bind(since)
    .bind(limit)
    .fetch_all(state.read_pool())
    .await?;

    let next_since = rows.last().map(|r| r.0).unwrap_or(since);
    let edges: Vec<Value> = rows
        .into_iter()
        .map(|(_, src, dst, action)| json!({ "src": src, "dst": dst, "type": action }))
        .collect();

    Ok(Json(json!({ "edges": edges, "next_since_id": next_since })))
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

/// POST /admin/gnn/embeddings — worker upserts a batch of trained embeddings.
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
            "INSERT INTO user_graph_embeddings (user_id, embedding, dim, model_version, updated_at) \
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
