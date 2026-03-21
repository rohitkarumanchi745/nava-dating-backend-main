// =============================================================================
// Graph Storage — Postgres read/write for nodes, edge links, edge properties
//
// Mirrors Netflix KV Abstraction:
//   - graph_nodes            → node property store (one row per node)
//   - graph_edge_links_fwd   → forward adjacency list
//   - graph_edge_links_rev   → reverse adjacency list
//   - graph_edge_properties  → direction-agnostic property store
//
// Edge links and edge properties are intentionally separated to prevent
// wide rows and allow independent property upserts.
// =============================================================================

use chrono::{DateTime, Utc};
use serde_json::Value as Json;
use sqlx::PgPool;

use crate::error::AppError;
use super::schema::{Direction, EdgeType, NodeType};

// ---------------------------------------------------------------------------
// Return types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct EdgeLink {
    pub from_type:  NodeType,
    pub from_id:    String,
    pub edge_type:  EdgeType,
    pub to_type:    NodeType,
    pub to_id:      String,
    pub written_at: DateTime<Utc>,
}

impl EdgeLink {
    /// The "neighbor" node id when traversing from a known source
    pub fn neighbor_id(&self, source_id: &str) -> String {
        if self.from_id == source_id { self.to_id.clone() } else { self.from_id.clone() }
    }

    pub fn neighbor_type(&self, source_id: &str) -> NodeType {
        if self.from_id == source_id { self.to_type } else { self.from_type }
    }
}

#[derive(Debug, Clone)]
pub struct NodeRecord {
    pub node_type:  NodeType,
    pub node_id:    String,
    pub properties: Json,
    pub updated_at: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Storage
// ---------------------------------------------------------------------------

#[derive(Clone)]
pub struct GraphStorage {
    pub pool: PgPool,
}

impl GraphStorage {
    pub fn new(pool: PgPool) -> Self { Self { pool } }

    // -----------------------------------------------------------------------
    // Node operations
    // -----------------------------------------------------------------------

    pub async fn upsert_node(
        &self,
        node_type: NodeType,
        node_id: &str,
        properties: Json,
    ) -> Result<(), AppError> {
        sqlx::query(r#"
            INSERT INTO graph_nodes (node_type, node_id, properties, updated_at)
            VALUES ($1::graph_node_type, $2, $3, NOW())
            ON CONFLICT (node_type, node_id)
            DO UPDATE SET properties = EXCLUDED.properties, updated_at = NOW()
        "#)
        .bind(node_type.to_string())
        .bind(node_id)
        .bind(&properties)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }

    pub async fn get_node(
        &self,
        node_type: NodeType,
        node_id: &str,
    ) -> Result<Option<NodeRecord>, AppError> {
        let row = sqlx::query_as::<_, (String, Json, DateTime<Utc>)>(r#"
            SELECT node_id, properties, updated_at
            FROM graph_nodes
            WHERE node_type = $1::graph_node_type AND node_id = $2
        "#)
        .bind(node_type.to_string())
        .bind(node_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(row.map(|(id, props, ts)| NodeRecord {
            node_type,
            node_id: id,
            properties: props,
            updated_at: ts,
        }))
    }

    // -----------------------------------------------------------------------
    // Edge write — updates both forward and reverse indexes
    // -----------------------------------------------------------------------

    pub async fn write_edge(
        &self,
        from_type: NodeType,
        from_id: &str,
        edge_type: EdgeType,
        to_type: NodeType,
        to_id: &str,
        properties: Option<Json>,
    ) -> Result<(), AppError> {
        let now = Utc::now();

        // Forward index
        sqlx::query(r#"
            INSERT INTO graph_edge_links_fwd
                (from_type, from_id, edge_type, to_type, to_id, written_at)
            VALUES ($1::graph_node_type, $2, $3::graph_edge_type, $4::graph_node_type, $5, $6)
            ON CONFLICT (from_type, from_id, edge_type, to_type, to_id)
            DO UPDATE SET written_at = EXCLUDED.written_at
        "#)
        .bind(from_type.to_string()).bind(from_id)
        .bind(edge_type.to_string())
        .bind(to_type.to_string()).bind(to_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        // Reverse index
        sqlx::query(r#"
            INSERT INTO graph_edge_links_rev
                (to_type, to_id, edge_type, from_type, from_id, written_at)
            VALUES ($1::graph_node_type, $2, $3::graph_edge_type, $4::graph_node_type, $5, $6)
            ON CONFLICT (to_type, to_id, edge_type, from_type, from_id)
            DO UPDATE SET written_at = EXCLUDED.written_at
        "#)
        .bind(to_type.to_string()).bind(to_id)
        .bind(edge_type.to_string())
        .bind(from_type.to_string()).bind(from_id)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        // Edge properties (direction-agnostic merge)
        if let Some(props) = properties {
            let edge_key = make_edge_key(from_id, to_id);
            sqlx::query(r#"
                INSERT INTO graph_edge_properties (edge_key, edge_type, properties, written_at)
                VALUES ($1, $2::graph_edge_type, $3, $4)
                ON CONFLICT (edge_key, edge_type)
                DO UPDATE SET
                    properties = graph_edge_properties.properties || EXCLUDED.properties,
                    written_at = EXCLUDED.written_at
            "#)
            .bind(&edge_key)
            .bind(edge_type.to_string())
            .bind(&props)
            .bind(now)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;
        }

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Edge delete
    // -----------------------------------------------------------------------

    pub async fn delete_edge(
        &self,
        from_type: NodeType,
        from_id: &str,
        edge_type: EdgeType,
        to_type: NodeType,
        to_id: &str,
    ) -> Result<(), AppError> {
        sqlx::query(r#"
            DELETE FROM graph_edge_links_fwd
            WHERE from_type = $1::graph_node_type AND from_id = $2
              AND edge_type = $3::graph_edge_type
              AND to_type = $4::graph_node_type AND to_id = $5
        "#)
        .bind(from_type.to_string()).bind(from_id)
        .bind(edge_type.to_string())
        .bind(to_type.to_string()).bind(to_id)
        .execute(&self.pool).await
        .map_err(|e| AppError::Database(e.to_string()))?;

        sqlx::query(r#"
            DELETE FROM graph_edge_links_rev
            WHERE to_type = $1::graph_node_type AND to_id = $2
              AND edge_type = $3::graph_edge_type
              AND from_type = $4::graph_node_type AND from_id = $5
        "#)
        .bind(to_type.to_string()).bind(to_id)
        .bind(edge_type.to_string())
        .bind(from_type.to_string()).bind(from_id)
        .execute(&self.pool).await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    // -----------------------------------------------------------------------
    // Edge link reads (used by traversal engine)
    // -----------------------------------------------------------------------

    pub async fn get_edge_links(
        &self,
        start_type: NodeType,
        start_id: &str,
        edge_type: EdgeType,
        direction: Direction,
        target_type: Option<NodeType>,
        limit: usize,
        latest_first: bool,
    ) -> Result<Vec<EdgeLink>, AppError> {
        match direction {
            Direction::Out => {
                self.fwd_links(start_type, start_id, edge_type, target_type, limit, latest_first).await
            }
            Direction::In => {
                self.rev_links(start_type, start_id, edge_type, target_type, limit, latest_first).await
            }
            Direction::Both => {
                let (fwd, rev) = tokio::join!(
                    self.fwd_links(start_type, start_id, edge_type, target_type, limit, latest_first),
                    self.rev_links(start_type, start_id, edge_type, target_type, limit, latest_first),
                );
                let mut links = fwd?;
                links.extend(rev?);
                // Deduplicate for bidirectional same-type edges
                links.sort_by(|a, b| {
                    let ka = make_edge_key(&a.from_id, &a.to_id);
                    let kb = make_edge_key(&b.from_id, &b.to_id);
                    ka.cmp(&kb).then(b.written_at.cmp(&a.written_at))
                });
                links.dedup_by(|a, b| make_edge_key(&a.from_id, &a.to_id) == make_edge_key(&b.from_id, &b.to_id));
                links.truncate(limit);
                Ok(links)
            }
        }
    }

    async fn fwd_links(
        &self,
        from_type: NodeType,
        from_id: &str,
        edge_type: EdgeType,
        to_type: Option<NodeType>,
        limit: usize,
        latest_first: bool,
    ) -> Result<Vec<EdgeLink>, AppError> {
        let order = if latest_first { "DESC" } else { "ASC" };
        let sql = format!(r#"
            SELECT from_type::TEXT, from_id, edge_type::TEXT, to_type::TEXT, to_id, written_at
            FROM graph_edge_links_fwd
            WHERE from_type = $1::graph_node_type
              AND from_id = $2
              AND edge_type = $3::graph_edge_type
              AND ($4::TEXT IS NULL OR to_type::TEXT = $4)
            ORDER BY written_at {order}
            LIMIT $5
        "#);

        let rows = sqlx::query_as::<_, (String, String, String, String, String, DateTime<Utc>)>(&sql)
            .bind(from_type.to_string())
            .bind(from_id)
            .bind(edge_type.to_string())
            .bind(to_type.map(|t| t.to_string()))
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(row_to_link).collect())
    }

    async fn rev_links(
        &self,
        to_type: NodeType,
        to_id: &str,
        edge_type: EdgeType,
        from_type: Option<NodeType>,
        limit: usize,
        latest_first: bool,
    ) -> Result<Vec<EdgeLink>, AppError> {
        let order = if latest_first { "DESC" } else { "ASC" };
        let sql = format!(r#"
            SELECT from_type::TEXT, from_id, edge_type::TEXT, to_type::TEXT, to_id, written_at
            FROM graph_edge_links_rev
            WHERE to_type = $1::graph_node_type
              AND to_id = $2
              AND edge_type = $3::graph_edge_type
              AND ($4::TEXT IS NULL OR from_type::TEXT = $4)
            ORDER BY written_at {order}
            LIMIT $5
        "#);

        let rows = sqlx::query_as::<_, (String, String, String, String, String, DateTime<Utc>)>(&sql)
            .bind(to_type.to_string())
            .bind(to_id)
            .bind(edge_type.to_string())
            .bind(from_type.map(|t| t.to_string()))
            .bind(limit as i64)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(row_to_link).collect())
    }

    // -----------------------------------------------------------------------
    // Edge property read
    // -----------------------------------------------------------------------

    pub async fn get_edge_properties(
        &self,
        from_id: &str,
        to_id: &str,
        edge_type: EdgeType,
    ) -> Result<Option<Json>, AppError> {
        let edge_key = make_edge_key(from_id, to_id);
        let row = sqlx::query_as::<_, (Json,)>(r#"
            SELECT properties FROM graph_edge_properties
            WHERE edge_key = $1 AND edge_type = $2::graph_edge_type
        "#)
        .bind(&edge_key)
        .bind(edge_type.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(row.map(|(p,)| p))
    }

    // -----------------------------------------------------------------------
    // Queue node for async deletion
    // -----------------------------------------------------------------------

    pub async fn queue_node_deletion(
        &self,
        node_type: NodeType,
        node_id: &str,
    ) -> Result<(), AppError> {
        sqlx::query(r#"
            INSERT INTO graph_delete_queue (node_type, node_id)
            VALUES ($1::graph_node_type, $2)
        "#)
        .bind(node_type.to_string())
        .bind(node_id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Direction-agnostic edge key: LEAST(a,b) + ":" + GREATEST(a,b)
pub fn make_edge_key(id1: &str, id2: &str) -> String {
    if id1 <= id2 { format!("{}:{}", id1, id2) } else { format!("{}:{}", id2, id1) }
}

fn row_to_link(row: (String, String, String, String, String, DateTime<Utc>)) -> EdgeLink {
    let (ft, fid, et, tt, tid, wa) = row;
    EdgeLink {
        from_type:  NodeType::from_str(&ft),
        from_id:    fid,
        edge_type:  EdgeType::from_str(&et),
        to_type:    NodeType::from_str(&tt),
        to_id:      tid,
        written_at: wa,
    }
}
