// =============================================================================
// Nava Graph Abstraction — Public API
//
// Architecture inspired by Netflix's Graph Abstraction Layer:
//   - Property Graph Model (nodes + typed edges + properties)
//   - Forward/reverse edge indexes (GraphStorage)
//   - Write-aside link cache + read-aside property cache (GraphCache)
//   - Gremlin-inspired chainable traversal engine (TraversalEngine)
//   - Schema validation on writes (GraphSchema)
//
// Nava use cases:
//   1. Friend-of-Friend discovery     (2-hop matched_with)
//   2. University network             (2-hop via University)
//   3. Reel collaborative filtering   (2-hop via viewed Reels)
//   4. Fraud / multi-account detect   (2-hop via Device)
//   5. Social proximity score         (common connection count)
// =============================================================================

pub mod cache;
pub mod schema;
pub mod storage;
pub mod traversal;

use redis::aio::ConnectionManager;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;
use sqlx::PgPool;
use uuid::Uuid;

use crate::error::AppError;
use cache::GraphCache;
use schema::{Direction, EdgeType, GraphSchema, NodeType};
use storage::GraphStorage;
use traversal::{
    fof_query, reel_collab_query, shared_device_query, university_network_query,
    TraversalEngine, TraversalResult,
};

// ---------------------------------------------------------------------------
// Public output types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphRecommendation {
    pub user_id:            String,
    pub score:              f64,
    pub reason:             String,
    pub mutual_connections: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphFraudResult {
    pub user_id:           String,
    pub fraud_score:       f64,
    pub shared_device_accounts: Vec<String>,
    pub suspicious:        bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GraphStats {
    pub total_nodes:      i64,
    pub total_edges:      i64,
    pub edge_type_counts: std::collections::HashMap<String, i64>,
}

// ---------------------------------------------------------------------------
// GraphAbstraction — the main entry point
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct GraphAbstraction {
    storage:  GraphStorage,
    cache:    Option<GraphCache>,
    schema:   std::sync::Arc<GraphSchema>,
    engine:   std::sync::Arc<TraversalEngine>,
}

impl GraphAbstraction {
    pub fn new(pool: PgPool, redis: Option<ConnectionManager>) -> Self {
        let storage = GraphStorage::new(pool.clone());
        let cache   = redis.map(GraphCache::new);
        let schema  = std::sync::Arc::new(GraphSchema::nava());
        let engine  = std::sync::Arc::new(TraversalEngine::new(GraphStorage::new(pool)));
        Self { storage, cache, schema, engine }
    }

    // -----------------------------------------------------------------------
    // Write API
    // -----------------------------------------------------------------------

    /// Write a validated edge (rejects if not in schema)
    pub async fn write_edge(
        &self,
        from_type: NodeType,
        from_id: &str,
        edge_type: EdgeType,
        to_type: NodeType,
        to_id: &str,
        properties: Option<Json>,
    ) -> Result<(), AppError> {
        if !self.schema.validate_edge(from_type, edge_type, to_type) {
            return Err(AppError::bad_request(format!(
                "Schema violation: {} -[{}]-> {} is not an allowed edge",
                from_type, edge_type, to_type
            )));
        }

        self.storage.write_edge(from_type, from_id, edge_type, to_type, to_id, properties).await?;

        // For bidirectional edges also write the reverse direction in fwd index
        if self.schema.is_bidirectional(edge_type) {
            self.storage.write_edge(to_type, to_id, edge_type, from_type, from_id, None).await?;
        }

        // Invalidate link cache for both ends
        if let Some(cache) = &self.cache {
            cache.invalidate_links(from_type, from_id, edge_type).await;
            cache.invalidate_links(to_type, to_id, edge_type).await;
        }

        Ok(())
    }

    /// Upsert a node with properties
    pub async fn upsert_node(
        &self,
        node_type: NodeType,
        node_id: &str,
        properties: Json,
    ) -> Result<(), AppError> {
        self.storage.upsert_node(node_type, node_id, properties.clone()).await?;
        if let Some(cache) = &self.cache {
            cache.set_node_props(node_type, node_id, &properties).await;
        }
        Ok(())
    }

    /// Queue a node for async deletion (all edges cleaned up asynchronously)
    pub async fn delete_node(&self, node_type: NodeType, node_id: &str) -> Result<(), AppError> {
        self.storage.queue_node_deletion(node_type, node_id).await?;
        if let Some(cache) = &self.cache {
            cache.invalidate_node(node_type, node_id).await;
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Raw traversal API
    // -----------------------------------------------------------------------

    pub async fn traverse(&self, query: traversal::TraversalQuery) -> Result<TraversalResult, AppError> {
        self.engine.execute(query).await
    }

    // -----------------------------------------------------------------------
    // Nava use-case: Friend-of-Friend recommendations
    // -----------------------------------------------------------------------

    pub async fn friend_of_friend(&self, user_id: &str, limit: usize) -> Result<Vec<GraphRecommendation>, AppError> {
        self.fof_with_counts(user_id, limit).await
    }

    async fn fof_with_counts(&self, user_id: &str, limit: usize) -> Result<Vec<GraphRecommendation>, AppError> {
        use std::collections::HashMap;

        // Hop 1: my matches
        let my_matches = self.storage.get_edge_links(
            NodeType::User, user_id, EdgeType::MatchedWith, Direction::Both, None, 500, false,
        ).await?;

        // Hop 2: their matches (in parallel) — collect neighbor IDs first to avoid borrow issues
        let neighbor_ids: Vec<String> = my_matches.iter()
            .map(|link| link.neighbor_id(user_id))
            .collect();

        let futs: Vec<_> = neighbor_ids.iter().map(|neighbor| {
            self.storage.get_edge_links(
                NodeType::User, neighbor, EdgeType::MatchedWith, Direction::Both, None, 100, false,
            )
        }).collect();

        let hop2_results = futures::future::join_all(futs).await;

        let my_match_ids: std::collections::HashSet<String> =
            my_matches.iter().map(|l| l.neighbor_id(user_id).to_string()).collect();

        let mut counts: HashMap<String, usize> = HashMap::new();
        for res in hop2_results {
            for link in res? {
                let nid = link.neighbor_id(user_id).to_string();
                if nid != user_id && !my_match_ids.contains(&nid) {
                    *counts.entry(nid).or_insert(0) += 1;
                }
            }
        }

        let mut recs: Vec<GraphRecommendation> = counts
            .into_iter()
            .map(|(uid, cnt)| GraphRecommendation {
                user_id:            uid,
                score:              cnt as f64 * 10.0,
                reason:             "friend_of_friend".into(),
                mutual_connections: cnt,
            })
            .collect();

        recs.sort_by(|a, b| b.score.partial_cmp(&a.score).unwrap_or(std::cmp::Ordering::Equal));
        recs.truncate(limit);
        Ok(recs)
    }

    // -----------------------------------------------------------------------
    // Nava use-case: University network
    // -----------------------------------------------------------------------

    pub async fn university_network(&self, user_id: &str, limit: usize) -> Result<Vec<GraphRecommendation>, AppError> {
        let result = self.engine.execute(university_network_query(user_id)).await?;

        let recs = result.nodes.into_iter().take(limit).map(|n| GraphRecommendation {
            user_id:            n.node_id,
            score:              50.0,
            reason:             "same_university".into(),
            mutual_connections: 0,
        }).collect();

        Ok(recs)
    }

    // -----------------------------------------------------------------------
    // Nava use-case: Reel collaborative filtering (users with similar taste)
    // -----------------------------------------------------------------------

    pub async fn reel_collaborators(&self, user_id: &str, limit: usize) -> Result<Vec<GraphRecommendation>, AppError> {
        let result = self.engine.execute(reel_collab_query(user_id)).await?;

        let recs = result.nodes.into_iter().take(limit).map(|n| GraphRecommendation {
            user_id:            n.node_id,
            score:              30.0,
            reason:             "similar_reel_taste".into(),
            mutual_connections: 0,
        }).collect();

        Ok(recs)
    }

    // -----------------------------------------------------------------------
    // Nava use-case: Fraud detection via shared devices
    // -----------------------------------------------------------------------

    pub async fn fraud_check(&self, user_id: &str) -> Result<GraphFraudResult, AppError> {
        let result = self.engine.execute(shared_device_query(user_id)).await?;

        let shared_accounts: Vec<String> = result.nodes.iter().map(|n| n.node_id.clone()).collect();
        let count = shared_accounts.len();
        let fraud_score = match count {
            0     => 0.0,
            1..=2 => 20.0,
            3..=5 => 60.0,
            _     => 90.0,
        };

        Ok(GraphFraudResult {
            user_id:                 user_id.to_string(),
            fraud_score,
            shared_device_accounts:  shared_accounts,
            suspicious:              fraud_score >= 60.0,
        })
    }

    // -----------------------------------------------------------------------
    // Social proximity: count mutual connections between two users
    // -----------------------------------------------------------------------

    pub async fn social_proximity(&self, user_a: &str, user_b: &str) -> Result<usize, AppError> {
        let (a_matches, b_matches) = tokio::join!(
            self.storage.get_edge_links(
                NodeType::User, user_a, EdgeType::MatchedWith, Direction::Both, None, 1000, false
            ),
            self.storage.get_edge_links(
                NodeType::User, user_b, EdgeType::MatchedWith, Direction::Both, None, 1000, false
            ),
        );

        let a_set: std::collections::HashSet<String> =
            a_matches?.iter().map(|l| l.neighbor_id(user_a).to_string()).collect();
        let b_set: std::collections::HashSet<String> =
            b_matches?.iter().map(|l| l.neighbor_id(user_b).to_string()).collect();

        Ok(a_set.intersection(&b_set).count())
    }

    // -----------------------------------------------------------------------
    // Stats
    // -----------------------------------------------------------------------

    pub async fn stats(&self) -> Result<GraphStats, AppError> {
        let total_nodes: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM graph_nodes")
            .fetch_one(self.storage.pool())
            .await
            .unwrap_or(0);

        let total_edges: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM graph_edge_links_fwd")
            .fetch_one(self.storage.pool())
            .await
            .unwrap_or(0);

        let type_rows = sqlx::query_as::<_, (String, i64)>(r#"
            SELECT edge_type::TEXT, COUNT(*) FROM graph_edge_links_fwd GROUP BY edge_type
        "#)
        .fetch_all(self.storage.pool())
        .await
        .unwrap_or_default();

        let edge_type_counts = type_rows.into_iter().collect();

        Ok(GraphStats { total_nodes, total_edges, edge_type_counts })
    }
}

// Expose pool on storage for stats queries
impl GraphStorage {
    pub fn pool(&self) -> &PgPool { &self.pool }
}
