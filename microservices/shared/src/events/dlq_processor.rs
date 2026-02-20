//! Dead Letter Queue (DLQ) processor for retrying failed events
//!
//! Provides automatic retry with exponential backoff, circuit breaker,
//! and deadlock prevention for failed Kafka messages.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::Arc;
use std::time::Duration;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tokio::sync::{Mutex, RwLock, Semaphore};
use tokio::time::timeout;
use tracing::{error, info, warn};

use super::producer::{EventProducer, ProducerConfig};
use super::types::DlqEvent;

/// DLQ entry with retry metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DlqEntry {
    pub id: i64,
    pub original_topic: String,
    pub original_partition: i32,
    pub original_offset: i64,
    pub original_payload: String,
    pub error_message: String,
    pub retry_count: i32,
    pub max_retries: i32,
    pub first_failed_at: DateTime<Utc>,
    pub last_failed_at: DateTime<Utc>,
    pub next_retry_at: Option<DateTime<Utc>>,
    pub status: DlqStatus,
}

/// DLQ entry status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, sqlx::Type)]
#[sqlx(type_name = "dlq_status", rename_all = "lowercase")]
pub enum DlqStatus {
    Pending,
    Processing,
    Retried,
    Failed,
    Discarded,
}

impl std::fmt::Display for DlqStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DlqStatus::Pending => write!(f, "pending"),
            DlqStatus::Processing => write!(f, "processing"),
            DlqStatus::Retried => write!(f, "retried"),
            DlqStatus::Failed => write!(f, "failed"),
            DlqStatus::Discarded => write!(f, "discarded"),
        }
    }
}

/// Circuit breaker state for preventing cascading failures
#[derive(Debug)]
pub struct CircuitBreaker {
    failure_count: AtomicU32,
    last_failure: RwLock<Option<DateTime<Utc>>>,
    is_open: AtomicBool,
    failure_threshold: u32,
    reset_timeout: Duration,
}

impl CircuitBreaker {
    pub fn new(failure_threshold: u32, reset_timeout: Duration) -> Self {
        Self {
            failure_count: AtomicU32::new(0),
            last_failure: RwLock::new(None),
            is_open: AtomicBool::new(false),
            failure_threshold,
            reset_timeout,
        }
    }

    pub async fn record_failure(&self) {
        let count = self.failure_count.fetch_add(1, Ordering::SeqCst) + 1;
        *self.last_failure.write().await = Some(Utc::now());

        if count >= self.failure_threshold {
            self.is_open.store(true, Ordering::SeqCst);
            warn!(
                failure_count = count,
                threshold = self.failure_threshold,
                "Circuit breaker opened"
            );
        }
    }

    pub fn record_success(&self) {
        self.failure_count.store(0, Ordering::SeqCst);
        if self.is_open.swap(false, Ordering::SeqCst) {
            info!("Circuit breaker closed after success");
        }
    }

    pub async fn is_available(&self) -> bool {
        if !self.is_open.load(Ordering::SeqCst) {
            return true;
        }

        // Check if we should try half-open
        if let Some(last) = *self.last_failure.read().await {
            let elapsed = Utc::now().signed_duration_since(last);
            if elapsed.to_std().unwrap_or(Duration::ZERO) >= self.reset_timeout {
                info!("Circuit breaker entering half-open state");
                return true;
            }
        }

        false
    }
}

/// DLQ processor configuration
#[derive(Debug, Clone)]
pub struct DlqProcessorConfig {
    /// Maximum concurrent retries
    pub max_concurrent: usize,
    /// Base delay for exponential backoff (ms)
    pub base_delay_ms: u64,
    /// Maximum delay cap (ms)
    pub max_delay_ms: u64,
    /// Maximum retries before marking as failed
    pub max_retries: i32,
    /// Processing timeout per message (ms)
    pub processing_timeout_ms: u64,
    /// Batch size for fetching entries
    pub batch_size: i32,
    /// Circuit breaker failure threshold
    pub circuit_breaker_threshold: u32,
    /// Circuit breaker reset timeout (seconds)
    pub circuit_breaker_reset_secs: u64,
}

impl Default for DlqProcessorConfig {
    fn default() -> Self {
        Self {
            max_concurrent: 10,
            base_delay_ms: 1000,
            max_delay_ms: 300000, // 5 minutes
            max_retries: 5,
            processing_timeout_ms: 30000, // 30 seconds
            batch_size: 100,
            circuit_breaker_threshold: 10,
            circuit_breaker_reset_secs: 60,
        }
    }
}

/// DLQ processor for retrying failed messages
pub struct DlqProcessor {
    pool: PgPool,
    producer: EventProducer,
    config: DlqProcessorConfig,
    semaphore: Arc<Semaphore>,
    circuit_breakers: RwLock<HashMap<String, Arc<CircuitBreaker>>>,
    processing_lock: Mutex<()>,
}

impl DlqProcessor {
    /// Create a new DLQ processor
    pub async fn new(pool: PgPool, config: DlqProcessorConfig) -> Result<Self, String> {
        // Create producer for republishing
        let producer_config = ProducerConfig::from_env();
        let producer = EventProducer::new(producer_config, "dlq-processor")
            .map_err(|e| e.to_string())?;

        let processor = Self {
            pool,
            producer,
            semaphore: Arc::new(Semaphore::new(config.max_concurrent)),
            circuit_breakers: RwLock::new(HashMap::new()),
            processing_lock: Mutex::new(()),
            config,
        };

        processor.ensure_tables().await?;
        Ok(processor)
    }

    /// Ensure DLQ tables exist
    async fn ensure_tables(&self) -> Result<(), String> {
        // Create status enum type if not exists
        sqlx::query(
            r#"
            DO $$ BEGIN
                CREATE TYPE dlq_status AS ENUM ('pending', 'processing', 'retried', 'failed', 'discarded');
            EXCEPTION
                WHEN duplicate_object THEN null;
            END $$
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS kafka_dlq (
                id BIGSERIAL PRIMARY KEY,
                original_topic VARCHAR(255) NOT NULL,
                original_partition INTEGER NOT NULL,
                original_offset BIGINT NOT NULL,
                original_payload TEXT NOT NULL,
                error_message TEXT NOT NULL,
                retry_count INTEGER NOT NULL DEFAULT 0,
                max_retries INTEGER NOT NULL DEFAULT 5,
                first_failed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                last_failed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                next_retry_at TIMESTAMPTZ,
                status dlq_status NOT NULL DEFAULT 'pending',
                created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                UNIQUE(original_topic, original_partition, original_offset)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        // Create indexes
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_dlq_status_retry
            ON kafka_dlq (status, next_retry_at)
            WHERE status = 'pending'
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_dlq_topic
            ON kafka_dlq (original_topic)
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        info!("DLQ tables ensured");
        Ok(())
    }

    /// Add an event to the DLQ
    pub async fn add_to_dlq(&self, event: DlqEvent) -> Result<i64, String> {
        let next_retry_at = self.calculate_next_retry(0);

        let id = sqlx::query_scalar::<_, i64>(
            r#"
            INSERT INTO kafka_dlq (
                original_topic, original_partition, original_offset,
                original_payload, error_message, retry_count,
                max_retries, first_failed_at, last_failed_at,
                next_retry_at, status
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, 'pending')
            ON CONFLICT (original_topic, original_partition, original_offset)
            DO UPDATE SET
                retry_count = kafka_dlq.retry_count + 1,
                last_failed_at = EXCLUDED.last_failed_at,
                error_message = EXCLUDED.error_message,
                next_retry_at = $10,
                updated_at = NOW()
            RETURNING id
            "#,
        )
        .bind(&event.original_topic)
        .bind(event.original_partition)
        .bind(event.original_offset)
        .bind(&event.original_payload)
        .bind(&event.error_message)
        .bind(event.retry_count)
        .bind(self.config.max_retries)
        .bind(event.first_failed_at)
        .bind(event.last_failed_at)
        .bind(next_retry_at)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        info!(id, topic = %event.original_topic, "Added event to DLQ");
        Ok(id)
    }

    /// Calculate next retry time with exponential backoff and jitter
    fn calculate_next_retry(&self, retry_count: i32) -> DateTime<Utc> {
        let base = self.config.base_delay_ms;
        let max = self.config.max_delay_ms;

        // Exponential backoff: base * 2^retry_count
        let delay_ms = std::cmp::min(base * 2u64.pow(retry_count as u32), max);

        // Add jitter (±20%)
        let jitter = (rand::random::<f64>() - 0.5) * 0.4 * delay_ms as f64;
        let final_delay = (delay_ms as f64 + jitter).max(base as f64) as i64;

        Utc::now() + chrono::Duration::milliseconds(final_delay)
    }

    /// Get circuit breaker for a topic
    async fn get_circuit_breaker(&self, topic: &str) -> Arc<CircuitBreaker> {
        {
            let breakers = self.circuit_breakers.read().await;
            if let Some(cb) = breakers.get(topic) {
                return cb.clone();
            }
        }

        let mut breakers = self.circuit_breakers.write().await;
        breakers
            .entry(topic.to_string())
            .or_insert_with(|| {
                Arc::new(CircuitBreaker::new(
                    self.config.circuit_breaker_threshold,
                    Duration::from_secs(self.config.circuit_breaker_reset_secs),
                ))
            })
            .clone()
    }

    /// Process pending DLQ entries
    pub async fn process_pending(&self) -> Result<ProcessingStats, String> {
        // Acquire processing lock to prevent concurrent batch processing
        // This prevents deadlocks when multiple instances try to process
        let _lock = self.processing_lock.lock().await;

        let mut stats = ProcessingStats::default();

        // Fetch pending entries that are ready for retry
        let entries = sqlx::query_as::<_, (i64, String, i32, i64, String, String, i32, i32)>(
            r#"
            SELECT id, original_topic, original_partition, original_offset,
                   original_payload, error_message, retry_count, max_retries
            FROM kafka_dlq
            WHERE status = 'pending'
              AND (next_retry_at IS NULL OR next_retry_at <= NOW())
            ORDER BY next_retry_at ASC NULLS FIRST
            LIMIT $1
            FOR UPDATE SKIP LOCKED
            "#,
        )
        .bind(self.config.batch_size)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        stats.fetched = entries.len();

        for (id, topic, partition, offset, payload, error, retry_count, max_retries) in entries {
            // Check circuit breaker
            let circuit_breaker = self.get_circuit_breaker(&topic).await;
            if !circuit_breaker.is_available().await {
                stats.circuit_broken += 1;
                continue;
            }

            // Check if max retries exceeded
            if retry_count >= max_retries {
                self.mark_failed(id).await?;
                stats.max_retries_exceeded += 1;
                continue;
            }

            // Acquire semaphore permit for concurrency control
            let permit = match self.semaphore.clone().try_acquire_owned() {
                Ok(p) => p,
                Err(_) => {
                    stats.concurrency_limited += 1;
                    continue;
                }
            };

            // Mark as processing
            self.mark_processing(id).await?;

            // Try to republish with timeout
            let result = timeout(
                Duration::from_millis(self.config.processing_timeout_ms),
                self.republish_event(&topic, &payload),
            )
            .await;

            match result {
                Ok(Ok(_)) => {
                    self.mark_retried(id).await?;
                    circuit_breaker.record_success();
                    stats.succeeded += 1;
                    info!(id, topic = %topic, "DLQ entry successfully retried");
                }
                Ok(Err(e)) => {
                    circuit_breaker.record_failure().await;
                    let next_retry = self.calculate_next_retry(retry_count + 1);
                    self.increment_retry(id, &e, next_retry).await?;
                    stats.failed += 1;
                    warn!(id, topic = %topic, error = %e, "DLQ retry failed");
                }
                Err(_) => {
                    circuit_breaker.record_failure().await;
                    let next_retry = self.calculate_next_retry(retry_count + 1);
                    self.increment_retry(id, "Timeout during republish", next_retry)
                        .await?;
                    stats.timeouts += 1;
                    warn!(id, topic = %topic, "DLQ retry timed out");
                }
            }

            drop(permit);
        }

        Ok(stats)
    }

    /// Republish an event to its original topic
    async fn republish_event(&self, topic: &str, payload: &str) -> Result<(), String> {
        // Parse the payload to extract event_id for partition key
        let event_id = serde_json::from_str::<serde_json::Value>(payload)
            .ok()
            .and_then(|v| v.get("event_id").and_then(|e| e.as_str()).map(String::from));

        // Re-publish raw payload
        self.producer
            .publish(topic, "dlq.retry", payload, event_id.as_deref())
            .await
            .map_err(|e| e.to_string())?;

        Ok(())
    }

    async fn mark_processing(&self, id: i64) -> Result<(), String> {
        sqlx::query(
            "UPDATE kafka_dlq SET status = 'processing', updated_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn mark_retried(&self, id: i64) -> Result<(), String> {
        sqlx::query("UPDATE kafka_dlq SET status = 'retried', updated_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn mark_failed(&self, id: i64) -> Result<(), String> {
        sqlx::query("UPDATE kafka_dlq SET status = 'failed', updated_at = NOW() WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| e.to_string())?;
        Ok(())
    }

    async fn increment_retry(
        &self,
        id: i64,
        error: &str,
        next_retry_at: DateTime<Utc>,
    ) -> Result<(), String> {
        sqlx::query(
            r#"
            UPDATE kafka_dlq
            SET retry_count = retry_count + 1,
                error_message = $2,
                next_retry_at = $3,
                last_failed_at = NOW(),
                status = 'pending',
                updated_at = NOW()
            WHERE id = $1
            "#,
        )
        .bind(id)
        .bind(error)
        .bind(next_retry_at)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;
        Ok(())
    }

    /// Get DLQ statistics
    pub async fn get_stats(&self) -> Result<DlqStats, String> {
        let row = sqlx::query_as::<_, (i64, i64, i64, i64, i64)>(
            r#"
            SELECT
                COUNT(*) FILTER (WHERE status = 'pending') as pending,
                COUNT(*) FILTER (WHERE status = 'processing') as processing,
                COUNT(*) FILTER (WHERE status = 'retried') as retried,
                COUNT(*) FILTER (WHERE status = 'failed') as failed,
                COUNT(*) FILTER (WHERE status = 'discarded') as discarded
            FROM kafka_dlq
            "#,
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(DlqStats {
            pending: row.0 as u64,
            processing: row.1 as u64,
            retried: row.2 as u64,
            failed: row.3 as u64,
            discarded: row.4 as u64,
        })
    }

    /// Discard old failed entries
    pub async fn cleanup_old_entries(&self, older_than_days: i32) -> Result<u64, String> {
        let result = sqlx::query(
            r#"
            DELETE FROM kafka_dlq
            WHERE status IN ('retried', 'failed', 'discarded')
              AND updated_at < NOW() - INTERVAL '1 day' * $1
            "#,
        )
        .bind(older_than_days)
        .execute(&self.pool)
        .await
        .map_err(|e| e.to_string())?;

        Ok(result.rows_affected())
    }
}

/// Statistics from a processing run
#[derive(Debug, Default)]
pub struct ProcessingStats {
    pub fetched: usize,
    pub succeeded: usize,
    pub failed: usize,
    pub timeouts: usize,
    pub max_retries_exceeded: usize,
    pub circuit_broken: usize,
    pub concurrency_limited: usize,
}

/// DLQ statistics
#[derive(Debug, Clone, Serialize)]
pub struct DlqStats {
    pub pending: u64,
    pub processing: u64,
    pub retried: u64,
    pub failed: u64,
    pub discarded: u64,
}

/// Run the DLQ processor loop
pub async fn run_dlq_processor(
    pool: PgPool,
    config: DlqProcessorConfig,
    shutdown: tokio::sync::watch::Receiver<bool>,
) -> Result<(), String> {
    let processor = DlqProcessor::new(pool, config).await?;
    let mut shutdown = shutdown;

    info!("DLQ processor started");

    loop {
        // Check for shutdown
        if *shutdown.borrow() {
            info!("DLQ processor shutting down");
            break;
        }

        // Process pending entries
        match processor.process_pending().await {
            Ok(stats) => {
                if stats.fetched > 0 {
                    info!(
                        fetched = stats.fetched,
                        succeeded = stats.succeeded,
                        failed = stats.failed,
                        timeouts = stats.timeouts,
                        "DLQ processing batch completed"
                    );
                }
            }
            Err(e) => {
                error!(error = %e, "DLQ processing error");
            }
        }

        // Wait before next batch, with shutdown check
        tokio::select! {
            _ = tokio::time::sleep(Duration::from_secs(5)) => {}
            _ = shutdown.changed() => {
                if *shutdown.borrow() {
                    info!("DLQ processor shutting down");
                    break;
                }
            }
        }
    }

    Ok(())
}
