// =============================================================================
// Graph Cache — Redis write-aside (edge links) + read-aside (properties)
//
// Mirrors Netflix's two caching strategies:
//   1. Write-aside for edge links  — prevents re-writing links that exist
//      TTL: 60s, invalidated on delete
//   2. Read-aside for properties   — reduces Postgres read amplification
//      TTL: 300s, TTL-driven invalidation (properties tolerate short staleness)
// =============================================================================

use redis::aio::ConnectionManager;
use redis::AsyncCommands;
use serde_json::Value as Json;
use std::time::Instant;

use super::schema::{Direction, EdgeType, NodeType};
use super::storage::EdgeLink;

const LINK_TTL_SECS: u64 = 60;
const PROPS_TTL_SECS: u64 = 300;

#[derive(Clone)]
pub struct GraphCache {
    conn: ConnectionManager,
}

impl GraphCache {
    pub fn new(conn: ConnectionManager) -> Self { Self { conn } }

    // -----------------------------------------------------------------------
    // Write-aside: edge link cache
    // Key: graph:links:{from_type}:{from_id}:{edge_type}:{direction}
    // Value: JSON array of EdgeLink summaries
    // -----------------------------------------------------------------------

    pub async fn get_links(
        &self,
        start_type: NodeType,
        start_id: &str,
        edge_type: EdgeType,
        direction: Direction,
    ) -> Option<Vec<String>> {
        let key = link_key(start_type, start_id, edge_type, direction);
        let mut conn = self.conn.clone();
        let raw: Option<String> = conn.get(&key).await.ok()?;
        raw.and_then(|s| serde_json::from_str(&s).ok())
    }

    pub async fn set_links(
        &self,
        start_type: NodeType,
        start_id: &str,
        edge_type: EdgeType,
        direction: Direction,
        neighbor_ids: &[String],
    ) {
        let key = link_key(start_type, start_id, edge_type, direction);
        if let Ok(val) = serde_json::to_string(neighbor_ids) {
            let mut conn = self.conn.clone();
            let _: Result<(), _> = conn.set_ex(&key, val, LINK_TTL_SECS).await;
        }
    }

    /// Invalidate link cache on edge delete
    pub async fn invalidate_links(
        &self,
        start_type: NodeType,
        start_id: &str,
        edge_type: EdgeType,
    ) {
        let mut conn = self.conn.clone();
        for dir in [Direction::Out, Direction::In, Direction::Both] {
            let key = link_key(start_type, start_id, edge_type, dir);
            let _: Result<(), _> = conn.del(&key).await;
        }
    }

    // -----------------------------------------------------------------------
    // Read-aside: edge property cache
    // Key: graph:props:{edge_key}:{edge_type}
    // Value: JSON object of properties
    // -----------------------------------------------------------------------

    pub async fn get_props(
        &self,
        edge_key: &str,
        edge_type: EdgeType,
    ) -> Option<Json> {
        let key = props_key(edge_key, edge_type);
        let mut conn = self.conn.clone();
        let raw: Option<String> = conn.get(&key).await.ok()?;
        raw.and_then(|s| serde_json::from_str(&s).ok())
    }

    pub async fn set_props(
        &self,
        edge_key: &str,
        edge_type: EdgeType,
        props: &Json,
    ) {
        let key = props_key(edge_key, edge_type);
        if let Ok(val) = serde_json::to_string(props) {
            let mut conn = self.conn.clone();
            let _: Result<(), _> = conn.set_ex(&key, val, PROPS_TTL_SECS).await;
        }
    }

    // -----------------------------------------------------------------------
    // Node property cache
    // Key: graph:node:{node_type}:{node_id}
    // -----------------------------------------------------------------------

    pub async fn get_node_props(&self, node_type: NodeType, node_id: &str) -> Option<Json> {
        let key = format!("graph:node:{}:{}", node_type, node_id);
        let mut conn = self.conn.clone();
        let raw: Option<String> = conn.get(&key).await.ok()?;
        raw.and_then(|s| serde_json::from_str(&s).ok())
    }

    pub async fn set_node_props(&self, node_type: NodeType, node_id: &str, props: &Json) {
        let key = format!("graph:node:{}:{}", node_type, node_id);
        if let Ok(val) = serde_json::to_string(props) {
            let mut conn = self.conn.clone();
            let _: Result<(), _> = conn.set_ex(&key, val, PROPS_TTL_SECS).await;
        }
    }

    pub async fn invalidate_node(&self, node_type: NodeType, node_id: &str) {
        let key = format!("graph:node:{}:{}", node_type, node_id);
        let mut conn = self.conn.clone();
        let _: Result<(), _> = conn.del(&key).await;
    }
}

// ---------------------------------------------------------------------------
// Key helpers
// ---------------------------------------------------------------------------

fn link_key(start_type: NodeType, start_id: &str, edge_type: EdgeType, direction: Direction) -> String {
    let dir_str = match direction {
        Direction::Out  => "out",
        Direction::In   => "in",
        Direction::Both => "both",
    };
    format!("graph:links:{}:{}:{}:{}", start_type, start_id, edge_type, dir_str)
}

fn props_key(edge_key: &str, edge_type: EdgeType) -> String {
    format!("graph:props:{}:{}", edge_key, edge_type)
}
