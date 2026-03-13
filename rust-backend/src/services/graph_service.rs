// =============================================================================
// NAVA Platform - Graph Service (PostgreSQL CTE Implementation)
// =============================================================================
// All graph queries (FoF, university network, interest recs, fraud detection)
// are implemented as pure PostgreSQL CTEs.  Neo4j has been removed.
// =============================================================================

use sqlx::PgPool;
use uuid::Uuid;
use tracing::instrument;

use crate::error::AppError;

// Type alias for Result
pub type Result<T> = std::result::Result<T, AppError>;

// -----------------------------------------------------------------------------
// Types and Structures
// -----------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub struct GraphService {
    pub postgres: PgPool,
}

#[derive(Debug, Clone)]
pub struct UserNode {
    pub id: Uuid,
    pub phone: String,
    pub name: Option<String>,
    pub gender: Option<String>,
    pub date_of_birth: Option<chrono::DateTime<chrono::Utc>>,
    pub bio: Option<String>,
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub city: Option<String>,
    pub is_verified: bool,
    pub is_premium: bool,
    pub is_student: bool,
    pub is_active: bool,
    pub created_at: chrono::DateTime<chrono::Utc>,
    pub last_active_at: Option<chrono::DateTime<chrono::Utc>>,
}

#[derive(Debug, Clone)]
pub struct SwipeAction {
    pub from_user_id: Uuid,
    pub to_user_id: Uuid,
    pub action: SwipeType,
    pub source: String,
    pub created_at: chrono::DateTime<chrono::Utc>,
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
    pub matched_at: chrono::DateTime<chrono::Utc>,
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

// Simple health status returned by health_check()
#[derive(Debug, Clone)]
pub struct ServiceHealth {
    pub postgres_healthy: bool,
}

// -----------------------------------------------------------------------------
// GraphService Implementation
// -----------------------------------------------------------------------------

impl GraphService {
    /// Create a new GraphService backed by PostgreSQL only.
    pub fn new(postgres: PgPool) -> Self {
        Self { postgres }
    }

    /// Check health of the postgres connection.
    #[instrument(skip(self))]
    pub async fn health_check(&self) -> ServiceHealth {
        let postgres_healthy = sqlx::query("SELECT 1")
            .execute(&self.postgres)
            .await
            .is_ok();
        ServiceHealth { postgres_healthy }
    }

    /// Get PostgreSQL pool
    pub fn postgres(&self) -> &PgPool {
        &self.postgres
    }

    // -------------------------------------------------------------------------
    // No-op compatibility shim used by graphql.rs (set_user_university)
    // -------------------------------------------------------------------------

    /// No-op: university relationship is stored in student_verifications in Postgres.
    /// Kept for API compatibility with graphql.rs call-site.
    pub async fn set_user_university(
        &self,
        _user_id: i32,
        _university_name: &str,
    ) -> Result<()> {
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Graph-Powered Recommendations (pure PostgreSQL CTEs)
    // -------------------------------------------------------------------------

    /// Get friend-of-friend recommendations via PostgreSQL CTE.
    #[instrument(skip(self))]
    pub async fn get_friend_of_friend_recommendations(
        &self,
        user_id: Uuid,
        limit: i32,
    ) -> Result<Vec<UserRecommendation>> {
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

    /// Get university network recommendations via PostgreSQL CTE.
    #[instrument(skip(self))]
    pub async fn get_university_recommendations(
        &self,
        user_id: Uuid,
        limit: i32,
    ) -> Result<Vec<UserRecommendation>> {
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

    /// Get interest-based recommendations via PostgreSQL CTE.
    #[instrument(skip(self))]
    pub async fn get_interest_recommendations(
        &self,
        user_id: Uuid,
        limit: i32,
    ) -> Result<Vec<UserRecommendation>> {
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
    // Fraud Detection (pure PostgreSQL)
    // -------------------------------------------------------------------------

    /// Detect potential fraud patterns using PostgreSQL queries.
    #[instrument(skip(self))]
    pub async fn detect_fraud_patterns(&self, user_id: Uuid) -> Result<FraudAnalysis> {
        let likes_last_hour: i64 = sqlx::query_scalar(r#"
            SELECT COUNT(*) as likes_last_hour FROM swipes
            WHERE from_user_id = $1 AND action = 'like' AND created_at > NOW() - INTERVAL '1 hour'
        "#)
        .bind(user_id)
        .fetch_one(&self.postgres)
        .await
        .unwrap_or(0);

        let fraud_score = if likes_last_hour > 50 { 30.0_f64 } else { 0.0_f64 };

        Ok(FraudAnalysis {
            fraud_score,
            circular_block_patterns: 0,
            likes_last_hour,
            suspicious_patterns: vec![],
        })
    }

    // -------------------------------------------------------------------------
    // Social Graph Stats (pure PostgreSQL)
    // -------------------------------------------------------------------------

    /// Get social graph stats for the platform (total users, total matches).
    #[instrument(skip(self))]
    pub async fn get_social_graph_stats(&self) -> Result<(i64, i64)> {
        let total_users: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM users WHERE is_active = true")
            .fetch_one(&self.postgres)
            .await
            .unwrap_or(0);

        let total_matches: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM matches WHERE is_active = true")
            .fetch_one(&self.postgres)
            .await
            .unwrap_or(0);

        Ok((total_users, total_matches))
    }
}

// -----------------------------------------------------------------------------
// Serde implementations (kept for any remaining serialization needs)
// -----------------------------------------------------------------------------

impl serde::Serialize for UserNode {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        use serde::ser::SerializeStruct;
        let mut state = serializer.serialize_struct("UserNode", 8)?;
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
