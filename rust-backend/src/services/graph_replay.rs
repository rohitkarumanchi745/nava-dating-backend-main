//! Graph replay service — regenerates derived serving layers from `interaction_events`.
//!
//! The event log is the analytical source of truth. Graph edges, behavior
//! profiles, and other derived stores can rot if writes are dropped or
//! if their schema evolves. This module provides idempotent rebuild jobs
//! so the serving layer can always be reconstructed from events.
//!
//! All rebuilds use ON CONFLICT DO NOTHING so they are safe to run
//! repeatedly without mutating existing correct rows.

use sqlx::PgPool;

/// Rebuild all user→user graph edges from `interaction_events`.
/// Optional `since` filter lets you replay recent activity cheaply.
/// Returns the number of edges written/refreshed per edge type.
pub async fn replay_user_edges(db: &PgPool, since_days: Option<i32>) -> Result<ReplayReport, sqlx::Error> {
    let time_filter = since_days
        .map(|d| format!("AND created_at > NOW() - INTERVAL '{d} days'"))
        .unwrap_or_default();

    // Upsert user nodes for everyone mentioned in events
    let nodes = sqlx::query(&format!(
        r#"
        INSERT INTO graph_nodes (node_type, node_id, properties)
        SELECT DISTINCT 'user', user_id::text, '{{}}'::jsonb
        FROM interaction_events WHERE 1=1 {time_filter}
        UNION
        SELECT DISTINCT 'user', target_user_id::text, '{{}}'::jsonb
        FROM interaction_events WHERE target_user_id IS NOT NULL {time_filter}
        ON CONFLICT DO NOTHING
        "#
    ))
    .execute(db).await?.rows_affected();

    // Forward liked edges
    let liked_fwd = sqlx::query(&format!(
        r#"
        INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id)
        SELECT DISTINCT 'user', user_id::text, 'liked', 'user', target_user_id::text
        FROM interaction_events
        WHERE event_type IN ('like','like_with_message','superlike')
          AND target_user_id IS NOT NULL {time_filter}
        ON CONFLICT DO NOTHING
        "#
    )).execute(db).await?.rows_affected();

    // Reverse liked edges
    sqlx::query(&format!(
        r#"
        INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id)
        SELECT DISTINCT 'user', target_user_id::text, 'liked', 'user', user_id::text
        FROM interaction_events
        WHERE event_type IN ('like','like_with_message','superlike')
          AND target_user_id IS NOT NULL {time_filter}
        ON CONFLICT DO NOTHING
        "#
    )).execute(db).await?;

    // Forward passed edges
    let passed_fwd = sqlx::query(&format!(
        r#"
        INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id)
        SELECT DISTINCT 'user', user_id::text, 'passed', 'user', target_user_id::text
        FROM interaction_events
        WHERE event_type = 'pass' AND target_user_id IS NOT NULL {time_filter}
        ON CONFLICT DO NOTHING
        "#
    )).execute(db).await?.rows_affected();

    sqlx::query(&format!(
        r#"
        INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id)
        SELECT DISTINCT 'user', target_user_id::text, 'passed', 'user', user_id::text
        FROM interaction_events
        WHERE event_type = 'pass' AND target_user_id IS NOT NULL {time_filter}
        ON CONFLICT DO NOTHING
        "#
    )).execute(db).await?;

    // matched_with edges from mutual matches (derived from matches table)
    let matched_fwd = sqlx::query(&format!(
        r#"
        INSERT INTO graph_edge_links_fwd (from_type, from_id, edge_type, to_type, to_id)
        SELECT 'user', user1_id::text, 'matched_with', 'user', user2_id::text
        FROM matches WHERE is_mutual_match = TRUE {}
        UNION
        SELECT 'user', user2_id::text, 'matched_with', 'user', user1_id::text
        FROM matches WHERE is_mutual_match = TRUE {}
        ON CONFLICT DO NOTHING
        "#,
        since_days.map(|d| format!("AND updated_at > NOW() - INTERVAL '{d} days'")).unwrap_or_default(),
        since_days.map(|d| format!("AND updated_at > NOW() - INTERVAL '{d} days'")).unwrap_or_default(),
    )).execute(db).await?.rows_affected();

    sqlx::query(&format!(
        r#"
        INSERT INTO graph_edge_links_rev (to_type, to_id, edge_type, from_type, from_id)
        SELECT 'user', user2_id::text, 'matched_with', 'user', user1_id::text
        FROM matches WHERE is_mutual_match = TRUE {}
        UNION
        SELECT 'user', user1_id::text, 'matched_with', 'user', user2_id::text
        FROM matches WHERE is_mutual_match = TRUE {}
        ON CONFLICT DO NOTHING
        "#,
        since_days.map(|d| format!("AND updated_at > NOW() - INTERVAL '{d} days'")).unwrap_or_default(),
        since_days.map(|d| format!("AND updated_at > NOW() - INTERVAL '{d} days'")).unwrap_or_default(),
    )).execute(db).await?;

    Ok(ReplayReport {
        nodes_affected: nodes,
        liked_edges: liked_fwd,
        passed_edges: passed_fwd,
        matched_edges: matched_fwd,
    })
}

#[derive(Debug, serde::Serialize)]
pub struct ReplayReport {
    pub nodes_affected: u64,
    pub liked_edges: u64,
    pub passed_edges: u64,
    pub matched_edges: u64,
}
