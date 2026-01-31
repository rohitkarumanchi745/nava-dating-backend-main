// =============================================================================
// NAVA Platform - Dual-Write Middleware
// =============================================================================
// Ensures fault-tolerant writes to both PostgreSQL and Neo4j:
// - Automatic failover when one database is unavailable
// - Queues failed writes for later sync
// - Health monitoring and circuit breaker pattern
// =============================================================================

use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use chrono::{DateTime, Utc};
use tracing::{info, warn, error, instrument};

// -----------------------------------------------------------------------------
// Circuit Breaker Implementation
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct CircuitBreaker {
    state: Arc<RwLock<CircuitState>>,
    config: CircuitBreakerConfig,
}

#[derive(Debug, Clone)]
struct CircuitState {
    status: CircuitStatus,
    failure_count: u32,
    success_count: u32,
    last_failure_time: Option<DateTime<Utc>>,
    last_state_change: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitStatus {
    Closed,     // Normal operation
    Open,       // Failing, reject requests
    HalfOpen,   // Testing if service recovered
}

#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub success_threshold: u32,
    pub timeout: Duration,
    pub half_open_max_requests: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            success_threshold: 3,
            timeout: Duration::from_secs(30),
            half_open_max_requests: 3,
        }
    }
}

impl CircuitBreaker {
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: Arc::new(RwLock::new(CircuitState {
                status: CircuitStatus::Closed,
                failure_count: 0,
                success_count: 0,
                last_failure_time: None,
                last_state_change: Utc::now(),
            })),
            config,
        }
    }

    /// Check if requests should be allowed through
    pub async fn can_execute(&self) -> bool {
        let mut state = self.state.write().await;

        match state.status {
            CircuitStatus::Closed => true,
            CircuitStatus::Open => {
                // Check if timeout has elapsed
                if let Some(last_failure) = state.last_failure_time {
                    let elapsed = Utc::now().signed_duration_since(last_failure);
                    if elapsed.num_seconds() >= self.config.timeout.as_secs() as i64 {
                        info!("Circuit breaker transitioning to half-open");
                        state.status = CircuitStatus::HalfOpen;
                        state.success_count = 0;
                        state.last_state_change = Utc::now();
                        true
                    } else {
                        false
                    }
                } else {
                    false
                }
            }
            CircuitStatus::HalfOpen => {
                // Allow limited requests in half-open state
                state.success_count < self.config.half_open_max_requests
            }
        }
    }

    /// Record a successful operation
    pub async fn record_success(&self) {
        let mut state = self.state.write().await;

        match state.status {
            CircuitStatus::HalfOpen => {
                state.success_count += 1;
                if state.success_count >= self.config.success_threshold {
                    info!("Circuit breaker closing after successful recovery");
                    state.status = CircuitStatus::Closed;
                    state.failure_count = 0;
                    state.success_count = 0;
                    state.last_state_change = Utc::now();
                }
            }
            CircuitStatus::Closed => {
                state.failure_count = 0; // Reset failure count on success
            }
            _ => {}
        }
    }

    /// Record a failed operation
    pub async fn record_failure(&self) {
        let mut state = self.state.write().await;

        state.failure_count += 1;
        state.last_failure_time = Some(Utc::now());

        match state.status {
            CircuitStatus::Closed => {
                if state.failure_count >= self.config.failure_threshold {
                    warn!("Circuit breaker opening due to {} failures", state.failure_count);
                    state.status = CircuitStatus::Open;
                    state.last_state_change = Utc::now();
                }
            }
            CircuitStatus::HalfOpen => {
                warn!("Circuit breaker re-opening after failure in half-open state");
                state.status = CircuitStatus::Open;
                state.last_state_change = Utc::now();
            }
            _ => {}
        }
    }

    /// Get current circuit status
    pub async fn status(&self) -> CircuitStatus {
        self.state.read().await.status
    }

    /// Get circuit statistics
    pub async fn stats(&self) -> CircuitStats {
        let state = self.state.read().await;
        CircuitStats {
            status: state.status,
            failure_count: state.failure_count,
            success_count: state.success_count,
            last_failure_time: state.last_failure_time,
            last_state_change: state.last_state_change,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CircuitStats {
    pub status: CircuitStatus,
    pub failure_count: u32,
    pub success_count: u32,
    pub last_failure_time: Option<DateTime<Utc>>,
    pub last_state_change: DateTime<Utc>,
}

// -----------------------------------------------------------------------------
// Dual-Write Manager
// -----------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct DualWriteManager {
    pub neo4j_circuit: CircuitBreaker,
    pub postgres_circuit: CircuitBreaker,
    pub sync_queue: Arc<RwLock<SyncQueue>>,
}

#[derive(Debug, Default)]
pub struct SyncQueue {
    pub neo4j_pending: Vec<SyncOperation>,
    pub postgres_pending: Vec<SyncOperation>,
}

#[derive(Debug, Clone)]
pub struct SyncOperation {
    pub operation_type: String,
    pub data: String,
    pub created_at: DateTime<Utc>,
    pub retry_count: u32,
}

impl DualWriteManager {
    pub fn new() -> Self {
        Self {
            neo4j_circuit: CircuitBreaker::new(CircuitBreakerConfig::default()),
            postgres_circuit: CircuitBreaker::new(CircuitBreakerConfig::default()),
            sync_queue: Arc::new(RwLock::new(SyncQueue::default())),
        }
    }

    /// Check if Neo4j is available for writes
    pub async fn can_write_neo4j(&self) -> bool {
        self.neo4j_circuit.can_execute().await
    }

    /// Check if PostgreSQL is available for writes
    pub async fn can_write_postgres(&self) -> bool {
        self.postgres_circuit.can_execute().await
    }

    /// Execute a dual-write operation with automatic failover
    #[instrument(skip(self, neo4j_op, postgres_op))]
    pub async fn execute_dual_write<F1, F2, R1, R2>(
        &self,
        neo4j_op: F1,
        postgres_op: F2,
        operation_name: &str,
        fallback_data: &str,
    ) -> DualWriteResult<R1, R2>
    where
        F1: std::future::Future<Output = Result<R1, String>>,
        F2: std::future::Future<Output = Result<R2, String>>,
    {
        let mut result = DualWriteResult::default();

        // Execute PostgreSQL write
        if self.can_write_postgres().await {
            match postgres_op.await {
                Ok(r) => {
                    self.postgres_circuit.record_success().await;
                    result.postgres_result = Some(Ok(r));
                }
                Err(e) => {
                    error!("PostgreSQL write failed: {}", e);
                    self.postgres_circuit.record_failure().await;
                    result.postgres_result = Some(Err(e.clone()));
                    self.queue_postgres_operation(operation_name, fallback_data).await;
                }
            }
        } else {
            warn!("PostgreSQL circuit open, queuing operation");
            self.queue_postgres_operation(operation_name, fallback_data).await;
            result.postgres_queued = true;
        }

        // Execute Neo4j write
        if self.can_write_neo4j().await {
            match neo4j_op.await {
                Ok(r) => {
                    self.neo4j_circuit.record_success().await;
                    result.neo4j_result = Some(Ok(r));
                }
                Err(e) => {
                    error!("Neo4j write failed: {}", e);
                    self.neo4j_circuit.record_failure().await;
                    result.neo4j_result = Some(Err(e.clone()));
                    self.queue_neo4j_operation(operation_name, fallback_data).await;
                }
            }
        } else {
            warn!("Neo4j circuit open, queuing operation");
            self.queue_neo4j_operation(operation_name, fallback_data).await;
            result.neo4j_queued = true;
        }

        result
    }

    /// Queue a Neo4j operation for later sync
    async fn queue_neo4j_operation(&self, operation_type: &str, data: &str) {
        let mut queue = self.sync_queue.write().await;
        queue.neo4j_pending.push(SyncOperation {
            operation_type: operation_type.to_string(),
            data: data.to_string(),
            created_at: Utc::now(),
            retry_count: 0,
        });

        // Limit queue size to prevent memory issues
        if queue.neo4j_pending.len() > 10000 {
            warn!("Neo4j sync queue full, dropping oldest operations");
            queue.neo4j_pending.drain(0..1000);
        }
    }

    /// Queue a PostgreSQL operation for later sync
    async fn queue_postgres_operation(&self, operation_type: &str, data: &str) {
        let mut queue = self.sync_queue.write().await;
        queue.postgres_pending.push(SyncOperation {
            operation_type: operation_type.to_string(),
            data: data.to_string(),
            created_at: Utc::now(),
            retry_count: 0,
        });

        // Limit queue size
        if queue.postgres_pending.len() > 10000 {
            warn!("PostgreSQL sync queue full, dropping oldest operations");
            queue.postgres_pending.drain(0..1000);
        }
    }

    /// Get pending sync operations count
    pub async fn pending_sync_count(&self) -> (usize, usize) {
        let queue = self.sync_queue.read().await;
        (queue.neo4j_pending.len(), queue.postgres_pending.len())
    }

    /// Get full status report
    pub async fn status_report(&self) -> DualWriteStatus {
        let neo4j_stats = self.neo4j_circuit.stats().await;
        let postgres_stats = self.postgres_circuit.stats().await;
        let (neo4j_pending, postgres_pending) = self.pending_sync_count().await;

        DualWriteStatus {
            neo4j_circuit: neo4j_stats,
            postgres_circuit: postgres_stats,
            neo4j_pending_sync: neo4j_pending,
            postgres_pending_sync: postgres_pending,
        }
    }

    /// Drain pending operations for processing
    pub async fn drain_neo4j_queue(&self) -> Vec<SyncOperation> {
        let mut queue = self.sync_queue.write().await;
        std::mem::take(&mut queue.neo4j_pending)
    }

    /// Drain PostgreSQL pending operations
    pub async fn drain_postgres_queue(&self) -> Vec<SyncOperation> {
        let mut queue = self.sync_queue.write().await;
        std::mem::take(&mut queue.postgres_pending)
    }
}

impl Default for DualWriteManager {
    fn default() -> Self {
        Self::new()
    }
}

// -----------------------------------------------------------------------------
// Result Types
// -----------------------------------------------------------------------------

#[derive(Debug)]
pub struct DualWriteResult<R1, R2> {
    pub neo4j_result: Option<Result<R1, String>>,
    pub postgres_result: Option<Result<R2, String>>,
    pub neo4j_queued: bool,
    pub postgres_queued: bool,
}

impl<R1, R2> Default for DualWriteResult<R1, R2> {
    fn default() -> Self {
        Self {
            neo4j_result: None,
            postgres_result: None,
            neo4j_queued: false,
            postgres_queued: false,
        }
    }
}

impl<R1, R2> DualWriteResult<R1, R2> {
    /// Check if at least one write succeeded
    pub fn any_success(&self) -> bool {
        matches!(&self.neo4j_result, Some(Ok(_))) ||
        matches!(&self.postgres_result, Some(Ok(_)))
    }

    /// Check if both writes succeeded
    pub fn all_success(&self) -> bool {
        matches!(&self.neo4j_result, Some(Ok(_))) &&
        matches!(&self.postgres_result, Some(Ok(_)))
    }

    /// Check if any operations were queued
    pub fn has_queued(&self) -> bool {
        self.neo4j_queued || self.postgres_queued
    }
}

#[derive(Debug, Clone)]
pub struct DualWriteStatus {
    pub neo4j_circuit: CircuitStats,
    pub postgres_circuit: CircuitStats,
    pub neo4j_pending_sync: usize,
    pub postgres_pending_sync: usize,
}

// -----------------------------------------------------------------------------
// Read Strategy Types
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ReadStrategy {
    /// Prefer Neo4j, fallback to PostgreSQL
    PreferNeo4j,
    /// Prefer PostgreSQL, fallback to Neo4j
    PreferPostgres,
    /// Read from both and merge (for consistency checks)
    ReadBoth,
    /// Read only from Neo4j (graph-specific queries)
    Neo4jOnly,
    /// Read only from PostgreSQL (relational queries)
    PostgresOnly,
}

impl Default for ReadStrategy {
    fn default() -> Self {
        Self::PreferPostgres
    }
}

// -----------------------------------------------------------------------------
// Middleware Layer Implementation
// -----------------------------------------------------------------------------

use axum::{
    extract::State,
    middleware::Next,
    response::Response,
    http::Request,
    body::Body,
};

/// Middleware to inject dual-write status into request extensions
pub async fn dual_write_middleware(
    State(manager): State<Arc<DualWriteManager>>,
    mut request: Request<Body>,
    next: Next,
) -> Response {
    // Add circuit breaker status to request extensions
    let neo4j_available = manager.can_write_neo4j().await;
    let postgres_available = manager.can_write_postgres().await;

    request.extensions_mut().insert(DatabaseAvailability {
        neo4j: neo4j_available,
        postgres: postgres_available,
    });

    next.run(request).await
}

#[derive(Debug, Clone, Copy)]
pub struct DatabaseAvailability {
    pub neo4j: bool,
    pub postgres: bool,
}

// -----------------------------------------------------------------------------
// Health Check Endpoint Types
// -----------------------------------------------------------------------------

#[derive(Debug, Clone, serde::Serialize)]
pub struct HealthResponse {
    pub status: String,
    pub neo4j: DatabaseHealthInfo,
    pub postgres: DatabaseHealthInfo,
    pub sync_queue: SyncQueueInfo,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct DatabaseHealthInfo {
    pub available: bool,
    pub circuit_status: String,
    pub failure_count: u32,
    pub last_failure: Option<String>,
}

#[derive(Debug, Clone, serde::Serialize)]
pub struct SyncQueueInfo {
    pub neo4j_pending: usize,
    pub postgres_pending: usize,
}

impl From<DualWriteStatus> for HealthResponse {
    fn from(status: DualWriteStatus) -> Self {
        let neo4j_available = status.neo4j_circuit.status != CircuitStatus::Open;
        let postgres_available = status.postgres_circuit.status != CircuitStatus::Open;

        let overall_status = if neo4j_available && postgres_available {
            "healthy"
        } else if neo4j_available || postgres_available {
            "degraded"
        } else {
            "unhealthy"
        };

        Self {
            status: overall_status.to_string(),
            neo4j: DatabaseHealthInfo {
                available: neo4j_available,
                circuit_status: format!("{:?}", status.neo4j_circuit.status),
                failure_count: status.neo4j_circuit.failure_count,
                last_failure: status.neo4j_circuit.last_failure_time.map(|t| t.to_rfc3339()),
            },
            postgres: DatabaseHealthInfo {
                available: postgres_available,
                circuit_status: format!("{:?}", status.postgres_circuit.status),
                failure_count: status.postgres_circuit.failure_count,
                last_failure: status.postgres_circuit.last_failure_time.map(|t| t.to_rfc3339()),
            },
            sync_queue: SyncQueueInfo {
                neo4j_pending: status.neo4j_pending_sync,
                postgres_pending: status.postgres_pending_sync,
            },
        }
    }
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_circuit_breaker_opens_after_failures() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 3,
            ..Default::default()
        });

        assert!(cb.can_execute().await);
        assert_eq!(cb.status().await, CircuitStatus::Closed);

        // Record failures
        for _ in 0..3 {
            cb.record_failure().await;
        }

        assert_eq!(cb.status().await, CircuitStatus::Open);
        assert!(!cb.can_execute().await);
    }

    #[tokio::test]
    async fn test_circuit_breaker_recovers_on_success() {
        let cb = CircuitBreaker::new(CircuitBreakerConfig {
            failure_threshold: 2,
            success_threshold: 2,
            timeout: Duration::from_millis(10),
            half_open_max_requests: 3,
        });

        // Open the circuit
        cb.record_failure().await;
        cb.record_failure().await;
        assert_eq!(cb.status().await, CircuitStatus::Open);

        // Wait for timeout
        tokio::time::sleep(Duration::from_millis(20)).await;

        // Should transition to half-open
        assert!(cb.can_execute().await);
        assert_eq!(cb.status().await, CircuitStatus::HalfOpen);

        // Record successes to close
        cb.record_success().await;
        cb.record_success().await;
        assert_eq!(cb.status().await, CircuitStatus::Closed);
    }

    #[tokio::test]
    async fn test_dual_write_manager_queuing() {
        let manager = DualWriteManager::new();

        // Queue some operations
        manager.queue_neo4j_operation("test", "data").await;
        manager.queue_postgres_operation("test", "data").await;

        let (neo4j_count, postgres_count) = manager.pending_sync_count().await;
        assert_eq!(neo4j_count, 1);
        assert_eq!(postgres_count, 1);
    }
}
