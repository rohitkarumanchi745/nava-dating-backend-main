//! Agentic auto-match endpoints: surface proposals, accept/decline (which feeds
//! the RL model), and an admin trigger for a matchmaking round.

use axum::{
    extract::{Query, State},
    http::HeaderMap,
    Json,
};
use serde::Deserialize;
use serde_json::{json, Value};
use std::collections::HashMap;
use uuid::Uuid;

use crate::{
    auth::{decode_access_token, extract_bearer_token, AdminClaims},
    error::AppError,
    services::matchmaker::{run_round, MatchmakerConfig},
    state::AppState,
};

/// GET /matches/auto/suggestions — the user's pending AI proposals.
pub async fn get_auto_suggestions(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let rows = sqlx::query_as::<_, (i32, i64, f64, Option<String>, Option<String>)>(
        r#"
        SELECT a.id, a.candidate_id, a.mutual_score, u.name, u.profile_photo_url
        FROM auto_match_suggestions a
        JOIN users u ON u.id = a.candidate_id
        WHERE a.user_id = $1 AND a.status = 'pending'
        ORDER BY a.mutual_score DESC
        LIMIT 20
        "#,
    )
    .bind(user_id as i64)
    .fetch_all(state.read_pool())
    .await?;

    let suggestions: Vec<Value> = rows
        .into_iter()
        .map(|(id, candidate_id, score, name, photo)| {
            json!({
                "suggestion_id": id,
                "candidate_id": candidate_id,
                "score": score,
                "name": name,
                "photo": photo,
            })
        })
        .collect();

    Ok(Json(json!({ "suggestions": suggestions })))
}

#[derive(Deserialize)]
pub struct RespondPayload {
    pub suggestion_id: i32,
}

/// POST /matches/auto/accept — accept a proposal: create the match and reward
/// the model for a correct prediction.
pub async fn accept_auto_match(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RespondPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let (candidate_id,) = sqlx::query_as::<_, (i64,)>(
        "SELECT candidate_id FROM auto_match_suggestions \
         WHERE id = $1 AND user_id = $2 AND status = 'pending'",
    )
    .bind(body.suggestion_id)
    .bind(user_id as i64)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("Suggestion not found or already handled"))?;

    // Create the match (canonical ordering to respect the unique index).
    let a = user_id as i64;
    let (u1, u2) = if a < candidate_id { (a, candidate_id) } else { (candidate_id, a) };
    let match_id = Uuid::new_v4().to_string();
    sqlx::query(
        r#"
        INSERT INTO matches
            (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match,
             match_reason, status, can_send_text)
        VALUES ($1,$2,$3,TRUE,TRUE,TRUE,'ai_auto','active',TRUE)
        ON CONFLICT (user1_id, user2_id) DO NOTHING
        "#,
    )
    .bind(&match_id).bind(u1).bind(u2)
    .execute(&state.db)
    .await?;

    // If a match already existed, use its id.
    let final_match_id: String = sqlx::query_scalar::<_, String>(
        "SELECT id FROM matches WHERE user1_id = $1 AND user2_id = $2",
    )
    .bind(u1).bind(u2)
    .fetch_optional(&state.db)
    .await?
    .unwrap_or(match_id);

    sqlx::query(
        "UPDATE auto_match_suggestions SET status = 'accepted', match_id = $2, responded_at = NOW() WHERE id = $1",
    )
    .bind(body.suggestion_id)
    .bind(&final_match_id)
    .execute(&state.db)
    .await?;

    // Reward: the user liked the AI's pick → positive signal for user→candidate.
    {
        let mut ml = state.ml.write().await;
        ml.record_swipe_weighted(&state.db, user_id, candidate_id as i32, true, false).await;
    }

    crate::handlers::publish_user_event(
        &state, candidate_id as i32, "auto_matched",
        json!({ "match_id": final_match_id, "candidate_id": user_id }),
    ).await;

    Ok(Json(json!({ "matched": true, "match_id": final_match_id })))
}

/// POST /matches/auto/decline — decline a proposal: penalize the prediction.
pub async fn decline_auto_match(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<RespondPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let (candidate_id,) = sqlx::query_as::<_, (i64,)>(
        "SELECT candidate_id FROM auto_match_suggestions \
         WHERE id = $1 AND user_id = $2 AND status = 'pending'",
    )
    .bind(body.suggestion_id)
    .bind(user_id as i64)
    .fetch_optional(&state.db)
    .await?
    .ok_or_else(|| AppError::not_found("Suggestion not found or already handled"))?;

    sqlx::query(
        "UPDATE auto_match_suggestions SET status = 'declined', responded_at = NOW() WHERE id = $1",
    )
    .bind(body.suggestion_id)
    .execute(&state.db)
    .await?;

    // Negative signal: the AI's prediction was wrong for user→candidate.
    {
        let mut ml = state.ml.write().await;
        ml.record_swipe_weighted(&state.db, user_id, candidate_id as i32, false, false).await;
    }

    Ok(Json(json!({ "declined": true })))
}

#[derive(Deserialize)]
pub struct AgentPromptPayload {
    /// Natural-language match intent, or pass structured `filters` directly.
    pub intent: Option<String>,
    pub filters: Option<crate::services::matchmaker::AgentFilters>,
}

/// POST /agent/matchmaker/prompt — the prompt-driven matchmaker agent.
/// Parses the NL intent into governed filters (in Rust) and returns
/// reciprocally-scored candidates, scoped to the authenticated user.
pub async fn agent_matchmaker_prompt(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(body): Json<AgentPromptPayload>,
) -> Result<Json<Value>, AppError> {
    let token = extract_bearer_token(&headers)?;
    let user_id = decode_access_token(&token, &state.config.secret_key)?;

    let filters = match (body.filters, body.intent.as_deref()) {
        (Some(f), _) => f,
        (None, Some(intent)) => crate::services::matchmaker::parse_intent(intent),
        (None, None) => return Err(AppError::bad_request("Provide `intent` or `filters`")),
    };

    let candidates = crate::services::matchmaker::agent_query(&state, user_id, &filters).await;

    Ok(Json(json!({
        "filters": filters,
        "candidates": candidates,
    })))
}

/// POST /admin/matches/auto/run — trigger a matchmaking round.
pub async fn admin_run_matchmaker(
    State(state): State<AppState>,
    _admin: AdminClaims,
    Query(params): Query<HashMap<String, String>>,
) -> Result<Json<Value>, AppError> {
    let user_limit = params
        .get("limit")
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(200);

    let cfg = MatchmakerConfig::from_env();
    let stats = run_round(&state, &cfg, user_limit).await;

    Ok(Json(json!({ "ok": true, "stats": stats })))
}
