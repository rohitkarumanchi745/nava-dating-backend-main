// =============================================================================
// Graph Traversal Engine — Gremlin-inspired chainable traversal API
//
// Mirrors Netflix's traversal design:
//   - TraversalQuery: builder with start node + ordered steps
//   - TraversalStep: single hop with direction, edge filters, node filters, limit
//   - TraversalEngine::execute: runs steps sequentially, fans out per-node in
//     parallel at each hop using tokio::join_all
//
// Enforces per-hop fan-out limits to bound latency on high-degree nodes.
// =============================================================================

use std::collections::{HashMap, HashSet};
use std::time::Instant;

use futures::future::join_all;
use serde::{Deserialize, Serialize};
use serde_json::Value as Json;

use crate::error::AppError;
use super::schema::{Direction, EdgeType, NodeType};
use super::storage::{GraphStorage, EdgeLink};

// ---------------------------------------------------------------------------
// Query builder types
// ---------------------------------------------------------------------------

/// A single traversal hop
#[derive(Debug, Clone)]
pub struct TraversalStep {
    pub direction:    Direction,
    pub edge_types:   Vec<EdgeType>,
    pub node_filter:  Vec<NodeType>,   // if non-empty, only keep these node types
    pub limit:        usize,           // max edges to follow per source node
    pub latest_first: bool,
}

impl TraversalStep {
    pub fn new(direction: Direction, edge_types: Vec<EdgeType>, limit: usize) -> Self {
        Self {
            direction,
            edge_types,
            node_filter: vec![],
            limit,
            latest_first: false,
        }
    }

    pub fn filter_nodes(mut self, types: Vec<NodeType>) -> Self {
        self.node_filter = types;
        self
    }

    pub fn latest_first(mut self) -> Self {
        self.latest_first = true;
        self
    }
}

/// Full traversal query (start node + ordered steps)
#[derive(Debug, Clone)]
pub struct TraversalQuery {
    pub start_type: NodeType,
    pub start_id:   String,
    pub steps:      Vec<TraversalStep>,
}

impl TraversalQuery {
    pub fn start(node_type: NodeType, node_id: impl Into<String>) -> Self {
        Self { start_type: node_type, start_id: node_id.into(), steps: vec![] }
    }

    pub fn hop(mut self, step: TraversalStep) -> Self {
        self.steps.push(step);
        self
    }
}

// ---------------------------------------------------------------------------
// Result types
// ---------------------------------------------------------------------------

/// A node reached during traversal (with optional edge properties)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraversalNode {
    pub node_type:       String,
    pub node_id:         String,
    pub edge_properties: Option<Json>,
}

/// Full traversal result
#[derive(Debug, Clone, Serialize)]
pub struct TraversalResult {
    pub start_id:          String,
    pub hops:              usize,
    /// Reached nodes at the *final* hop, deduplicated
    pub nodes:             Vec<TraversalNode>,
    pub traversal_time_ms: u64,
}

// ---------------------------------------------------------------------------
// Engine
// ---------------------------------------------------------------------------

pub struct TraversalEngine {
    storage: GraphStorage,
}

impl TraversalEngine {
    pub fn new(storage: GraphStorage) -> Self { Self { storage } }

    /// Execute a multi-hop traversal. Returns deduplicated nodes reached at
    /// the *final* step, excluding the start node.
    pub async fn execute(&self, query: TraversalQuery) -> Result<TraversalResult, AppError> {
        let t0 = Instant::now();
        let start_id = query.start_id.clone();

        // frontier: set of (node_type, node_id) pairs at the current hop
        let mut frontier: Vec<(NodeType, String)> =
            vec![(query.start_type, query.start_id.clone())];

        let mut final_nodes: Vec<TraversalNode> = vec![];
        let hops = query.steps.len();

        for (hop_idx, step) in query.steps.iter().enumerate() {
            let is_last = hop_idx == hops - 1;

            // For each node in the frontier, fan out to neighbors in parallel
            let futures: Vec<_> = frontier
                .iter()
                .map(|(ntype, nid)| {
                    self.expand_node(*ntype, nid.clone(), step)
                })
                .collect();

            let results = join_all(futures).await;

            let mut next_frontier: Vec<(NodeType, String)> = vec![];
            let mut seen: HashSet<String> = HashSet::new();
            // Always exclude the original start node
            seen.insert(start_id.clone());

            for (res, (_, src_id)) in results.into_iter().zip(frontier.iter()) {
                let links = res?;
                for link in links {
                    let neighbor_id   = link.neighbor_id(src_id);
                    let neighbor_type = link.neighbor_type(src_id);

                    // Apply node type filter
                    if !step.node_filter.is_empty()
                        && !step.node_filter.contains(&neighbor_type)
                    {
                        continue;
                    }

                    if seen.insert(neighbor_id.clone()) {
                        if is_last {
                            final_nodes.push(TraversalNode {
                                node_type:       neighbor_type.to_string(),
                                node_id:         neighbor_id.clone(),
                                edge_properties: None,
                            });
                        }
                        next_frontier.push((neighbor_type, neighbor_id));
                    }
                }
            }

            if !is_last {
                frontier = next_frontier;
            }
        }

        Ok(TraversalResult {
            start_id:          query.start_id,
            hops,
            nodes:             final_nodes,
            traversal_time_ms: t0.elapsed().as_millis() as u64,
        })
    }

    /// Expand a single node at one hop: fetch all matching edge links
    async fn expand_node(
        &self,
        node_type: NodeType,
        node_id: String,
        step: &TraversalStep,
    ) -> Result<Vec<EdgeLink>, AppError> {
        let mut all_links: Vec<EdgeLink> = vec![];

        for &edge_type in &step.edge_types {
            let links = self.storage.get_edge_links(
                node_type,
                &node_id,
                edge_type,
                step.direction,
                None,
                step.limit,
                step.latest_first,
            ).await?;
            all_links.extend(links);
        }

        Ok(all_links)
    }

    /// Convenience: collect final node IDs as strings
    pub async fn node_ids(&self, query: TraversalQuery) -> Result<Vec<String>, AppError> {
        let result = self.execute(query).await?;
        Ok(result.nodes.into_iter().map(|n| n.node_id).collect())
    }
}

// ---------------------------------------------------------------------------
// Nava-specific named traversal queries
// ---------------------------------------------------------------------------

/// Returns user IDs reachable via 2-hop matched_with (friend-of-friend)
pub fn fof_query(user_id: &str) -> TraversalQuery {
    TraversalQuery::start(NodeType::User, user_id)
        .hop(TraversalStep::new(Direction::Both, vec![EdgeType::MatchedWith], 200))
        .hop(TraversalStep::new(Direction::Both, vec![EdgeType::MatchedWith], 50)
            .filter_nodes(vec![NodeType::User]))
}

/// Returns user IDs at the same university (2-hop via University node)
pub fn university_network_query(user_id: &str) -> TraversalQuery {
    TraversalQuery::start(NodeType::User, user_id)
        .hop(TraversalStep::new(Direction::Out, vec![EdgeType::Attends], 5))
        .hop(TraversalStep::new(Direction::In, vec![EdgeType::Attends], 500)
            .filter_nodes(vec![NodeType::User]))
}

/// Collaborative filtering: users who viewed reels I viewed (2-hop)
pub fn reel_collab_query(user_id: &str) -> TraversalQuery {
    TraversalQuery::start(NodeType::User, user_id)
        .hop(TraversalStep::new(Direction::Out, vec![EdgeType::Viewed], 100)
            .latest_first())
        .hop(TraversalStep::new(Direction::In, vec![EdgeType::Viewed], 200)
            .filter_nodes(vec![NodeType::User]))
}

/// Fraud detection: users sharing the same device (2-hop via Device node)
pub fn shared_device_query(user_id: &str) -> TraversalQuery {
    TraversalQuery::start(NodeType::User, user_id)
        .hop(TraversalStep::new(Direction::Out, vec![EdgeType::Uses], 10))
        .hop(TraversalStep::new(Direction::In, vec![EdgeType::Uses], 100)
            .filter_nodes(vec![NodeType::User]))
}
