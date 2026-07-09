//! Agentic auto-matcher.
//!
//! Reuses the existing per-user preference model (`MlService::rank_candidates`,
//! learned from swipes via `record_swipe_weighted`) to score BOTH directions of
//! a pair and combine them into a reciprocal mutual-match score. High-scoring
//! pairs become proposals (default) or, above a higher bar and with safety gates,
//! instant matches — no swiping. Accept/decline is fed back to the model.
//!
//! This is the "agent": an autonomous policy over the existing ML, not a new LLM.
//! Matching is a bandit/RL problem — cheap per-user preference state, not a
//! per-user network — which is the cost-efficient choice here.

use std::env;

use serde_json::json;
use uuid::Uuid;

use crate::state::AppState;

#[derive(Clone, Debug)]
pub struct MatchmakerConfig {
    pub enabled: bool,
    /// Mutual score to create a pending proposal.
    pub propose_threshold: f64,
    /// Mutual score to instantly create an active match (no user confirmation).
    pub auto_confirm_threshold: f64,
    /// Require both users verified before an instant (auto-confirmed) match.
    pub require_verified_for_auto: bool,
    /// Candidate pool size fetched per user.
    pub candidates_per_user: i64,
    /// Only reverse-score the user's top-K forward candidates (prunes cost).
    pub top_k_reciprocal: usize,
    /// Max new proposals per user per round.
    pub max_proposals_per_user: i64,
    /// Weight of the GNN graph-structure score blended into the reciprocal
    /// score, in [0,1]. 0 = off (default; no GNN lookups happen). This flag is
    /// actually checked — blending only runs when > 0.
    pub gnn_weight: f64,
}

impl Default for MatchmakerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            propose_threshold: 0.72,
            auto_confirm_threshold: 0.88,
            require_verified_for_auto: true,
            candidates_per_user: 80,
            top_k_reciprocal: 20,
            max_proposals_per_user: 5,
            gnn_weight: 0.0,
        }
    }
}

impl MatchmakerConfig {
    pub fn from_env() -> Self {
        let d = Self::default();
        let b = |k: &str, dv: bool| env::var(k).ok().map(|v| matches!(v.as_str(), "1"|"true"|"yes"|"on")).unwrap_or(dv);
        let f = |k: &str, dv: f64| env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(dv);
        let i = |k: &str, dv: i64| env::var(k).ok().and_then(|v| v.parse().ok()).unwrap_or(dv);
        Self {
            enabled: b("AUTO_MATCH_ENABLED", d.enabled),
            propose_threshold: f("AUTO_MATCH_PROPOSE_THRESHOLD", d.propose_threshold),
            auto_confirm_threshold: f("AUTO_MATCH_CONFIRM_THRESHOLD", d.auto_confirm_threshold),
            require_verified_for_auto: b("AUTO_MATCH_REQUIRE_VERIFIED", d.require_verified_for_auto),
            candidates_per_user: i("AUTO_MATCH_CANDIDATES_PER_USER", d.candidates_per_user),
            top_k_reciprocal: i("AUTO_MATCH_TOP_K", d.top_k_reciprocal as i64) as usize,
            max_proposals_per_user: i("AUTO_MATCH_MAX_PROPOSALS", d.max_proposals_per_user),
            gnn_weight: f("GNN_SCORE_WEIGHT", d.gnn_weight).clamp(0.0, 1.0),
        }
    }
}

/// Blend a GNN graph-structure score into a base reciprocal score.
/// No-op (and no DB lookup) when `weight <= 0` or the pair has no embeddings.
async fn blend_gnn(state: &AppState, a: i64, b: i64, base: f64, weight: f64) -> f64 {
    if weight <= 0.0 {
        return base;
    }
    match crate::services::gnn::score(&state.db, a, b).await {
        Some(g) => (1.0 - weight) * base + weight * g,
        None => base,
    }
}

#[derive(Default, Debug, serde::Serialize)]
pub struct RoundStats {
    pub users_processed: i64,
    pub pairs_scored: i64,
    pub proposals_created: i64,
    pub auto_matched: i64,
}

/// Combine the two one-sided scores into a reciprocal score. Geometric mean
/// penalizes one-sided interest (a great A→B but weak B→A stays low).
fn reciprocal(forward: f64, reverse: f64) -> f64 {
    (forward.clamp(0.0, 1.0) * reverse.clamp(0.0, 1.0)).sqrt()
}

/// Score how much `user` would like `candidate` using the shared per-user model.
async fn forward_scores(state: &AppState, user_id: i32, candidate_ids: &[i32]) -> Vec<(i32, f64)> {
    if candidate_ids.is_empty() { return Vec::new(); }
    let mut ml = state.ml.write().await;
    ml.rank_candidates(&state.db, user_id, candidate_ids).await
}

/// Candidate pool for a user: active, complete, not already swiped/matched/
/// proposed. Ordered by recent activity for freshness.
async fn candidates_for(state: &AppState, user_id: i64, limit: i64) -> Vec<i64> {
    sqlx::query_scalar::<_, i64>(
        r#"
        SELECT u.id FROM users u
        WHERE u.id <> $1
          AND u.is_active = TRUE
          AND u.is_profile_complete = TRUE
          AND NOT EXISTS (SELECT 1 FROM swipes s WHERE s.from_user_id = $1 AND s.to_user_id = u.id)
          AND NOT EXISTS (
              SELECT 1 FROM matches m
              WHERE (m.user1_id = $1 AND m.user2_id = u.id)
                 OR (m.user1_id = u.id AND m.user2_id = $1))
          AND NOT EXISTS (
              SELECT 1 FROM auto_match_suggestions a
              WHERE a.user_id = $1 AND a.candidate_id = u.id
                AND a.status IN ('pending','accepted','auto_matched'))
        ORDER BY u.last_active DESC NULLS LAST
        LIMIT $2
        "#,
    )
    .bind(user_id)
    .bind(limit)
    .fetch_all(state.read_pool())
    .await
    .unwrap_or_default()
}

/// Run one matchmaking round over up to `user_limit` recently-active users.
pub async fn run_round(state: &AppState, cfg: &MatchmakerConfig, user_limit: i64) -> RoundStats {
    let mut stats = RoundStats::default();

    let users = sqlx::query_scalar::<_, i64>(
        "SELECT id FROM users WHERE is_active = TRUE AND is_profile_complete = TRUE \
         ORDER BY last_active DESC NULLS LAST LIMIT $1",
    )
    .bind(user_limit)
    .fetch_all(state.read_pool())
    .await
    .unwrap_or_default();

    for a_i64 in users {
        stats.users_processed += 1;
        let a = a_i64 as i32;

        let candidates = candidates_for(state, a_i64, cfg.candidates_per_user).await;
        if candidates.is_empty() { continue; }
        let cand_i32: Vec<i32> = candidates.iter().map(|c| *c as i32).collect();

        // A's preferences over the whole pool (one model call), keep the top-K.
        let mut forward = forward_scores(state, a, &cand_i32).await;
        forward.sort_by(|x, y| y.1.partial_cmp(&x.1).unwrap_or(std::cmp::Ordering::Equal));
        forward.truncate(cfg.top_k_reciprocal);

        let mut created_this_user = 0i64;
        for (b, fscore) in forward {
            if created_this_user >= cfg.max_proposals_per_user { break; }
            stats.pairs_scored += 1;

            // Reverse direction: does B like A?
            let rscore = forward_scores(state, b, &[a]).await
                .first().map(|x| x.1).unwrap_or(0.0);
            let base = reciprocal(fscore, rscore);
            // Blend in higher-order graph structure (no-op unless GNN_SCORE_WEIGHT > 0).
            let mutual = blend_gnn(state, a_i64, b as i64, base, cfg.gnn_weight).await;
            if mutual < cfg.propose_threshold { continue; }

            let auto = mutual >= cfg.auto_confirm_threshold
                && (!cfg.require_verified_for_auto || both_verified(state, a_i64, b as i64).await);

            if auto {
                if create_auto_match(state, a_i64, b as i64, mutual, fscore, rscore).await.is_some() {
                    stats.auto_matched += 1;
                    created_this_user += 1;
                }
            } else if insert_proposal(state, a_i64, b as i64, mutual, fscore, rscore).await {
                stats.proposals_created += 1;
                created_this_user += 1;
                notify_user(state, a_i64, "auto_match_suggestion", json!({ "candidate_id": b, "score": mutual })).await;
            }
        }
    }

    stats
}

async fn both_verified(state: &AppState, a: i64, b: i64) -> bool {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM users WHERE id IN ($1, $2) AND is_verified = TRUE",
    )
    .bind(a).bind(b)
    .fetch_one(state.read_pool())
    .await
    .unwrap_or(0) == 2
}

async fn insert_proposal(state: &AppState, user_id: i64, candidate_id: i64, mutual: f64, fwd: f64, rev: f64) -> bool {
    sqlx::query(
        "INSERT INTO auto_match_suggestions (user_id, candidate_id, mutual_score, forward_score, reverse_score) \
         VALUES ($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING",
    )
    .bind(user_id).bind(candidate_id).bind(mutual).bind(fwd).bind(rev)
    .execute(&state.db)
    .await
    .map(|r| r.rows_affected() > 0)
    .unwrap_or(false)
}

/// Instantly create an active, mutual match (canonical user1 < user2 to respect
/// the unique index) and record it as an accepted auto-suggestion for both.
pub async fn create_auto_match(state: &AppState, a: i64, b: i64, score: f64, fwd: f64, rev: f64) -> Option<String> {
    let (u1, u2) = if a < b { (a, b) } else { (b, a) };
    let match_id = Uuid::new_v4().to_string();

    let inserted = sqlx::query(
        r#"
        INSERT INTO matches
            (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match,
             ai_compatibility_score, match_reason, status, can_send_text)
        VALUES ($1,$2,$3,TRUE,TRUE,TRUE,$4,'ai_auto','active',TRUE)
        ON CONFLICT (user1_id, user2_id) DO NOTHING
        "#,
    )
    .bind(&match_id).bind(u1).bind(u2).bind(score)
    .execute(&state.db)
    .await
    .map(|r| r.rows_affected() > 0)
    .unwrap_or(false);

    if !inserted { return None; }

    // Record the auto-match as an accepted suggestion for the initiating side.
    let _ = sqlx::query(
        "INSERT INTO auto_match_suggestions \
            (user_id, candidate_id, mutual_score, forward_score, reverse_score, status, match_id, responded_at) \
         VALUES ($1,$2,$3,$4,$5,'auto_matched',$6,NOW()) ON CONFLICT DO NOTHING",
    )
    .bind(a).bind(b).bind(score).bind(fwd).bind(rev).bind(&match_id)
    .execute(&state.db)
    .await;

    notify_user(state, a, "auto_matched", json!({ "match_id": match_id, "candidate_id": b, "score": score })).await;
    notify_user(state, b, "auto_matched", json!({ "match_id": match_id, "candidate_id": a, "score": score })).await;

    Some(match_id)
}

async fn notify_user(state: &AppState, user_id: i64, kind: &str, payload: serde_json::Value) {
    crate::handlers::publish_user_event(state, user_id as i32, kind, payload).await;
}

// ============================================================================
// Prompt-driven matchmaker agent (all-Rust; no Python / Agent Lightning).
//
// A user's natural-language intent is parsed into structured, governed filters
// in Rust, then run through the SAME reciprocal scorer. The LLM never touches
// the data layer — only structured, user-scoped filters do. The "learning from
// engagement" is the existing RL bandit (record_swipe_weighted on accept/decline).
// ============================================================================

#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct AgentFilters {
    pub min_age: Option<i32>,
    pub max_age: Option<i32>,
    pub only_verified: Option<bool>,
    pub interests: Option<Vec<String>>,
    pub limit: Option<i64>,
    /// If true, high-scoring candidates are turned into proposals.
    #[serde(default)]
    pub propose: bool,
}

#[derive(Debug, serde::Serialize)]
pub struct ScoredCandidate {
    pub candidate_id: i64,
    pub mutual_score: f64,
    pub forward_score: f64,
    pub reverse_score: f64,
}

/// Interest vocabulary the intent parser recognizes. An ONNX intent/slot model
/// (via tract-onnx) can replace this parser behind `parse_intent`'s signature.
const INTEREST_VOCAB: &[&str] = &[
    "hiking", "climbing", "running", "cycling", "yoga", "gym", "fitness",
    "coffee", "tea", "music", "art", "travel", "reading", "gaming", "cooking",
    "foodie", "photography", "dancing", "movies", "dogs", "cats", "surfing",
    "skiing", "wine", "hiking",
];

/// Parse a natural-language match intent into structured, governed filters.
/// Deterministic over the bounded matchmaker vocabulary.
pub fn parse_intent(prompt: &str) -> AgentFilters {
    let p = prompt.to_lowercase();
    let mut f = AgentFilters { limit: Some(10), propose: true, ..Default::default() };

    if p.contains("verified") {
        f.only_verified = Some(true);
    }

    if let Some((lo, hi)) = parse_age_range(&p) {
        f.min_age = Some(lo);
        f.max_age = Some(hi);
    } else {
        if p.contains("20s") || p.contains("twenties") { f.min_age = Some(20); f.max_age = Some(29); }
        else if p.contains("30s") || p.contains("thirties") { f.min_age = Some(30); f.max_age = Some(39); }
        else if p.contains("40s") || p.contains("forties") { f.min_age = Some(40); f.max_age = Some(49); }
        if let Some(n) = parse_after(&p, "over ").or_else(|| parse_after(&p, "older than ")) {
            f.min_age = Some(n);
        }
        if let Some(n) = parse_after(&p, "under ").or_else(|| parse_after(&p, "younger than ")) {
            f.max_age = Some(n);
        }
    }

    let mut interests: Vec<String> = INTEREST_VOCAB
        .iter()
        .filter(|kw| p.contains(**kw))
        .map(|s| s.to_string())
        .collect();
    interests.sort();
    interests.dedup();
    if !interests.is_empty() {
        f.interests = Some(interests);
    }

    if let Some(n) = parse_after(&p, "top ") {
        f.limit = Some((n as i64).clamp(1, 50));
    }

    f
}

/// Read the integer immediately following `prefix` in `p`, if any.
fn parse_after(p: &str, prefix: &str) -> Option<i32> {
    let start = p.find(prefix)? + prefix.len();
    let digits: String = p[start..].chars().take_while(|c| c.is_ascii_digit()).collect();
    digits.parse().ok()
}

/// Parse an age range like "25-30" or "25 to 30" into a sane (min, max).
fn parse_age_range(p: &str) -> Option<(i32, i32)> {
    for sep in ["-", " to "] {
        if let Some(pos) = p.find(sep) {
            let left: String = p[..pos]
                .chars().rev().take_while(|c| c.is_ascii_digit())
                .collect::<String>().chars().rev().collect();
            let right: String = p[pos + sep.len()..]
                .chars().take_while(|c| c.is_ascii_digit()).collect();
            if let (Ok(lo), Ok(hi)) = (left.parse::<i32>(), right.parse::<i32>()) {
                if (18..100).contains(&lo) && hi >= lo && hi < 100 {
                    return Some((lo, hi));
                }
            }
        }
    }
    None
}

/// Governed tool: structured filters → reciprocally-scored candidates for
/// `user_id`. Only ever sees structured, user-scoped input — never raw SQL from
/// the model. Reuses the existing per-user preference model in both directions.
pub async fn agent_query(state: &AppState, user_id: i32, filters: &AgentFilters) -> Vec<ScoredCandidate> {
    let limit = filters.limit.unwrap_or(10).clamp(1, 50);

    let candidates = sqlx::query_scalar::<_, i64>(
        r#"
        SELECT u.id FROM users u
        WHERE u.id <> $1
          AND u.is_active = TRUE
          AND u.is_profile_complete = TRUE
          AND NOT EXISTS (SELECT 1 FROM swipes s WHERE s.from_user_id = $1 AND s.to_user_id = u.id)
          AND NOT EXISTS (
              SELECT 1 FROM matches m
              WHERE (m.user1_id = $1 AND m.user2_id = u.id)
                 OR (m.user1_id = u.id AND m.user2_id = $1))
          AND NOT EXISTS (
              SELECT 1 FROM auto_match_suggestions a
              WHERE a.user_id = $1 AND a.candidate_id = u.id
                AND a.status IN ('pending','accepted','auto_matched'))
          AND ($2::int IS NULL OR (u.dob IS NOT NULL AND EXTRACT(YEAR FROM AGE(u.dob)) >= $2))
          AND ($3::int IS NULL OR (u.dob IS NOT NULL AND EXTRACT(YEAR FROM AGE(u.dob)) <= $3))
          AND ($4::bool IS NULL OR u.is_verified = $4)
          AND ($5::text[] IS NULL OR (u.interests IS NOT NULL AND jsonb_exists_any(u.interests, $5)))
        ORDER BY u.last_active DESC NULLS LAST
        LIMIT $6
        "#,
    )
    .bind(user_id as i64)
    .bind(filters.min_age)
    .bind(filters.max_age)
    .bind(filters.only_verified)
    .bind(filters.interests.as_deref())
    .bind(limit * 3) // over-fetch, then re-rank by reciprocal score
    .fetch_all(state.read_pool())
    .await
    .unwrap_or_default();

    if candidates.is_empty() {
        return Vec::new();
    }
    let cand_i32: Vec<i32> = candidates.iter().map(|c| *c as i32).collect();

    let mut forward = forward_scores(state, user_id, &cand_i32).await;
    forward.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    forward.truncate(limit as usize);

    // Read the GNN blend weight once per request (0 = off).
    let gnn_weight = MatchmakerConfig::from_env().gnn_weight;

    let mut out = Vec::with_capacity(forward.len());
    for (b, f) in forward {
        let r = forward_scores(state, b, &[user_id]).await
            .first().map(|x| x.1).unwrap_or(0.0);
        let base = reciprocal(f, r);
        let m = blend_gnn(state, user_id as i64, b as i64, base, gnn_weight).await;
        if filters.propose && m >= 0.5 {
            let _ = insert_proposal(state, user_id as i64, b as i64, m, f, r).await;
        }
        out.push(ScoredCandidate { candidate_id: b as i64, mutual_score: m, forward_score: f, reverse_score: r });
    }
    out.sort_by(|a, b| b.mutual_score.partial_cmp(&a.mutual_score).unwrap_or(std::cmp::Ordering::Equal));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_age_range() {
        let f = parse_intent("show me people 25-30");
        assert_eq!(f.min_age, Some(25));
        assert_eq!(f.max_age, Some(30));
    }

    #[test]
    fn parses_decades_and_verified() {
        let f = parse_intent("verified women in their 20s");
        assert_eq!(f.min_age, Some(20));
        assert_eq!(f.max_age, Some(29));
        assert_eq!(f.only_verified, Some(true));
    }

    #[test]
    fn parses_over_under() {
        assert_eq!(parse_intent("someone over 30").min_age, Some(30));
        assert_eq!(parse_intent("anyone under 28").max_age, Some(28));
    }

    #[test]
    fn extracts_interests() {
        let f = parse_intent("grad students who love hiking and coffee");
        let got = f.interests.unwrap();
        assert!(got.contains(&"hiking".to_string()));
        assert!(got.contains(&"coffee".to_string()));
    }

    #[test]
    fn reciprocal_penalizes_one_sided() {
        // strong one-way (0.9, 0.1) should score below balanced (0.5, 0.5)
        assert!(reciprocal(0.9, 0.1) < reciprocal(0.5, 0.5));
        assert!((reciprocal(1.0, 1.0) - 1.0).abs() < 1e-9);
    }
}
