//! Offset store for tracking consumer progress
//!
//! Provides persistent offset storage in PostgreSQL for exactly-once
//! processing semantics and consumer recovery.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sqlx::PgPool;
use tracing::{error, info, warn};

/// Offset record stored in database
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OffsetRecord {
    pub consumer_group: String,
    pub topic: String,
    pub partition: i32,
    pub offset: i64,
    pub committed_at: DateTime<Utc>,
}

/// PostgreSQL-backed offset store for consumer state
pub struct OffsetStore {
    pool: PgPool,
}

impl OffsetStore {
    /// Create a new offset store
    pub async fn new(pool: PgPool) -> Result<Self, sqlx::Error> {
        let store = Self { pool };
        store.ensure_table().await?;
        Ok(store)
    }

    /// Ensure the offsets table exists
    async fn ensure_table(&self) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS kafka_consumer_offsets (
                consumer_group VARCHAR(255) NOT NULL,
                topic VARCHAR(255) NOT NULL,
                partition INTEGER NOT NULL,
                offset_value BIGINT NOT NULL,
                committed_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                PRIMARY KEY (consumer_group, topic, partition)
            )
            "#,
        )
        .execute(&self.pool)
        .await?;

        // Create index for faster lookups
        sqlx::query(
            r#"
            CREATE INDEX IF NOT EXISTS idx_kafka_offsets_group_topic
            ON kafka_consumer_offsets (consumer_group, topic)
            "#,
        )
        .execute(&self.pool)
        .await?;

        info!("Kafka offset store table ensured");
        Ok(())
    }

    /// Get the last committed offset for a partition
    pub async fn get_offset(
        &self,
        consumer_group: &str,
        topic: &str,
        partition: i32,
    ) -> Result<Option<i64>, sqlx::Error> {
        let result = sqlx::query_scalar::<_, i64>(
            r#"
            SELECT offset_value
            FROM kafka_consumer_offsets
            WHERE consumer_group = $1 AND topic = $2 AND partition = $3
            "#,
        )
        .bind(consumer_group)
        .bind(topic)
        .bind(partition)
        .fetch_optional(&self.pool)
        .await?;

        Ok(result)
    }

    /// Get all offsets for a consumer group
    pub async fn get_group_offsets(
        &self,
        consumer_group: &str,
    ) -> Result<Vec<OffsetRecord>, sqlx::Error> {
        let rows = sqlx::query_as::<_, (String, String, i32, i64, DateTime<Utc>)>(
            r#"
            SELECT consumer_group, topic, partition, offset_value, committed_at
            FROM kafka_consumer_offsets
            WHERE consumer_group = $1
            ORDER BY topic, partition
            "#,
        )
        .bind(consumer_group)
        .fetch_all(&self.pool)
        .await?;

        Ok(rows
            .into_iter()
            .map(|(consumer_group, topic, partition, offset, committed_at)| OffsetRecord {
                consumer_group,
                topic,
                partition,
                offset,
                committed_at,
            })
            .collect())
    }

    /// Commit an offset (upsert)
    pub async fn commit_offset(
        &self,
        consumer_group: &str,
        topic: &str,
        partition: i32,
        offset: i64,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            r#"
            INSERT INTO kafka_consumer_offsets (consumer_group, topic, partition, offset_value, committed_at)
            VALUES ($1, $2, $3, $4, NOW())
            ON CONFLICT (consumer_group, topic, partition)
            DO UPDATE SET
                offset_value = EXCLUDED.offset_value,
                committed_at = EXCLUDED.committed_at
            "#,
        )
        .bind(consumer_group)
        .bind(topic)
        .bind(partition)
        .bind(offset)
        .execute(&self.pool)
        .await?;

        Ok(())
    }

    /// Commit multiple offsets in a transaction
    pub async fn commit_offsets_batch(
        &self,
        offsets: &[(String, String, i32, i64)], // (group, topic, partition, offset)
    ) -> Result<(), sqlx::Error> {
        let mut tx = self.pool.begin().await?;

        for (consumer_group, topic, partition, offset) in offsets {
            sqlx::query(
                r#"
                INSERT INTO kafka_consumer_offsets (consumer_group, topic, partition, offset_value, committed_at)
                VALUES ($1, $2, $3, $4, NOW())
                ON CONFLICT (consumer_group, topic, partition)
                DO UPDATE SET
                    offset_value = EXCLUDED.offset_value,
                    committed_at = EXCLUDED.committed_at
                "#,
            )
            .bind(consumer_group)
            .bind(topic)
            .bind(partition)
            .bind(offset)
            .execute(&mut *tx)
            .await?;
        }

        tx.commit().await?;
        Ok(())
    }

    /// Reset offset to a specific value (for reprocessing)
    pub async fn reset_offset(
        &self,
        consumer_group: &str,
        topic: &str,
        partition: i32,
        offset: i64,
    ) -> Result<(), sqlx::Error> {
        self.commit_offset(consumer_group, topic, partition, offset)
            .await
    }

    /// Delete all offsets for a consumer group (for reset)
    pub async fn delete_group_offsets(&self, consumer_group: &str) -> Result<u64, sqlx::Error> {
        let result = sqlx::query(
            r#"
            DELETE FROM kafka_consumer_offsets
            WHERE consumer_group = $1
            "#,
        )
        .bind(consumer_group)
        .execute(&self.pool)
        .await?;

        Ok(result.rows_affected())
    }

    /// Get consumer group lag (difference between latest and committed offset)
    /// Note: This requires knowing the latest offset from Kafka
    pub async fn calculate_lag(
        &self,
        consumer_group: &str,
        topic: &str,
        partition: i32,
        latest_offset: i64,
    ) -> Result<i64, sqlx::Error> {
        let committed = self
            .get_offset(consumer_group, topic, partition)
            .await?
            .unwrap_or(0);

        Ok(latest_offset - committed)
    }
}

/// Offset checkpoint for batch processing
#[derive(Debug, Clone)]
pub struct OffsetCheckpoint {
    pending_offsets: std::collections::HashMap<(String, i32), i64>, // (topic, partition) -> offset
    consumer_group: String,
}

impl OffsetCheckpoint {
    pub fn new(consumer_group: impl Into<String>) -> Self {
        Self {
            pending_offsets: std::collections::HashMap::new(),
            consumer_group: consumer_group.into(),
        }
    }

    /// Mark an offset as processed (but not committed)
    pub fn mark_processed(&mut self, topic: &str, partition: i32, offset: i64) {
        let key = (topic.to_string(), partition);
        // Only update if this offset is higher
        if let Some(existing) = self.pending_offsets.get(&key) {
            if offset > *existing {
                self.pending_offsets.insert(key, offset);
            }
        } else {
            self.pending_offsets.insert(key, offset);
        }
    }

    /// Flush all pending offsets to the store
    pub async fn flush(&mut self, store: &OffsetStore) -> Result<(), sqlx::Error> {
        if self.pending_offsets.is_empty() {
            return Ok(());
        }

        let offsets: Vec<(String, String, i32, i64)> = self
            .pending_offsets
            .iter()
            .map(|((topic, partition), offset)| {
                (
                    self.consumer_group.clone(),
                    topic.clone(),
                    *partition,
                    *offset,
                )
            })
            .collect();

        store.commit_offsets_batch(&offsets).await?;
        self.pending_offsets.clear();

        Ok(())
    }

    /// Get number of pending offsets
    pub fn pending_count(&self) -> usize {
        self.pending_offsets.len()
    }
}
