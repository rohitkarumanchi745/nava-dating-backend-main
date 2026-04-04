//! Swipe service — single source of truth for like/pass/superlike domain events.
//!
//! Both REST and GraphQL resolvers call into this module so the business
//! action "user A likes user B" is identical regardless of transport:
//!   - matches table upsert (with mutual detection)
//!   - graph edges written (liked, matched_with)
//!   - graph nodes upserted
//!   - interaction_events row logged
//!   - RL agent fed swipe signal
//!
//! Side effects (graph, events, RL) are fire-and-forget and never fail the caller.

use sqlx::PgPool;
use uuid::Uuid;

/// Outcome of a like action.
#[derive(Debug, Clone)]
pub struct LikeOutcome {
    pub match_id: String,
    pub is_mutual: bool,
    /// True when we inserted a new match row (vs updated an existing one).
    pub is_new_match: bool,
}

/// Core like action. Returns the match_id and whether it's mutual.
///
/// This is the ONE place that owns the semantics of "user_id liked target_id":
/// upserts the matches row, writes graph edges, logs the interaction event.
/// RL agent feeding is done by the caller (needs MlService handle).
pub async fn execute_like(
    db: &PgPool,
    user_id: i32,
    target_id: i32,
    surface: &str,
) -> Result<LikeOutcome, sqlx::Error> {
    // Enforce canonical ordering: lower id is user1
    let (user1_id, user2_id, is_user1) = if user_id < target_id {
        (user_id, target_id, true)
    } else {
        (target_id, user_id, false)
    };

    #[derive(sqlx::FromRow)]
    struct MatchRow {
        id: String,
        user1_liked: Option<bool>,
        user2_liked: Option<bool>,
    }

    let existing = sqlx::query_as::<_, MatchRow>(
        "SELECT id, user1_liked, user2_liked FROM matches WHERE user1_id = $1 AND user2_id = $2"
    )
    .bind(user1_id)
    .bind(user2_id)
    .fetch_optional(db)
    .await?;

    let (match_id, is_mutual, is_new_match) = match existing {
        Some(m) => {
            let other_liked = if is_user1 { m.user2_liked } else { m.user1_liked };
            let is_mutual = other_liked.unwrap_or(false);
            let query = if is_user1 {
                "UPDATE matches SET user1_liked = TRUE, is_mutual_match = $1, updated_at = NOW() WHERE id = $2"
            } else {
                "UPDATE matches SET user2_liked = TRUE, is_mutual_match = $1, updated_at = NOW() WHERE id = $2"
            };
            sqlx::query(query).bind(is_mutual).bind(&m.id).execute(db).await?;
            (m.id, is_mutual, false)
        }
        None => {
            let new_id = Uuid::new_v4().to_string();
            let (u1_liked, u2_liked) = if is_user1 { (true, false) } else { (false, true) };
            sqlx::query(
                "INSERT INTO matches (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, status, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, FALSE, 'active', NOW(), NOW())"
            )
            .bind(&new_id).bind(user1_id).bind(user2_id).bind(u1_liked).bind(u2_liked)
            .execute(db).await?;
            (new_id, false, true)
        }
    };

    // Side effects: graph edges + event log (fire-and-forget, must not fail request)
    write_graph_side_effects(db.clone(), user_id, target_id, is_mutual);
    log_event(db.clone(), user_id, target_id, "like", surface);

    Ok(LikeOutcome { match_id, is_mutual, is_new_match })
}

/// Core pass action.
pub async fn execute_pass(
    db: &PgPool,
    user_id: i32,
    target_id: i32,
    surface: &str,
) -> Result<(), sqlx::Error> {
    let (user1_id, user2_id, is_user1) = if user_id < target_id {
        (user_id, target_id, true)
    } else {
        (target_id, user_id, false)
    };

    let existing: Option<(String,)> = sqlx::query_as(
        "SELECT id FROM matches WHERE user1_id = $1 AND user2_id = $2"
    )
    .bind(user1_id).bind(user2_id).fetch_optional(db).await?;

    match existing {
        Some((id,)) => {
            let query = if is_user1 {
                "UPDATE matches SET user1_liked = FALSE, updated_at = NOW() WHERE id = $1"
            } else {
                "UPDATE matches SET user2_liked = FALSE, updated_at = NOW() WHERE id = $1"
            };
            sqlx::query(query).bind(&id).execute(db).await?;
        }
        None => {
            let new_id = Uuid::new_v4().to_string();
            let (u1_liked, u2_liked) = if is_user1 { (false, true) } else { (true, false) };
            sqlx::query(
                "INSERT INTO matches (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, status, created_at, updated_at) VALUES ($1, $2, $3, $4, $5, FALSE, 'active', NOW(), NOW())"
            )
            .bind(&new_id).bind(user1_id).bind(user2_id).bind(u1_liked).bind(u2_liked)
            .execute(db).await?;
        }
    }

    // Side effects
    write_passed_edge(db.clone(), user_id, target_id);
    log_event(db.clone(), user_id, target_id, "pass", surface);

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers (fire-and-forget)
// ---------------------------------------------------------------------------

fn write_graph_side_effects(db: PgPool, user_id: i32, target_id: i32, is_mutual: bool) {
    let uid = user_id.to_string();
    let tid = target_id.to_string();
    tokio::spawn(async move {
        // Upsert user nodes
        let _ = sqlx::query(
            "INSERT INTO graph_nodes (node_type, node_id, properties) VALUES ('user', $1, '{}') ON CONFLICT DO NOTHING"
        ).bind(&uid).execute(&db).await;
        let _ = sqlx::query(
            "INSERT INTO graph_nodes (node_type, node_id, properties) VALUES ('user', $1, '{}') ON CONFLICT DO NOTHING"
        ).bind(&tid).execute(&db).await;

        // Forward + reverse liked edges
        let _ = sqlx::query(
            "INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id) VALUES ('user', $1, 'liked', 'user', $2) ON CONFLICT DO NOTHING"
        ).bind(&uid).bind(&tid).execute(&db).await;
        let _ = sqlx::query(
            "INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id) VALUES ('user', $2, 'liked', 'user', $1) ON CONFLICT DO NOTHING"
        ).bind(&uid).bind(&tid).execute(&db).await;

        if is_mutual {
            // Bidirectional matched_with edges
            let _ = sqlx::query(
                "INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id) VALUES ('user', $1, 'matched_with', 'user', $2) ON CONFLICT DO NOTHING"
            ).bind(&uid).bind(&tid).execute(&db).await;
            let _ = sqlx::query(
                "INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id) VALUES ('user', $2, 'matched_with', 'user', $1) ON CONFLICT DO NOTHING"
            ).bind(&tid).bind(&uid).execute(&db).await;
            let _ = sqlx::query(
                "INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id) VALUES ('user', $2, 'matched_with', 'user', $1) ON CONFLICT DO NOTHING"
            ).bind(&uid).bind(&tid).execute(&db).await;
            let _ = sqlx::query(
                "INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id) VALUES ('user', $1, 'matched_with', 'user', $2) ON CONFLICT DO NOTHING"
            ).bind(&tid).bind(&uid).execute(&db).await;
        }
    });
}

fn write_passed_edge(db: PgPool, user_id: i32, target_id: i32) {
    let uid = user_id.to_string();
    let tid = target_id.to_string();
    tokio::spawn(async move {
        let _ = sqlx::query(
            "INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id) VALUES ('user', $1, 'passed', 'user', $2) ON CONFLICT DO NOTHING"
        ).bind(&uid).bind(&tid).execute(&db).await;
        let _ = sqlx::query(
            "INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id) VALUES ('user', $2, 'passed', 'user', $1) ON CONFLICT DO NOTHING"
        ).bind(&uid).bind(&tid).execute(&db).await;
    });
}

fn log_event(db: PgPool, user_id: i32, target_id: i32, event_type: &'static str, surface: &str) {
    let surf = surface.to_string();
    tokio::spawn(async move {
        let _ = sqlx::query(
            "INSERT INTO interaction_events (user_id, target_user_id, event_type, surface, created_at) VALUES ($1, $2, $3, $4, NOW())"
        )
        .bind(user_id).bind(target_id).bind(event_type).bind(&surf)
        .execute(&db).await;
    });
}
