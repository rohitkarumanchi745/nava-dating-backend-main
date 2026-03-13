// =============================================================================
// NAVA Platform - Graph Service (Neo4j Integration with PostgreSQL Sync)
// =============================================================================
// Provides dual-database architecture with automatic failover:
// - Primary writes to both PostgreSQL and Neo4j
// - Automatic fallback when one database is unavailable
// - Background sync to maintain consistency
// =============================================================================

use neo4rs::{Graph, Query, ConfigBuilder};
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use uuid::Uuid;
use tracing::{info, warn, error, instrument};

use crate::error::AppError;
use super::neo4j_http::Neo4jHttp;

// Type alias for Result
pub type Result<T> = std::result::Result<T, AppError>;

// -----------------------------------------------------------------------------
// Types and Structures
// -----------------------------------------------------------------------------

#[derive(Clone)]
pub struct GraphService {
    neo4j: Option<Arc<Graph>>,     // Bolt connection (optional - may not work with Neo4j 5.x)
    neo4j_http: Option<Neo4jHttp>, // HTTP API (works with all Neo4j versions)
    postgres: PgPool,
    health: Arc<RwLock<DatabaseHealth>>,
}

impl std::fmt::Debug for GraphService {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GraphService")
            .field("postgres", &"PgPool")
            .field("health", &self.health)
            .finish()
    }
}

#[derive(Debug, Clone)]
pub struct DatabaseHealth {
    pub neo4j_healthy: bool,
    pub postgres_healthy: bool,
    pub last_neo4j_check: DateTime<Utc>,
    pub last_postgres_check: DateTime<Utc>,
    pub neo4j_error_count: u32,
    pub postgres_error_count: u32,
}

impl Default for DatabaseHealth {
    fn default() -> Self {
        Self {
            neo4j_healthy: true,
            postgres_healthy: true,
            last_neo4j_check: Utc::now(),
            last_postgres_check: Utc::now(),
            neo4j_error_count: 0,
            postgres_error_count: 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct UserNode {
    pub id: Uuid,
    pub phone: String,
    pub name: Option<String>,
    pub gender: Option<String>,
    pub date_of_birth: Option<DateTime<Utc>>,
    pub bio: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub city: Option<String>,
    pub is_verified: bool,
    pub is_premium: bool,
    pub is_student: bool,
    pub is_active: bool,
    pub created_at: DateTime<Utc>,
    pub last_active_at: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone)]
pub struct SwipeAction {
    pub from_user_id: Uuid,
    pub to_user_id: Uuid,
    pub action: SwipeType,
    pub source: String,
    pub created_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SwipeType {
    Like,
    Pass,
    Block,
}

impl std::fmt::Display for SwipeType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SwipeType::Like => write!(f, "LIKED"),
            SwipeType::Pass => write!(f, "PASSED"),
            SwipeType::Block => write!(f, "BLOCKED"),
        }
    }
}

#[derive(Debug, Clone)]
pub struct MatchResult {
    pub user1_id: Uuid,
    pub user2_id: Uuid,
    pub matched_at: DateTime<Utc>,
    pub is_new: bool,
}

#[derive(Debug, Clone)]
pub struct UserRecommendation {
    pub user_id: Uuid,
    pub score: f64,
    pub reason: RecommendationReason,
    pub mutual_connections: i32,
    pub shared_interests: Vec<String>,
}

#[derive(Debug, Clone)]
pub enum RecommendationReason {
    FriendOfFriend,
    SameUniversity,
    SharedInterests,
    LocationBased,
    Popular,
}

// -----------------------------------------------------------------------------
// GraphService Implementation
// -----------------------------------------------------------------------------

impl GraphService {
    /// Create a new GraphService using HTTP API only (for Neo4j 5.x compatibility)
    pub fn with_http(host: &str, user: &str, password: &str, postgres: PgPool) -> Self {
        use super::neo4j_http::Neo4jHttpClient;

        let neo4j_http = Neo4jHttp::new(Neo4jHttpClient::new(host, 7474, user, password));
        info!("GraphService initialized with Neo4j HTTP API (host: {})", host);

        Self {
            neo4j: None,
            neo4j_http: Some(neo4j_http),
            postgres,
            health: Arc::new(RwLock::new(DatabaseHealth::default())),
        }
    }

    /// Create a new GraphService with an existing Neo4j Bolt connection and PostgreSQL pool
    pub fn with_graph(neo4j: Arc<Graph>, postgres: PgPool) -> Self {
        // Also create HTTP client for fallback using explicit env reads
        let neo4j_http = Self::create_http_client();
        if neo4j_http.is_some() {
            info!("GraphService initialized with Neo4j Bolt + HTTP fallback");
        } else {
            info!("GraphService initialized with Neo4j Bolt only");
        }
        Self {
            neo4j: Some(neo4j),
            neo4j_http,
            postgres,
            health: Arc::new(RwLock::new(DatabaseHealth::default())),
        }
    }

    /// Create HTTP client from environment
    fn create_http_client() -> Option<Neo4jHttp> {
        use super::neo4j_http::Neo4jHttpClient;

        let uri = std::env::var("NEO4J_URI").ok()?;
        let user = std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".to_string());
        let password = std::env::var("NEO4J_PASSWORD").ok()?;

        // Parse URI to get host
        let uri_clean = uri.replace("bolt://", "").replace("neo4j://", "");
        let host = uri_clean.split(':').next().unwrap_or("127.0.0.1");

        info!("Creating Neo4j HTTP client for {}", host);
        Some(Neo4jHttp::new(Neo4jHttpClient::new(host, 7474, &user, &password)))
    }

    /// Create a new GraphService by connecting to Neo4j
    pub async fn new(neo4j_uri: &str, neo4j_user: &str, neo4j_password: &str, postgres: PgPool) -> Result<Self> {
        let config = ConfigBuilder::default()
            .uri(neo4j_uri)
            .user(neo4j_user)
            .password(neo4j_password)
            .max_connections(50)
            .build()
            .map_err(|e| AppError::Internal(format!("Neo4j config error: {}", e)))?;

        let neo4j = Graph::connect(config)
            .await
            .map_err(|e| AppError::Internal(format!("Neo4j connection error: {}", e)))?;

        info!("Connected to Neo4j successfully");

        let neo4j_http = Neo4jHttp::from_env();

        Ok(Self {
            neo4j: Some(Arc::new(neo4j)),
            neo4j_http,
            postgres,
            health: Arc::new(RwLock::new(DatabaseHealth::default())),
        })
    }

    /// Get Neo4j Bolt connection for direct queries (if available)
    pub fn neo4j(&self) -> Option<&Graph> {
        self.neo4j.as_ref().map(|g| g.as_ref())
    }

    /// Get Neo4j HTTP client (if available)
    pub fn neo4j_http(&self) -> Option<&Neo4jHttp> {
        self.neo4j_http.as_ref()
    }

    /// Check if any Neo4j connection is available
    pub fn has_neo4j(&self) -> bool {
        self.neo4j.is_some() || self.neo4j_http.is_some()
    }

    /// Get PostgreSQL pool
    pub fn postgres(&self) -> &PgPool {
        &self.postgres
    }

    /// Check health of both databases
    #[instrument(skip(self))]
    pub async fn health_check(&self) -> DatabaseHealth {
        let mut health = self.health.write().await;

        // Check Neo4j - try HTTP first (more reliable), fall back to Bolt
        let neo4j_healthy = if let Some(ref http) = self.neo4j_http {
            // Use HTTP API for health check (works with Neo4j 5.x)
            http.health_check().await
        } else if let Some(ref bolt) = self.neo4j {
            // Fall back to Bolt
            bolt.run(Query::new("RETURN 1".to_string())).await.is_ok()
        } else {
            // No Neo4j connection available
            false
        };

        if neo4j_healthy {
            health.neo4j_healthy = true;
            health.neo4j_error_count = 0;
        } else {
            warn!("Neo4j health check failed");
            health.neo4j_healthy = false;
            health.neo4j_error_count += 1;
        }
        health.last_neo4j_check = Utc::now();

        // Check PostgreSQL
        match sqlx::query("SELECT 1").execute(&self.postgres).await {
            Ok(_) => {
                health.postgres_healthy = true;
                health.postgres_error_count = 0;
            }
            Err(e) => {
                warn!("PostgreSQL health check failed: {}", e);
                health.postgres_healthy = false;
                health.postgres_error_count += 1;
            }
        }
        health.last_postgres_check = Utc::now();

        health.clone()
    }

    /// Check if Neo4j is available
    pub async fn is_neo4j_healthy(&self) -> bool {
        self.health.read().await.neo4j_healthy
    }

    /// Check if PostgreSQL is available
    pub async fn is_postgres_healthy(&self) -> bool {
        self.health.read().await.postgres_healthy
    }

    /// Execute a Cypher query using HTTP API (preferred) or Bolt
    async fn run_cypher(&self, query: &str) -> Result<()> {
        if let Some(ref http) = self.neo4j_http {
            http.execute(query, None).await?;
            Ok(())
        } else if let Some(ref bolt) = self.neo4j {
            bolt.run(Query::new(query.to_string())).await
                .map_err(|e| AppError::Internal(format!("Neo4j query failed: {}", e)))?;
            Ok(())
        } else {
            Err(AppError::Internal("No Neo4j connection available".to_string()))
        }
    }

    /// Execute a Cypher query and get results using HTTP API
    async fn query_cypher(&self, query: &str) -> Result<Vec<serde_json::Value>> {
        if let Some(ref http) = self.neo4j_http {
            let response = http.execute(query, None).await?;
            // Convert results to JSON values
            let mut results = Vec::new();
            for result in response.results {
                for row in result.data {
                    results.push(serde_json::json!(row.row));
                }
            }
            Ok(results)
        } else {
            // For Bolt, we'd need to implement row streaming - for now just return empty
            warn!("Bolt queries not fully supported for result streaming");
            Ok(Vec::new())
        }
    }

    // -------------------------------------------------------------------------
    // User Operations (Dual-Write)
    // -------------------------------------------------------------------------

    /// Create or update user in both databases
    #[instrument(skip(self, user))]
    pub async fn upsert_user(&self, user: &UserNode) -> Result<()> {
        let health = self.health.read().await;

        // Write to PostgreSQL (primary for user data)
        if health.postgres_healthy {
            self.upsert_user_postgres(user).await?;
        } else {
            warn!("PostgreSQL unavailable, queuing user upsert for sync");
            self.queue_postgres_sync("user_upsert", &serde_json::to_string(user).unwrap_or_default()).await;
        }

        // Write to Neo4j (for graph relationships)
        if health.neo4j_healthy {
            self.upsert_user_neo4j(user).await?;
        } else {
            warn!("Neo4j unavailable, queuing user upsert for sync");
            self.queue_neo4j_sync("user_upsert", &serde_json::to_string(user).unwrap_or_default()).await;
        }

        Ok(())
    }

    async fn upsert_user_postgres(&self, user: &UserNode) -> Result<()> {
        sqlx::query(r#"
            INSERT INTO users (id, phone, is_verified, is_premium, is_active, created_at, updated_at)
            VALUES ($1, $2, $3, $4, $5, $6, NOW())
            ON CONFLICT (id) DO UPDATE SET
                is_verified = EXCLUDED.is_verified,
                is_premium = EXCLUDED.is_premium,
                is_active = EXCLUDED.is_active,
                updated_at = NOW()
        "#)
        .bind(user.id)
        .bind(&user.phone)
        .bind(user.is_verified)
        .bind(user.is_premium)
        .bind(user.is_active)
        .bind(user.created_at)
        .execute(&self.postgres)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(())
    }

    async fn upsert_user_neo4j(&self, user: &UserNode) -> Result<()> {
        let query = Query::new(r#"
            MERGE (u:User {id: $id})
            SET u.phone = $phone,
                u.name = $name,
                u.gender = $gender,
                u.is_verified = $is_verified,
                u.is_premium = $is_premium,
                u.is_student = $is_student,
                u.is_active = $is_active,
                u.latitude = $latitude,
                u.longitude = $longitude,
                u.city = $city,
                u.created_at = datetime($created_at),
                u.updated_at = datetime()
        "#.to_string())
        .param("id", user.id.to_string())
        .param("phone", user.phone.clone())
        .param("name", user.name.clone().unwrap_or_default())
        .param("gender", user.gender.clone().unwrap_or_default())
        .param("is_verified", user.is_verified)
        .param("is_premium", user.is_premium)
        .param("is_student", user.is_student)
        .param("is_active", user.is_active)
        .param("latitude", user.latitude.unwrap_or(0.0))
        .param("longitude", user.longitude.unwrap_or(0.0))
        .param("city", user.city.clone().unwrap_or_default())
        .param("created_at", user.created_at.to_rfc3339());

        // Use HTTP API if available, fallback to Bolt
        if let Some(ref http) = self.neo4j_http {
            let cypher = format!(r#"
                MERGE (u:User {{id: '{}'}})
                SET u.phone = '{}',
                    u.name = '{}',
                    u.gender = '{}',
                    u.is_verified = {},
                    u.is_premium = {},
                    u.is_student = {},
                    u.is_active = {},
                    u.latitude = {},
                    u.longitude = {},
                    u.city = '{}',
                    u.created_at = datetime('{}')
            "#,
                user.id, user.phone,
                user.name.as_deref().unwrap_or(""),
                user.gender.as_deref().unwrap_or(""),
                user.is_verified, user.is_premium, user.is_student, user.is_active,
                user.latitude.unwrap_or(0.0), user.longitude.unwrap_or(0.0),
                user.city.as_deref().unwrap_or(""),
                user.created_at.to_rfc3339()
            );
            http.execute(&cypher, None).await?;
        } else if let Some(ref bolt) = self.neo4j {
            bolt.run(query).await
                .map_err(|e| AppError::Internal(format!("Neo4j upsert user error: {}", e)))?;
        }

        Ok(())
    }

    // -------------------------------------------------------------------------
    // University Relationship (Dual-Write)
    // -------------------------------------------------------------------------

    /// Set STUDIES_AT relationship for a user in Neo4j and sync university node.
    /// Called whenever update_profile saves a university. Fire-and-forget safe.
    pub async fn set_user_university(
        &self,
        user_id: i32,
        university_name: &str,
    ) -> Result<()> {
        let health = self.health.read().await;
        if !health.neo4j_healthy {
            warn!("Neo4j unavailable, skipping STUDIES_AT sync for user {}", user_id);
            return Ok(());
        }

        let uid = user_id.to_string();
        let uname = university_name.replace('\'', "\\'");

        // HTTP API path (preferred)
        if let Some(ref http) = self.neo4j_http {
            let cypher = format!(r#"
                MERGE (u:User {{id: '{uid}'}})
                MERGE (c:University {{name: '{uname}'}})
                MERGE (u)-[:STUDIES_AT]->(c)
                SET c.updated_at = datetime()
            "#);
            if let Err(e) = http.execute(&cypher, None).await {
                warn!("Neo4j STUDIES_AT HTTP write failed for user {}: {}", user_id, e);
            }
        } else if let Some(ref bolt) = self.neo4j {
            let q = Query::new(r#"
                MERGE (u:User {id: $uid})
                MERGE (c:University {name: $uname})
                MERGE (u)-[:STUDIES_AT]->(c)
                SET c.updated_at = datetime()
            "#.to_string())
            .param("uid", uid)
            .param("uname", university_name.to_string());
            if let Err(e) = bolt.run(q).await {
                warn!("Neo4j STUDIES_AT bolt write failed for user {}: {}", user_id, e);
            }
        }

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Swipe Operations (Dual-Write)
    // -------------------------------------------------------------------------

    /// Record a swipe action in both databases, returns match if mutual like
    #[instrument(skip(self))]
    pub async fn record_swipe(&self, swipe: &SwipeAction) -> Result<Option<MatchResult>> {
        let health = self.health.read().await;
        let mut match_result: Option<MatchResult> = None;

        // Write to PostgreSQL
        if health.postgres_healthy {
            match_result = self.record_swipe_postgres(swipe).await?;
        } else {
            self.queue_postgres_sync("swipe", &serde_json::to_string(swipe).unwrap_or_default()).await;
        }

        // Write to Neo4j
        if health.neo4j_healthy {
            let neo4j_match = self.record_swipe_neo4j(swipe).await?;
            if match_result.is_none() {
                match_result = neo4j_match;
            }
        } else {
            self.queue_neo4j_sync("swipe", &serde_json::to_string(swipe).unwrap_or_default()).await;
        }

        Ok(match_result)
    }

    async fn record_swipe_postgres(&self, swipe: &SwipeAction) -> Result<Option<MatchResult>> {
        // Insert swipe
        sqlx::query(r#"
            INSERT INTO swipes (from_user_id, to_user_id, action, created_at)
            VALUES ($1, $2, $3, $4)
            ON CONFLICT (from_user_id, to_user_id) DO UPDATE SET
                action = EXCLUDED.action,
                created_at = EXCLUDED.created_at
        "#)
        .bind(swipe.from_user_id)
        .bind(swipe.to_user_id)
        .bind(swipe.action.to_string().to_lowercase())
        .bind(swipe.created_at)
        .execute(&self.postgres)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        // Check for mutual like
        if swipe.action == SwipeType::Like {
            let mutual = sqlx::query_scalar::<_, bool>(r#"
                SELECT EXISTS(
                    SELECT 1 FROM swipes
                    WHERE from_user_id = $1 AND to_user_id = $2 AND action = 'like'
                )
            "#)
            .bind(swipe.to_user_id)
            .bind(swipe.from_user_id)
            .fetch_one(&self.postgres)
            .await
            .map_err(|e| AppError::Database(e.to_string()))?;

            if mutual {
                // Create match (ensure user1_id < user2_id for consistency)
                let (user1_id, user2_id) = if swipe.from_user_id < swipe.to_user_id {
                    (swipe.from_user_id, swipe.to_user_id)
                } else {
                    (swipe.to_user_id, swipe.from_user_id)
                };

                let result = sqlx::query_scalar::<_, bool>(r#"
                    INSERT INTO matches (user1_id, user2_id, created_at)
                    VALUES ($1, $2, $3)
                    ON CONFLICT (user1_id, user2_id) DO NOTHING
                    RETURNING true
                "#)
                .bind(user1_id)
                .bind(user2_id)
                .bind(Utc::now())
                .fetch_optional(&self.postgres)
                .await
                .map_err(|e| AppError::Database(e.to_string()))?;

                return Ok(Some(MatchResult {
                    user1_id,
                    user2_id,
                    matched_at: Utc::now(),
                    is_new: result.unwrap_or(false),
                }));
            }
        }

        Ok(None)
    }

    async fn record_swipe_neo4j(&self, swipe: &SwipeAction) -> Result<Option<MatchResult>> {
        let relationship_type = swipe.action.to_string();

        // Use HTTP API if available
        if let Some(ref http) = self.neo4j_http {
            // Create relationship via HTTP
            let cypher = format!(r#"
                MATCH (from:User {{id: '{}'}}), (to:User {{id: '{}'}})
                MERGE (from)-[r:{}]->(to)
                SET r.created_at = datetime('{}'),
                    r.source = '{}'
            "#, swipe.from_user_id, swipe.to_user_id, relationship_type,
                swipe.created_at.to_rfc3339(), swipe.source);
            http.execute(&cypher, None).await?;

            // Check for mutual like and create MATCHED relationship
            if swipe.action == SwipeType::Like {
                let match_cypher = format!(r#"
                    MATCH (from:User {{id: '{}'}})-[:LIKED]->(to:User {{id: '{}'}}),
                          (to)-[:LIKED]->(from)
                    MERGE (from)-[m:MATCHED]-(to)
                    ON CREATE SET m.created_at = datetime(), m.is_active = true
                    RETURN m.created_at IS NOT NULL as is_new
                "#, swipe.from_user_id, swipe.to_user_id);

                let response = http.execute(&match_cypher, None).await?;
                if let Some(result) = response.results.first() {
                    if let Some(row) = result.data.first() {
                        if let Some(is_new) = row.row.first().and_then(|v| v.as_bool()) {
                            let (user1_id, user2_id) = if swipe.from_user_id < swipe.to_user_id {
                                (swipe.from_user_id, swipe.to_user_id)
                            } else {
                                (swipe.to_user_id, swipe.from_user_id)
                            };
                            return Ok(Some(MatchResult {
                                user1_id,
                                user2_id,
                                matched_at: Utc::now(),
                                is_new,
                            }));
                        }
                    }
                }
            }
        } else if let Some(ref bolt) = self.neo4j {
            // Fallback to Bolt
            let query = Query::new(format!(r#"
                MATCH (from:User {{id: $from_id}}), (to:User {{id: $to_id}})
                MERGE (from)-[r:{relationship_type}]->(to)
                SET r.created_at = datetime($created_at),
                    r.source = $source
            "#))
            .param("from_id", swipe.from_user_id.to_string())
            .param("to_id", swipe.to_user_id.to_string())
            .param("created_at", swipe.created_at.to_rfc3339())
            .param("source", swipe.source.clone());

            bolt.run(query).await
                .map_err(|e| AppError::Internal(format!("Neo4j swipe error: {}", e)))?;

            // Check for mutual like and create MATCHED relationship
            if swipe.action == SwipeType::Like {
                let mut result = bolt.execute(Query::new(r#"
                    MATCH (from:User {id: $from_id})-[:LIKED]->(to:User {id: $to_id}),
                          (to)-[:LIKED]->(from)
                    MERGE (from)-[m:MATCHED]-(to)
                    ON CREATE SET m.created_at = datetime(), m.is_active = true
                    RETURN m.created_at IS NOT NULL as is_new
                "#.to_string())
                .param("from_id", swipe.from_user_id.to_string())
                .param("to_id", swipe.to_user_id.to_string()))
                .await
                .map_err(|e| AppError::Internal(format!("Neo4j match check error: {}", e)))?;

                if let Ok(Some(row)) = result.next().await {
                    let is_new: bool = row.get("is_new").unwrap_or(false);
                    let (user1_id, user2_id) = if swipe.from_user_id < swipe.to_user_id {
                        (swipe.from_user_id, swipe.to_user_id)
                    } else {
                        (swipe.to_user_id, swipe.from_user_id)
                    };
                    return Ok(Some(MatchResult {
                        user1_id,
                        user2_id,
                        matched_at: Utc::now(),
                        is_new,
                    }));
                }
            }
        }

        Ok(None)
    }

    // -------------------------------------------------------------------------
    // Graph-Powered Recommendations (Neo4j Primary, PostgreSQL Fallback)
    // -------------------------------------------------------------------------

    /// Get friend-of-friend recommendations
    #[instrument(skip(self))]
    pub async fn get_friend_of_friend_recommendations(&self, user_id: Uuid, limit: i32) -> Result<Vec<UserRecommendation>> {
        if self.is_neo4j_healthy().await {
            self.get_fof_neo4j(user_id, limit).await
        } else {
            warn!("Neo4j unavailable, using PostgreSQL fallback for FoF recommendations");
            self.get_fof_postgres_fallback(user_id, limit).await
        }
    }

    async fn get_fof_neo4j(&self, user_id: Uuid, limit: i32) -> Result<Vec<UserRecommendation>> {
        let mut recommendations = Vec::new();

        // Use HTTP API if available
        if let Some(ref http) = self.neo4j_http {
            let cypher = format!(r#"
                MATCH (me:User {{id: '{}'}})-[:MATCHED]-(friend)-[:MATCHED]-(fof:User)
                WHERE NOT (me)-[:MATCHED|BLOCKED|PASSED]-(fof)
                  AND me <> fof
                  AND fof.is_active = true
                WITH fof, count(DISTINCT friend) as mutual_count
                ORDER BY mutual_count DESC
                LIMIT {}
                RETURN fof.id as user_id, mutual_count
            "#, user_id, limit);

            let response = http.execute(&cypher, None).await?;
            if let Some(result) = response.results.first() {
                for row in &result.data {
                    if row.row.len() >= 2 {
                        let user_id_str = row.row[0].as_str().unwrap_or_default();
                        let mutual_count = row.row[1].as_i64().unwrap_or(0);

                        if let Ok(uid) = Uuid::parse_str(user_id_str) {
                            recommendations.push(UserRecommendation {
                                user_id: uid,
                                score: mutual_count as f64 * 10.0,
                                reason: RecommendationReason::FriendOfFriend,
                                mutual_connections: mutual_count as i32,
                                shared_interests: vec![],
                            });
                        }
                    }
                }
            }
        } else if let Some(ref bolt) = self.neo4j {
            let mut result = bolt.execute(Query::new(r#"
                MATCH (me:User {id: $user_id})-[:MATCHED]-(friend)-[:MATCHED]-(fof:User)
                WHERE NOT (me)-[:MATCHED|BLOCKED|PASSED]-(fof)
                  AND me <> fof
                  AND fof.is_active = true
                WITH fof, count(DISTINCT friend) as mutual_count
                ORDER BY mutual_count DESC
                LIMIT $limit
                RETURN fof.id as user_id, mutual_count
            "#.to_string())
            .param("user_id", user_id.to_string())
            .param("limit", limit as i64))
            .await
            .map_err(|e| AppError::Internal(format!("Neo4j FoF query error: {}", e)))?;

            while let Ok(Some(row)) = result.next().await {
                let user_id_str: String = row.get("user_id").unwrap_or_default();
                let mutual_count: i64 = row.get("mutual_count").unwrap_or(0);

                if let Ok(uid) = Uuid::parse_str(&user_id_str) {
                    recommendations.push(UserRecommendation {
                        user_id: uid,
                        score: mutual_count as f64 * 10.0,
                        reason: RecommendationReason::FriendOfFriend,
                        mutual_connections: mutual_count as i32,
                        shared_interests: vec![],
                    });
                }
            }
        }

        Ok(recommendations)
    }

    async fn get_fof_postgres_fallback(&self, user_id: Uuid, limit: i32) -> Result<Vec<UserRecommendation>> {
        // PostgreSQL fallback using JOINs (slower but functional)
        let rows = sqlx::query_as::<_, (Uuid, i64)>(r#"
            WITH my_matches AS (
                SELECT CASE WHEN user1_id = $1 THEN user2_id ELSE user1_id END as match_id
                FROM matches
                WHERE (user1_id = $1 OR user2_id = $1) AND is_active = true
            ),
            fof AS (
                SELECT CASE WHEN m.user1_id = mm.match_id THEN m.user2_id ELSE m.user1_id END as fof_id
                FROM matches m
                JOIN my_matches mm ON (m.user1_id = mm.match_id OR m.user2_id = mm.match_id)
                WHERE m.is_active = true
            )
            SELECT fof_id, count(*) as mutual_count
            FROM fof
            WHERE fof_id != $1
              AND fof_id NOT IN (SELECT match_id FROM my_matches)
              AND fof_id NOT IN (SELECT to_user_id FROM swipes WHERE from_user_id = $1 AND action IN ('pass', 'block'))
            GROUP BY fof_id
            ORDER BY mutual_count DESC
            LIMIT $2
        "#)
        .bind(user_id)
        .bind(limit)
        .fetch_all(&self.postgres)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(|(uid, count)| UserRecommendation {
            user_id: uid,
            score: count as f64 * 10.0,
            reason: RecommendationReason::FriendOfFriend,
            mutual_connections: count as i32,
            shared_interests: vec![],
        }).collect())
    }

    /// Get university network recommendations
    #[instrument(skip(self))]
    pub async fn get_university_recommendations(&self, user_id: Uuid, limit: i32) -> Result<Vec<UserRecommendation>> {
        if self.is_neo4j_healthy().await {
            self.get_university_neo4j(user_id, limit).await
        } else {
            self.get_university_postgres_fallback(user_id, limit).await
        }
    }

    async fn get_university_neo4j(&self, user_id: Uuid, limit: i32) -> Result<Vec<UserRecommendation>> {
        let mut recommendations = Vec::new();

        // Use HTTP API if available
        if let Some(ref http) = self.neo4j_http {
            let cypher = format!(r#"
                MATCH (me:User {{id: '{}'}})-[:STUDIES_AT]->(uni:University)<-[:STUDIES_AT]-(classmate:User)
                WHERE NOT (me)-[:MATCHED|BLOCKED|PASSED]-(classmate)
                  AND me <> classmate
                  AND classmate.is_active = true
                RETURN classmate.id as user_id, uni.name as university_name
                LIMIT {}
            "#, user_id, limit);

            let response = http.execute(&cypher, None).await?;
            if let Some(result) = response.results.first() {
                for row in &result.data {
                    if row.row.len() >= 2 {
                        let user_id_str = row.row[0].as_str().unwrap_or_default();
                        let university = row.row[1].as_str().unwrap_or_default().to_string();

                        if let Ok(uid) = Uuid::parse_str(user_id_str) {
                            recommendations.push(UserRecommendation {
                                user_id: uid,
                                score: 50.0,
                                reason: RecommendationReason::SameUniversity,
                                mutual_connections: 0,
                                shared_interests: vec![university],
                            });
                        }
                    }
                }
            }
        } else if let Some(ref bolt) = self.neo4j {
            let mut result = bolt.execute(Query::new(r#"
                MATCH (me:User {id: $user_id})-[:STUDIES_AT]->(uni:University)<-[:STUDIES_AT]-(classmate:User)
                WHERE NOT (me)-[:MATCHED|BLOCKED|PASSED]-(classmate)
                  AND me <> classmate
                  AND classmate.is_active = true
                RETURN classmate.id as user_id, uni.name as university_name
                LIMIT $limit
            "#.to_string())
            .param("user_id", user_id.to_string())
            .param("limit", limit as i64))
            .await
            .map_err(|e| AppError::Internal(format!("Neo4j university query error: {}", e)))?;

            while let Ok(Some(row)) = result.next().await {
                let user_id_str: String = row.get("user_id").unwrap_or_default();
                let university: String = row.get("university_name").unwrap_or_default();

                if let Ok(uid) = Uuid::parse_str(&user_id_str) {
                    recommendations.push(UserRecommendation {
                        user_id: uid,
                        score: 50.0,
                        reason: RecommendationReason::SameUniversity,
                        mutual_connections: 0,
                        shared_interests: vec![university],
                    });
                }
            }
        }

        Ok(recommendations)
    }

    async fn get_university_postgres_fallback(&self, user_id: Uuid, limit: i32) -> Result<Vec<UserRecommendation>> {
        let rows = sqlx::query_as::<_, (Uuid, String)>(r#"
            SELECT sv2.user_id, u.name as university_name
            FROM student_verifications sv1
            JOIN student_verifications sv2 ON sv1.university_id = sv2.university_id
            JOIN universities u ON sv1.university_id = u.id
            WHERE sv1.user_id = $1
              AND sv2.user_id != $1
              AND sv1.verified = true
              AND sv2.verified = true
              AND sv2.user_id NOT IN (
                  SELECT to_user_id FROM swipes WHERE from_user_id = $1 AND action IN ('pass', 'block')
              )
              AND sv2.user_id NOT IN (
                  SELECT CASE WHEN user1_id = $1 THEN user2_id ELSE user1_id END
                  FROM matches WHERE user1_id = $1 OR user2_id = $1
              )
            LIMIT $2
        "#)
        .bind(user_id)
        .bind(limit)
        .fetch_all(&self.postgres)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(|(uid, uni)| UserRecommendation {
            user_id: uid,
            score: 50.0,
            reason: RecommendationReason::SameUniversity,
            mutual_connections: 0,
            shared_interests: vec![uni],
        }).collect())
    }

    /// Get interest-based recommendations
    #[instrument(skip(self))]
    pub async fn get_interest_recommendations(&self, user_id: Uuid, limit: i32) -> Result<Vec<UserRecommendation>> {
        if self.is_neo4j_healthy().await {
            self.get_interest_neo4j(user_id, limit).await
        } else {
            self.get_interest_postgres_fallback(user_id, limit).await
        }
    }

    async fn get_interest_neo4j(&self, user_id: Uuid, limit: i32) -> Result<Vec<UserRecommendation>> {
        let mut recommendations = Vec::new();

        // Use HTTP API if available
        if let Some(ref http) = self.neo4j_http {
            let cypher = format!(r#"
                MATCH (me:User {{id: '{}'}})-[:INTERESTED_IN]->(i:Interest)<-[:INTERESTED_IN]-(similar:User)
                WHERE NOT (me)-[:MATCHED|BLOCKED|PASSED]-(similar)
                  AND me <> similar
                  AND similar.is_active = true
                WITH similar, count(DISTINCT i) as shared_count, collect(i.name) as interests
                ORDER BY shared_count DESC
                LIMIT {}
                RETURN similar.id as user_id, shared_count, interests
            "#, user_id, limit);

            let response = http.execute(&cypher, None).await?;
            if let Some(result) = response.results.first() {
                for row in &result.data {
                    if row.row.len() >= 3 {
                        let user_id_str = row.row[0].as_str().unwrap_or_default();
                        let shared_count = row.row[1].as_i64().unwrap_or(0);
                        let interests: Vec<String> = row.row[2]
                            .as_array()
                            .map(|arr| arr.iter().filter_map(|v| v.as_str().map(String::from)).collect())
                            .unwrap_or_default();

                        if let Ok(uid) = Uuid::parse_str(user_id_str) {
                            recommendations.push(UserRecommendation {
                                user_id: uid,
                                score: shared_count as f64 * 15.0,
                                reason: RecommendationReason::SharedInterests,
                                mutual_connections: 0,
                                shared_interests: interests,
                            });
                        }
                    }
                }
            }
        } else if let Some(ref bolt) = self.neo4j {
            let mut result = bolt.execute(Query::new(r#"
                MATCH (me:User {id: $user_id})-[:INTERESTED_IN]->(i:Interest)<-[:INTERESTED_IN]-(similar:User)
                WHERE NOT (me)-[:MATCHED|BLOCKED|PASSED]-(similar)
                  AND me <> similar
                  AND similar.is_active = true
                WITH similar, count(DISTINCT i) as shared_count, collect(i.name) as interests
                ORDER BY shared_count DESC
                LIMIT $limit
                RETURN similar.id as user_id, shared_count, interests
            "#.to_string())
            .param("user_id", user_id.to_string())
            .param("limit", limit as i64))
            .await
            .map_err(|e| AppError::Internal(format!("Neo4j interest query error: {}", e)))?;

            while let Ok(Some(row)) = result.next().await {
                let user_id_str: String = row.get("user_id").unwrap_or_default();
                let shared_count: i64 = row.get("shared_count").unwrap_or(0);
                let interests: Vec<String> = row.get("interests").unwrap_or_default();

                if let Ok(uid) = Uuid::parse_str(&user_id_str) {
                    recommendations.push(UserRecommendation {
                        user_id: uid,
                        score: shared_count as f64 * 15.0,
                        reason: RecommendationReason::SharedInterests,
                        mutual_connections: 0,
                        shared_interests: interests,
                    });
                }
            }
        }

        Ok(recommendations)
    }

    async fn get_interest_postgres_fallback(&self, user_id: Uuid, limit: i32) -> Result<Vec<UserRecommendation>> {
        let rows = sqlx::query_as::<_, (Uuid, i64, Vec<String>)>(r#"
            WITH my_interests AS (
                SELECT interest_id FROM user_interests WHERE user_id = $1
            )
            SELECT ui.user_id,
                   count(*) as shared_count,
                   array_agg(i.name) as interests
            FROM user_interests ui
            JOIN my_interests mi ON ui.interest_id = mi.interest_id
            JOIN interests i ON ui.interest_id = i.id
            WHERE ui.user_id != $1
              AND ui.user_id NOT IN (
                  SELECT to_user_id FROM swipes WHERE from_user_id = $1 AND action IN ('pass', 'block')
              )
            GROUP BY ui.user_id
            ORDER BY shared_count DESC
            LIMIT $2
        "#)
        .bind(user_id)
        .bind(limit)
        .fetch_all(&self.postgres)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        Ok(rows.into_iter().map(|(uid, count, interests)| UserRecommendation {
            user_id: uid,
            score: count as f64 * 15.0,
            reason: RecommendationReason::SharedInterests,
            mutual_connections: 0,
            shared_interests: interests,
        }).collect())
    }

    // -------------------------------------------------------------------------
    // Fraud Detection (Neo4j-Powered)
    // -------------------------------------------------------------------------

    /// Detect potential fraud patterns using graph analysis
    #[instrument(skip(self))]
    pub async fn detect_fraud_patterns(&self, user_id: Uuid) -> Result<FraudAnalysis> {
        if !self.is_neo4j_healthy().await {
            return Ok(FraudAnalysis::default());
        }

        let mut analysis = FraudAnalysis::default();

        // Use HTTP API if available
        if let Some(ref http) = self.neo4j_http {
            // Check for circular blocking patterns
            let cypher1 = format!(r#"
                MATCH path = (u:User {{id: '{}'}})-[:BLOCKED*2..4]->(u)
                RETURN count(path) as circular_blocks
            "#, user_id);

            if let Ok(response) = http.execute(&cypher1, None).await {
                if let Some(result) = response.results.first() {
                    if let Some(row) = result.data.first() {
                        if let Some(count) = row.row.first().and_then(|v| v.as_i64()) {
                            analysis.circular_block_patterns = count;
                        }
                    }
                }
            }

            // Check for rapid like patterns
            let cypher2 = format!(r#"
                MATCH (u:User {{id: '{}'}})-[l:LIKED]->()
                WHERE l.created_at > datetime() - duration('PT1H')
                RETURN count(l) as recent_likes
            "#, user_id);

            if let Ok(response) = http.execute(&cypher2, None).await {
                if let Some(result) = response.results.first() {
                    if let Some(row) = result.data.first() {
                        if let Some(count) = row.row.first().and_then(|v| v.as_i64()) {
                            analysis.likes_last_hour = count;
                        }
                    }
                }
            }
        } else if let Some(ref bolt) = self.neo4j {
            // Check for circular blocking patterns (common fraud indicator)
            let mut result = bolt.execute(Query::new(r#"
                MATCH path = (u:User {id: $user_id})-[:BLOCKED*2..4]->(u)
                RETURN count(path) as circular_blocks
            "#.to_string())
            .param("user_id", user_id.to_string()))
            .await
            .map_err(|e| AppError::Internal(format!("Neo4j fraud query error: {}", e)))?;

            if let Ok(Some(row)) = result.next().await {
                analysis.circular_block_patterns = row.get("circular_blocks").unwrap_or(0);
            }

            // Check for rapid like patterns
            let mut result = bolt.execute(Query::new(r#"
                MATCH (u:User {id: $user_id})-[l:LIKED]->()
                WHERE l.created_at > datetime() - duration('PT1H')
                RETURN count(l) as recent_likes
            "#.to_string())
            .param("user_id", user_id.to_string()))
            .await
            .map_err(|e| AppError::Internal(format!("Neo4j fraud query error: {}", e)))?;

            if let Ok(Some(row)) = result.next().await {
                analysis.likes_last_hour = row.get("recent_likes").unwrap_or(0);
            }
        }

        // Calculate fraud score
        analysis.fraud_score = (analysis.circular_block_patterns as f64 * 20.0)
            + if analysis.likes_last_hour > 50 { 30.0 } else { 0.0 };

        Ok(analysis)
    }

    // -------------------------------------------------------------------------
    // Sync Queue Operations
    // -------------------------------------------------------------------------

    async fn queue_postgres_sync(&self, operation: &str, data: &str) {
        // In production, this would write to a persistent queue (Redis, Kafka, etc.)
        warn!("Queuing PostgreSQL sync: {} - {}", operation, &data[..data.len().min(100)]);
    }

    async fn queue_neo4j_sync(&self, operation: &str, data: &str) {
        // In production, this would write to a persistent queue (Redis, Kafka, etc.)
        warn!("Queuing Neo4j sync: {} - {}", operation, &data[..data.len().min(100)]);
    }

    // -------------------------------------------------------------------------
    // Full Sync Operations (Background Job)
    // -------------------------------------------------------------------------

    /// Sync all users from PostgreSQL to Neo4j
    #[instrument(skip(self))]
    pub async fn sync_users_to_neo4j(&self) -> Result<SyncResult> {
        let mut synced = 0;
        let mut errors = 0;

        let users = sqlx::query_as::<_, (i32, String, Option<String>, Option<String>, bool, bool, bool, chrono::NaiveDateTime)>(r#"
            SELECT u.id, u.phone_number, u.name, u.gender, COALESCE(u.is_verified, false) as is_verified,
                   COALESCE(u.is_student_verified, false) as is_student, COALESCE(u.is_active, true) as is_active, u.created_at
            FROM users u
        "#)
        .fetch_all(&self.postgres)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        for (id_i32, phone, name, gender, is_verified, is_student, is_active, created_at_naive) in users {
            let id = Uuid::from_u128(id_i32 as u128);
            let created_at = DateTime::<Utc>::from_naive_utc_and_offset(created_at_naive, Utc);
            let is_premium = false; // No is_premium column in users table
            let user = UserNode {
                id,
                phone,
                name,
                gender,
                date_of_birth: None,
                bio: None,
                latitude: None,
                longitude: None,
                city: None,
                is_verified,
                is_premium,
                is_student,
                is_active,
                created_at,
                last_active_at: None,
            };

            match self.upsert_user_neo4j(&user).await {
                Ok(_) => synced += 1,
                Err(e) => {
                    error!("Failed to sync user {} to Neo4j: {}", id, e);
                    errors += 1;
                }
            }
        }

        Ok(SyncResult { synced, errors, sync_type: "users".to_string() })
    }

    /// Sync all swipes/matches from PostgreSQL to Neo4j
    #[instrument(skip(self))]
    pub async fn sync_relationships_to_neo4j(&self) -> Result<SyncResult> {
        let mut synced = 0;
        let mut errors = 0;

        // Sync swipes (using BIGINT ids)
        let swipes = sqlx::query_as::<_, (i64, i64, String, chrono::NaiveDateTime)>(r#"
            SELECT from_user_id, to_user_id, action, created_at FROM swipes
        "#)
        .fetch_all(&self.postgres)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        for (from_id_i64, to_id_i64, action, created_at_naive) in swipes {
            let from_id = Uuid::from_u128(from_id_i64 as u128);
            let to_id = Uuid::from_u128(to_id_i64 as u128);
            let created_at = DateTime::<Utc>::from_naive_utc_and_offset(created_at_naive, Utc);
            let swipe_type = match action.as_str() {
                "like" => SwipeType::Like,
                "pass" => SwipeType::Pass,
                "block" => SwipeType::Block,
                _ => continue,
            };

            let swipe = SwipeAction {
                from_user_id: from_id,
                to_user_id: to_id,
                action: swipe_type,
                source: "sync".to_string(),
                created_at,
            };

            match self.record_swipe_neo4j(&swipe).await {
                Ok(_) => synced += 1,
                Err(e) => {
                    error!("Failed to sync swipe to Neo4j: {}", e);
                    errors += 1;
                }
            }
        }

        Ok(SyncResult { synced, errors, sync_type: "relationships".to_string() })
    }

    /// Sync universities to Neo4j
    #[instrument(skip(self))]
    pub async fn sync_universities_to_neo4j(&self) -> Result<SyncResult> {
        let mut synced = 0;
        let mut errors = 0;

        let universities = sqlx::query_as::<_, (i64, String, String, String, String, String)>(r#"
            SELECT id, name, COALESCE(email_domain, domain) as email_domain, country_code, tier, COALESCE(university_type, tier) as university_type FROM universities
        "#)
        .fetch_all(&self.postgres)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;

        for (id, name, email_domain, country_code, tier, uni_type) in universities {
            // Use HTTP API if available
            if let Some(ref http) = self.neo4j_http {
                let cypher = format!(r#"
                    MERGE (u:University {{id: {}}})
                    SET u.name = '{}',
                        u.email_domain = '{}',
                        u.country_code = '{}',
                        u.tier = '{}',
                        u.university_type = '{}'
                    WITH u
                    MERGE (c:Country {{code: '{}'}})
                    MERGE (u)-[:LOCATED_IN]->(c)
                "#, id, name.replace('\'', "''"), email_domain.replace('\'', "''"),
                    country_code.replace('\'', "''"), tier.replace('\'', "''"),
                    uni_type.replace('\'', "''"), country_code.replace('\'', "''"));

                match http.execute(&cypher, None).await {
                    Ok(_) => synced += 1,
                    Err(e) => {
                        error!("Failed to sync university to Neo4j: {}", e);
                        errors += 1;
                    }
                }
            } else if let Some(ref bolt) = self.neo4j {
                let query = Query::new(r#"
                    MERGE (u:University {id: $id})
                    SET u.name = $name,
                        u.email_domain = $email_domain,
                        u.country_code = $country_code,
                        u.tier = $tier,
                        u.university_type = $uni_type
                    WITH u
                    MERGE (c:Country {code: $country_code})
                    MERGE (u)-[:LOCATED_IN]->(c)
                "#.to_string())
                .param("id", id as i64)
                .param("name", name)
                .param("email_domain", email_domain)
                .param("country_code", country_code.clone())
                .param("tier", tier)
                .param("uni_type", uni_type);

                match bolt.run(query).await {
                    Ok(_) => synced += 1,
                    Err(e) => {
                        error!("Failed to sync university to Neo4j: {}", e);
                        errors += 1;
                    }
                }
            }
        }

        Ok(SyncResult { synced, errors, sync_type: "universities".to_string() })
    }
}

// -----------------------------------------------------------------------------
// Additional Types
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct FraudAnalysis {
    pub fraud_score: f64,
    pub circular_block_patterns: i64,
    pub likes_last_hour: i64,
    pub suspicious_patterns: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct SyncResult {
    pub synced: i32,
    pub errors: i32,
    pub sync_type: String,
}

// -----------------------------------------------------------------------------
// Serde implementations for queue serialization
// -----------------------------------------------------------------------------

impl serde::Serialize for UserNode {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("UserNode", 15)?;
        state.serialize_field("id", &self.id.to_string())?;
        state.serialize_field("phone", &self.phone)?;
        state.serialize_field("name", &self.name)?;
        state.serialize_field("gender", &self.gender)?;
        state.serialize_field("is_verified", &self.is_verified)?;
        state.serialize_field("is_premium", &self.is_premium)?;
        state.serialize_field("is_student", &self.is_student)?;
        state.serialize_field("is_active", &self.is_active)?;
        state.end()
    }
}

impl serde::Serialize for SwipeAction {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("SwipeAction", 5)?;
        state.serialize_field("from_user_id", &self.from_user_id.to_string())?;
        state.serialize_field("to_user_id", &self.to_user_id.to_string())?;
        state.serialize_field("action", &self.action.to_string())?;
        state.serialize_field("source", &self.source)?;
        state.serialize_field("created_at", &self.created_at.to_rfc3339())?;
        state.end()
    }
}
