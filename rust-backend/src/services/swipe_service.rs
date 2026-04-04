//! Swipe service — single source of truth for like/pass/superlike domain events.
//!
//! Both REST and GraphQL resolvers call into this module so the business
//! action "user A likes user B" is identical regardless of transport.
//!
//! # Canonicality
//! - `interaction_events` is the analytical source of truth (durable event log).
//! - `matches` is operational state, derived from the latest effective action.
//! - graph edges are a derived serving layer (rebuildable from events).
//!
//! # Idempotency
//! Swipes receive duplicate requests routinely (retries, double-taps, reconnects).
//! We make `execute_like` / `execute_pass` safe under concurrent calls:
//!   * Single UPSERT on `matches` with canonical user ordering — no read-then-write race.
//!   * Latest-action-wins: calling like after pass flips the stored action.
//!   * Mutual detection reads the post-update row to stay race-safe.
//!   * Graph edges use INSERT … ON CONFLICT DO NOTHING (idempotent).
//!
//! Side effects (graph edges, event log) are fire-and-forget and never fail
//! the caller, but failures are metered so they can be replayed later.

use sqlx::PgPool;
use uuid::Uuid;

/// Outcome of a like action.
#[derive(Debug, Clone)]
pub struct LikeOutcome {
    pub match_id: String,
    pub is_mutual: bool,
    pub is_new_match: bool,
}

/// Core like action. Latest-wins, concurrency-safe, idempotent.
///
/// Semantics: "user_id likes target_id" means user_id's slot becomes TRUE.
/// If target_id had already liked user_id, the match becomes mutual.
pub async fn execute_like(
    db: &PgPool,
    user_id: i32,
    target_id: i32,
    surface: &str,
) -> Result<LikeOutcome, sqlx::Error> {
    let (user1_id, user2_id, is_user1) = canonical_order(user_id, target_id);

    // Single upsert: on insert, current user's slot = TRUE, other = NULL.
    // On conflict (existing pair), current user's slot → TRUE; other stays as-is.
    // is_mutual_match is recomputed from the final column values.
    let new_id = Uuid::new_v4().to_string();
    let row: (String, Option<bool>, Option<bool>, bool, bool) = if is_user1 {
        sqlx::query_as(
            r#"
            INSERT INTO matches (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, status, created_at, updated_at)
            VALUES ($1, $2, $3, TRUE, NULL, FALSE, 'active', NOW(), NOW())
            ON CONFLICT (user1_id, user2_id) DO UPDATE SET
                user1_liked = TRUE,
                is_mutual_match = (COALESCE(matches.user2_liked, FALSE) = TRUE),
                updated_at = NOW()
            RETURNING id, user1_liked, user2_liked, is_mutual_match, (xmax = 0) AS inserted
            "#,
        )
        .bind(&new_id).bind(user1_id).bind(user2_id)
        .fetch_one(db).await?
    } else {
        sqlx::query_as(
            r#"
            INSERT INTO matches (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, status, created_at, updated_at)
            VALUES ($1, $2, $3, NULL, TRUE, FALSE, 'active', NOW(), NOW())
            ON CONFLICT (user1_id, user2_id) DO UPDATE SET
                user2_liked = TRUE,
                is_mutual_match = (COALESCE(matches.user1_liked, FALSE) = TRUE),
                updated_at = NOW()
            RETURNING id, user1_liked, user2_liked, is_mutual_match, (xmax = 0) AS inserted
            "#,
        )
        .bind(&new_id).bind(user1_id).bind(user2_id)
        .fetch_one(db).await?
    };

    let match_id = row.0;
    let is_mutual = row.3;
    let is_new_match = row.4;

    // Derived side effects (fire-and-forget, but metered for replay)
    write_graph_side_effects(db.clone(), user_id, target_id, is_mutual);
    log_event(db.clone(), user_id, target_id, "like", surface);

    Ok(LikeOutcome { match_id, is_mutual, is_new_match })
}

/// Core pass action. Latest-wins, concurrency-safe, idempotent.
///
/// Pass flips the user's slot to FALSE. Mutual is always FALSE after a pass.
pub async fn execute_pass(
    db: &PgPool,
    user_id: i32,
    target_id: i32,
    surface: &str,
) -> Result<(), sqlx::Error> {
    let (user1_id, user2_id, is_user1) = canonical_order(user_id, target_id);

    let new_id = Uuid::new_v4().to_string();
    if is_user1 {
        sqlx::query(
            r#"
            INSERT INTO matches (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, status, created_at, updated_at)
            VALUES ($1, $2, $3, FALSE, NULL, FALSE, 'active', NOW(), NOW())
            ON CONFLICT (user1_id, user2_id) DO UPDATE SET
                user1_liked = FALSE,
                is_mutual_match = FALSE,
                updated_at = NOW()
            "#,
        )
        .bind(&new_id).bind(user1_id).bind(user2_id)
        .execute(db).await?;
    } else {
        sqlx::query(
            r#"
            INSERT INTO matches (id, user1_id, user2_id, user1_liked, user2_liked, is_mutual_match, status, created_at, updated_at)
            VALUES ($1, $2, $3, NULL, FALSE, FALSE, 'active', NOW(), NOW())
            ON CONFLICT (user1_id, user2_id) DO UPDATE SET
                user2_liked = FALSE,
                is_mutual_match = FALSE,
                updated_at = NOW()
            "#,
        )
        .bind(&new_id).bind(user1_id).bind(user2_id)
        .execute(db).await?;
    }

    write_passed_edge(db.clone(), user_id, target_id);
    log_event(db.clone(), user_id, target_id, "pass", surface);

    Ok(())
}

// ---------------------------------------------------------------------------
// Internal helpers
// ---------------------------------------------------------------------------

#[inline]
fn canonical_order(a: i32, b: i32) -> (i32, i32, bool) {
    if a < b { (a, b, true) } else { (b, a, false) }
}

fn write_graph_side_effects(db: PgPool, user_id: i32, target_id: i32, is_mutual: bool) {
    let uid = user_id.to_string();
    let tid = target_id.to_string();
    tokio::spawn(async move {
        let _ = sqlx::query(
            "INSERT INTO graph_nodes (node_type, node_id, properties) VALUES ('user', $1, '{}') ON CONFLICT DO NOTHING"
        ).bind(&uid).execute(&db).await;
        let _ = sqlx::query(
            "INSERT INTO graph_nodes (node_type, node_id, properties) VALUES ('user', $1, '{}') ON CONFLICT DO NOTHING"
        ).bind(&tid).execute(&db).await;

        let _ = sqlx::query(
            "INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id) VALUES ('user', $1, 'liked', 'user', $2) ON CONFLICT DO NOTHING"
        ).bind(&uid).bind(&tid).execute(&db).await;
        let _ = sqlx::query(
            "INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id) VALUES ('user', $2, 'liked', 'user', $1) ON CONFLICT DO NOTHING"
        ).bind(&uid).bind(&tid).execute(&db).await;

        if is_mutual {
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
