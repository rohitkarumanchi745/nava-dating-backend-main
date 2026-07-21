//! CoreML-driven photo search + attestation verification.
//!
//! The device does all the ML: MobileCLIP (Core ML) embeds photos and queries,
//! and the on-device face check produces a verification attestation. The server
//! only stores the vectors, records verification, and runs pgvector ANN — no
//! server-side vision model, no OOM.

use axum::{extract::State, http::HeaderMap, Json};
use serde::Deserialize;
use serde_json::{json, Value};

use crate::{
    auth::{decode_access_token, extract_bearer_token},
    error::AppError,
    state::AppState,
};

/// MobileCLIP embedding dimension. Must match the app's bundled model.
const CLIP_DIM: usize = 512;

/// Format a float slice as a pgvector literal (`[1,2,3]`) to bind + cast `::vector`.
pub(crate) fn to_pgvector(v: &[f32]) -> String {
    let mut s = String::with_capacity(v.len() * 8);
    s.push('[');
    for (i, x) in v.iter().enumerate() {
        if i > 0 {
            s.push(',');
        }
        s.push_str(&x.to_string());
    }
    s.push(']');
    s
}

// ============================================================================
// Verification attestation (replaces server-side selfie vision)
// ============================================================================

#[derive(Deserialize)]
pub struct AttestationPayload {
    pub face_match_score: f32,
    pub liveness_score: f32,
    /// Apple App Attest assertion. PRODUCTION MUST verify this against Apple so a
    /// client can't just POST passing scores. Left as a seam for now.
    #[serde(default)]
    pub attestation: Option<String>,
}

/// POST /verify/attestation — record on-device face verification.
pub async fn verify_attestation(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AttestationPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    // Thresholds (tune to your model). NOTE: without verifying `attestation` via
    // Apple App Attest, these scores are client-asserted — harden before prod.
    const FACE_MATCH_MIN: f32 = 0.6;
    const LIVENESS_MIN: f32 = 0.5;
    if body.face_match_score < FACE_MATCH_MIN || body.liveness_score < LIVENESS_MIN {
        return Ok(Json(json!({
            "verified": false,
            "reason": "face-match or liveness below threshold"
        })));
    }

    sqlx::query("UPDATE users SET is_verified = TRUE, updated_at = NOW() WHERE id = $1")
        .bind(user_id as i64)
        .execute(&state.db)
        .await?;

    Ok(Json(json!({ "verified": true })))
}

// ============================================================================
// CLIP embedding ingestion (device uploads its own vector)
// ============================================================================

#[derive(Deserialize)]
pub struct ClipEmbeddingPayload {
    pub embedding: Vec<f32>,
    #[serde(default = "default_model")]
    pub model_version: String,
}

fn default_model() -> String {
    "mobileclip_s2".to_string()
}

/// POST /me/clip-embedding — store the caller's MobileCLIP image embedding.
pub async fn upsert_clip_embedding(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<ClipEmbeddingPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    if body.embedding.len() != CLIP_DIM {
        return Err(AppError::bad_request(format!(
            "embedding must be {} dimensions",
            CLIP_DIM
        )));
    }

    let vec_str = to_pgvector(&body.embedding);
    sqlx::query(
        "INSERT INTO user_clip_embeddings (user_id, embedding, model_version, updated_at) \
         VALUES ($1, $2::vector, $3, NOW()) \
         ON CONFLICT (user_id) DO UPDATE SET \
            embedding = EXCLUDED.embedding, model_version = EXCLUDED.model_version, updated_at = NOW()",
    )
    .bind(user_id as i64)
    .bind(&vec_str)
    .bind(&body.model_version)
    .execute(&state.db)
    .await?;

    Ok(Json(json!({ "stored": true })))
}

// ============================================================================
// Photo search (text query vector -> ANN over verified profiles)
// ============================================================================

#[derive(Deserialize)]
pub struct SearchPayload {
    /// Query embedding from the on-device MobileCLIP text encoder.
    pub embedding: Vec<f32>,
    #[serde(default)]
    pub limit: Option<i64>,
}

/// POST /search/photos — natural-language photo search over verified profiles.
/// Returns professional fields only; a match's full dating profile stays behind
/// the existing match-gated endpoint (the client fetches it when `is_match`).
pub async fn search_photos(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<SearchPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    if body.embedding.len() != CLIP_DIM {
        return Err(AppError::bad_request(format!(
            "embedding must be {} dimensions",
            CLIP_DIM
        )));
    }
    let limit = body.limit.unwrap_or(20).clamp(1, 50);
    let q = to_pgvector(&body.embedding);

    let rows = sqlx::query_as::<_, (i64, Option<String>, Option<String>, bool, f64)>(
        r#"
        SELECT u.id, u.name, u.profession_title,
               EXISTS(SELECT 1 FROM matches m
                      WHERE (m.user1_id = $1 AND m.user2_id = u.id)
                         OR (m.user1_id = u.id AND m.user2_id = $1)) AS is_match,
               (e.embedding <=> $2::vector)::float8 AS dist
        FROM user_clip_embeddings e
        JOIN users u ON u.id = e.user_id
        WHERE u.is_active = TRUE AND u.id <> $1
          -- Pool = verified profiles ∪ the viewer's own matches, so an
          -- unverified match still surfaces (they get the full dating profile
          -- via the match-gated path; unverified non-matches are excluded).
          AND (u.is_verified = TRUE
               OR EXISTS(SELECT 1 FROM matches m
                         WHERE (m.user1_id = $1 AND m.user2_id = u.id)
                            OR (m.user1_id = u.id AND m.user2_id = $1)))
          AND NOT EXISTS (
              SELECT 1 FROM swipes s
              WHERE s.from_user_id = $1 AND s.to_user_id = u.id AND s.action = 'block')
        ORDER BY e.embedding <=> $2::vector
        LIMIT $3
        "#,
    )
    .bind(user_id as i64)
    .bind(&q)
    .bind(limit)
    .fetch_all(state.read_pool())
    .await?;

    let results: Vec<Value> = rows
        .into_iter()
        .map(|(id, name, profession, is_match, dist)| {
            json!({
                "user_id": id,
                "name": name,
                "profession_title": profession,
                // true → client opens the full dating profile (match-gated);
                // false → client shows the professional profile only.
                "is_match": is_match,
                "similarity": 1.0 - dist, // pgvector cosine distance → similarity
            })
        })
        .collect();

    Ok(Json(json!({ "results": results })))
}
