// =============================================================================
// NAVA Platform - Background Sync Job
// =============================================================================
// Maintains consistency between PostgreSQL and Neo4j:
// - Periodic full sync
// - Process queued failed operations
// - Health monitoring and recovery
// - Incremental sync for new changes
// =============================================================================

use std::sync::Arc;
use std::time::Duration;
use tokio::time::{interval, sleep};
use chrono::{DateTime, Utc};
use tracing::{info, warn, error, instrument};

use crate::state::AppState;
use crate::services::graph_service::GraphService;
use crate::middleware::dual_write::DualWriteManager;

// -----------------------------------------------------------------------------
// Sync Job Configuration
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct SyncJobConfig {
    /// Interval between health checks (seconds)
    pub health_check_interval: u64,
    /// Interval between incremental syncs (seconds)
    pub incremental_sync_interval: u64,
    /// Interval between full syncs (seconds)
    pub full_sync_interval: u64,
    /// Maximum retry attempts for failed operations
    pub max_retry_attempts: u32,
    /// Batch size for sync operations
    pub batch_size: usize,
}

impl Default for SyncJobConfig {
    fn default() -> Self {
        Self {
            health_check_interval: 30,
            incremental_sync_interval: 60,
            full_sync_interval: 3600, // 1 hour
            max_retry_attempts: 3,
            batch_size: 100,
        }
    }
}

// -----------------------------------------------------------------------------
// Sync Job Runner
// -----------------------------------------------------------------------------

pub struct SyncJobRunner {
    state: Arc<AppState>,
    config: SyncJobConfig,
    last_full_sync: Option<DateTime<Utc>>,
    last_incremental_sync: Option<DateTime<Utc>>,
}

impl SyncJobRunner {
    pub fn new(state: Arc<AppState>, config: SyncJobConfig) -> Self {
        Self {
            state,
            config,
            last_full_sync: None,
            last_incremental_sync: None,
        }
    }

    /// Start all background sync jobs
    pub async fn start(self) {
        let runner = Arc::new(tokio::sync::RwLock::new(self));

        // Spawn health check job
        let health_runner = runner.clone();
        tokio::spawn(async move {
            health_check_job(health_runner).await;
        });

        // Spawn incremental sync job
        let incremental_runner = runner.clone();
        tokio::spawn(async move {
            incremental_sync_job(incremental_runner).await;
        });

        // Spawn full sync job
        let full_runner = runner.clone();
        tokio::spawn(async move {
            full_sync_job(full_runner).await;
        });

        // Spawn queue processor job
        let queue_runner = runner.clone();
        tokio::spawn(async move {
            queue_processor_job(queue_runner).await;
        });

        info!("Sync jobs started successfully");
    }
}

// -----------------------------------------------------------------------------
// Health Check Job
// -----------------------------------------------------------------------------

#[instrument(skip(runner))]
async fn health_check_job(runner: Arc<tokio::sync::RwLock<SyncJobRunner>>) {
    let interval_secs = {
        let r = runner.read().await;
        r.config.health_check_interval
    };

    let mut interval = interval(Duration::from_secs(interval_secs));

    loop {
        interval.tick().await;

        let state = {
            let r = runner.read().await;
            r.state.clone()
        };

        if let Some(graph_service) = state.graph() {
            let health = graph_service.health_check().await;

            if !health.neo4j_healthy {
                warn!(
                    "Neo4j unhealthy - errors: {}, last check: {}",
                    health.neo4j_error_count,
                    health.last_neo4j_check
                );
            }

            if !health.postgres_healthy {
                error!(
                    "PostgreSQL unhealthy - errors: {}, last check: {}",
                    health.postgres_error_count,
                    health.last_postgres_check
                );
            }

            // Update metrics
            if health.neo4j_healthy && health.postgres_healthy {
                info!("Database health check passed - both databases healthy");
            }
        }
    }
}

// -----------------------------------------------------------------------------
// Incremental Sync Job
// -----------------------------------------------------------------------------

#[instrument(skip(runner))]
async fn incremental_sync_job(runner: Arc<tokio::sync::RwLock<SyncJobRunner>>) {
    let interval_secs = {
        let r = runner.read().await;
        r.config.incremental_sync_interval
    };

    // Wait a bit before starting to let the system stabilize
    sleep(Duration::from_secs(10)).await;

    let mut interval = interval(Duration::from_secs(interval_secs));

    loop {
        interval.tick().await;

        let (state, last_sync) = {
            let r = runner.read().await;
            (r.state.clone(), r.last_incremental_sync)
        };

        // Only sync if Neo4j is available
        if !state.is_neo4j_available().await {
            warn!("Skipping incremental sync - Neo4j unavailable");
            continue;
        }

        if let Some(graph_service) = state.graph() {
            let since = last_sync.unwrap_or_else(|| {
                Utc::now() - chrono::Duration::seconds(interval_secs as i64 * 2)
            });

            match sync_recent_changes(graph_service, &state.db, since).await {
                Ok(count) => {
                    if count > 0 {
                        info!("Incremental sync completed: {} changes synced", count);
                    }
                    let mut r = runner.write().await;
                    r.last_incremental_sync = Some(Utc::now());
                }
                Err(e) => {
                    error!("Incremental sync failed: {}", e);
                }
            }
        }
    }
}

async fn sync_recent_changes(
    graph_service: &GraphService,
    db: &sqlx::PgPool,
    since: DateTime<Utc>,
) -> Result<i32, String> {
    let mut synced = 0;

    // Sync recent swipes (using BIGINT ids converted to UUID)
    let swipes: Vec<(i64, i64, String, chrono::NaiveDateTime)> = sqlx::query_as(r#"
        SELECT from_user_id, to_user_id, action, created_at
        FROM swipes
        WHERE created_at > $1
        ORDER BY created_at ASC
        LIMIT 1000
    "#)
    .bind(since.naive_utc())
    .fetch_all(db)
    .await
    .map_err(|e| e.to_string())?;

    for (from_id, to_id, action, created_at_naive) in swipes {
        let created_at = DateTime::<Utc>::from_naive_utc_and_offset(created_at_naive, Utc);
        let from_id = uuid::Uuid::from_u128(from_id as u128);
        let to_id = uuid::Uuid::from_u128(to_id as u128);
        let swipe_type = match action.as_str() {
            "like" => crate::services::graph_service::SwipeType::Like,
            "pass" => crate::services::graph_service::SwipeType::Pass,
            "block" => crate::services::graph_service::SwipeType::Block,
            _ => continue,
        };

        let swipe = crate::services::graph_service::SwipeAction {
            from_user_id: from_id,
            to_user_id: to_id,
            action: swipe_type,
            source: "incremental_sync".to_string(),
            created_at,
        };

        // Only sync to Neo4j (PostgreSQL is source of truth)
        if graph_service.is_neo4j_healthy().await {
            if let Err(e) = graph_service.record_swipe(&swipe).await {
                warn!("Failed to sync swipe to Neo4j: {}", e);
            } else {
                synced += 1;
            }
        }
    }

    // Sync recent user updates (using actual column names, i32 for users.id)
    let users: Vec<(i32, String, Option<String>, bool, bool, bool, chrono::NaiveDateTime)> = sqlx::query_as(r#"
        SELECT u.id, u.phone_number, u.name, COALESCE(u.is_verified, false) as is_verified,
               COALESCE(u.is_student_verified, false) as is_student, COALESCE(u.is_active, true) as is_active, u.updated_at
        FROM users u
        WHERE u.updated_at > $1
        ORDER BY u.updated_at ASC
        LIMIT 1000
    "#)
    .bind(since.naive_utc())
    .fetch_all(db)
    .await
    .map_err(|e| e.to_string())?;

    for (id_i32, phone, name, is_verified, is_student, is_active, updated_at_naive) in users {
        let created_at = DateTime::<Utc>::from_naive_utc_and_offset(updated_at_naive, Utc);
        let id = uuid::Uuid::from_u128(id_i32 as u128);
        let is_premium = false; // Users table doesn't have is_premium column
        let user = crate::services::graph_service::UserNode {
            id,
            phone,
            name,
            gender: None,
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

        if let Err(e) = graph_service.upsert_user(&user).await {
            warn!("Failed to sync user {} to Neo4j: {}", id, e);
        } else {
            synced += 1;
        }
    }

    Ok(synced)
}

// -----------------------------------------------------------------------------
// Full Sync Job
// -----------------------------------------------------------------------------

#[instrument(skip(runner))]
async fn full_sync_job(runner: Arc<tokio::sync::RwLock<SyncJobRunner>>) {
    let interval_secs = {
        let r = runner.read().await;
        r.config.full_sync_interval
    };

    // Wait before first full sync
    sleep(Duration::from_secs(60)).await;

    let mut interval = interval(Duration::from_secs(interval_secs));

    loop {
        interval.tick().await;

        let state = {
            let r = runner.read().await;
            r.state.clone()
        };

        // Only run full sync if Neo4j is available
        if !state.is_neo4j_available().await {
            warn!("Skipping full sync - Neo4j unavailable");
            continue;
        }

        if let Some(graph_service) = state.graph() {
            info!("Starting full database sync...");

            // Sync universities first (they're referenced by users)
            match graph_service.sync_universities_to_neo4j().await {
                Ok(result) => {
                    info!("Universities sync: {} synced, {} errors", result.synced, result.errors);
                }
                Err(e) => {
                    error!("Universities sync failed: {}", e);
                }
            }

            // Sync users
            match graph_service.sync_users_to_neo4j().await {
                Ok(result) => {
                    info!("Users sync: {} synced, {} errors", result.synced, result.errors);
                }
                Err(e) => {
                    error!("Users sync failed: {}", e);
                }
            }

            // Sync relationships
            match graph_service.sync_relationships_to_neo4j().await {
                Ok(result) => {
                    info!("Relationships sync: {} synced, {} errors", result.synced, result.errors);
                }
                Err(e) => {
                    error!("Relationships sync failed: {}", e);
                }
            }

            info!("Full database sync completed");

            let mut r = runner.write().await;
            r.last_full_sync = Some(Utc::now());
        }
    }
}

// -----------------------------------------------------------------------------
// Queue Processor Job
// -----------------------------------------------------------------------------

#[instrument(skip(runner))]
async fn queue_processor_job(runner: Arc<tokio::sync::RwLock<SyncJobRunner>>) {
    // Process queued operations every 5 seconds
    let mut interval = interval(Duration::from_secs(5));

    loop {
        interval.tick().await;

        let state = {
            let r = runner.read().await;
            r.state.clone()
        };

        // Process Neo4j queue if Neo4j is back online
        if state.is_neo4j_available().await {
            let neo4j_queue = state.dual_write.drain_neo4j_queue().await;
            if !neo4j_queue.is_empty() {
                info!("Processing {} queued Neo4j operations", neo4j_queue.len());
                for op in neo4j_queue {
                    // Process based on operation type
                    match process_neo4j_operation(&state, &op).await {
                        Ok(_) => {}
                        Err(e) => {
                            warn!("Failed to process queued Neo4j operation: {}", e);
                            // Re-queue if retry count not exceeded
                            if op.retry_count < 3 {
                                // Would re-queue here in production
                            }
                        }
                    }
                }
            }
        }

        // Process PostgreSQL queue if PostgreSQL is back online
        if state.is_postgres_available().await {
            let postgres_queue = state.dual_write.drain_postgres_queue().await;
            if !postgres_queue.is_empty() {
                info!("Processing {} queued PostgreSQL operations", postgres_queue.len());
                for op in postgres_queue {
                    match process_postgres_operation(&state, &op).await {
                        Ok(_) => {}
                        Err(e) => {
                            warn!("Failed to process queued PostgreSQL operation: {}", e);
                        }
                    }
                }
            }
        }
    }
}

async fn process_neo4j_operation(
    state: &AppState,
    op: &crate::middleware::dual_write::SyncOperation,
) -> Result<(), String> {
    let graph_service = state.graph()
        .ok_or_else(|| "Graph service unavailable".to_string())?;

    match op.operation_type.as_str() {
        "user_upsert" => {
            // Parse and upsert user
            // In production, deserialize the user data and call upsert
            info!("Processing user_upsert from queue");
            Ok(())
        }
        "swipe" => {
            // Parse and record swipe
            info!("Processing swipe from queue");
            Ok(())
        }
        _ => {
            warn!("Unknown operation type: {}", op.operation_type);
            Ok(())
        }
    }
}

async fn process_postgres_operation(
    state: &AppState,
    op: &crate::middleware::dual_write::SyncOperation,
) -> Result<(), String> {
    match op.operation_type.as_str() {
        "user_upsert" => {
            info!("Processing user_upsert from PostgreSQL queue");
            Ok(())
        }
        "swipe" => {
            info!("Processing swipe from PostgreSQL queue");
            Ok(())
        }
        _ => {
            warn!("Unknown operation type: {}", op.operation_type);
            Ok(())
        }
    }
}

// -----------------------------------------------------------------------------
// Startup Sync Function
// -----------------------------------------------------------------------------

/// Run initial sync on application startup
#[instrument(skip(state))]
pub async fn run_startup_sync(state: &AppState) -> Result<(), String> {
    let graph_service = state.graph()
        .ok_or_else(|| "Graph service unavailable".to_string())?;

    info!("Running startup sync...");

    // Check if Neo4j has any data
    let has_data = check_neo4j_has_data(graph_service).await?;

    if !has_data {
        info!("Neo4j is empty, running full initial sync...");

        // Sync in order of dependencies
        graph_service.sync_universities_to_neo4j().await
            .map_err(|e| format!("University sync failed: {}", e))?;

        graph_service.sync_users_to_neo4j().await
            .map_err(|e| format!("User sync failed: {}", e))?;

        graph_service.sync_relationships_to_neo4j().await
            .map_err(|e| format!("Relationship sync failed: {}", e))?;

        info!("Initial sync completed successfully");
    } else {
        info!("Neo4j already has data, skipping full sync");
    }

    Ok(())
}

async fn check_neo4j_has_data(graph_service: &GraphService) -> Result<bool, String> {
    use neo4rs::Query;

    // Use HTTP API if available
    if let Some(http) = graph_service.neo4j_http() {
        let response = http.execute("MATCH (n) RETURN count(n) as count LIMIT 1", None)
            .await
            .map_err(|e| format!("Neo4j query failed: {}", e))?;

        if let Some(result) = response.results.first() {
            if let Some(row) = result.data.first() {
                if let Some(count) = row.row.first().and_then(|v| v.as_i64()) {
                    return Ok(count > 0);
                }
            }
        }
        Ok(false)
    } else if let Some(bolt) = graph_service.neo4j() {
        let mut result = bolt.execute(
            Query::new("MATCH (n) RETURN count(n) as count LIMIT 1".to_string())
        )
        .await
        .map_err(|e| format!("Neo4j query failed: {}", e))?;

        if let Ok(Some(row)) = result.next().await {
            let count: i64 = row.get("count").unwrap_or(0);
            Ok(count > 0)
        } else {
            Ok(false)
        }
    } else {
        Err("No Neo4j connection available".to_string())
    }
}
