//! ClickHouse OLAP storage — ported from microservices/analytics-service/storage.rs.
//!
//! Stores all domain events in a columnar OLAP engine with materialized views
//! for DAU and hourly event counts.

use chrono::{DateTime, Utc};
use serde_json::Value;
use tracing::{error, info, warn};

pub struct ClickHouseStorage {
    client: reqwest::Client,
    base_url: String,
    database: String,
}

impl ClickHouseStorage {
    pub async fn new() -> Self {
        let base_url =
            std::env::var("CLICKHOUSE_URL").unwrap_or_else(|_| "http://localhost:8123".to_string());
        let database =
            std::env::var("CLICKHOUSE_DATABASE").unwrap_or_else(|_| "nava_analytics".to_string());

        let storage = Self {
            client: reqwest::Client::new(),
            base_url,
            database,
        };

        if let Err(e) = storage.init_tables().await {
            warn!("Failed to initialize ClickHouse tables: {}", e);
        }

        storage
    }

    async fn init_tables(&self) -> Result<(), String> {
        self.execute_query(&format!("CREATE DATABASE IF NOT EXISTS {}", self.database))
            .await?;

        // Events table with monthly partitioning and 365-day TTL
        let create_events_table = format!(
            r#"
            CREATE TABLE IF NOT EXISTS {}.events (
                event_id String,
                event_type String,
                timestamp DateTime64(3),
                user_id Nullable(Int32),
                source_service String,
                correlation_id String,
                data String,
                received_at DateTime64(3) DEFAULT now64(3)
            )
            ENGINE = MergeTree()
            PARTITION BY toYYYYMM(timestamp)
            ORDER BY (event_type, timestamp, event_id)
            TTL timestamp + INTERVAL 365 DAY
            "#,
            self.database
        );
        self.execute_query(&create_events_table).await?;

        // Materialized view: daily active users
        let create_dau_mv = format!(
            r#"
            CREATE MATERIALIZED VIEW IF NOT EXISTS {db}.daily_active_users
            ENGINE = SummingMergeTree()
            PARTITION BY toYYYYMM(date)
            ORDER BY (date)
            AS SELECT
                toDate(timestamp) as date,
                uniqExact(user_id) as dau
            FROM {db}.events
            WHERE user_id IS NOT NULL
            GROUP BY date
            "#,
            db = self.database
        );
        self.execute_query(&create_dau_mv).await.ok();

        // Materialized view: hourly event counts
        let create_event_counts_mv = format!(
            r#"
            CREATE MATERIALIZED VIEW IF NOT EXISTS {db}.event_counts_hourly
            ENGINE = SummingMergeTree()
            PARTITION BY toYYYYMM(hour)
            ORDER BY (event_type, hour)
            AS SELECT
                event_type,
                toStartOfHour(timestamp) as hour,
                count() as count
            FROM {db}.events
            GROUP BY event_type, hour
            "#,
            db = self.database
        );
        self.execute_query(&create_event_counts_mv).await.ok();

        info!("ClickHouse tables initialized");
        Ok(())
    }

    pub async fn store_event(
        &self,
        event_id: &str,
        event_type: &str,
        timestamp: &DateTime<Utc>,
        data: &Value,
        source_service: &str,
    ) -> Result<(), String> {
        let user_id = data.get("user_id").and_then(|v| v.as_i64()).map(|v| v as i32);
        let data_json = serde_json::to_string(data).map_err(|e| e.to_string())?;

        let query = format!(
            r#"
            INSERT INTO {}.events
            (event_id, event_type, timestamp, user_id, source_service, correlation_id, data)
            VALUES ('{}', '{}', '{}', {}, '{}', '', '{}')
            "#,
            self.database,
            escape_string(event_id),
            escape_string(event_type),
            timestamp.format("%Y-%m-%d %H:%M:%S%.3f"),
            user_id.map(|id| id.to_string()).unwrap_or_else(|| "NULL".to_string()),
            escape_string(source_service),
            escape_string(&data_json)
        );

        self.execute_query(&query).await
    }

    async fn execute_query(&self, query: &str) -> Result<(), String> {
        let response = self
            .client
            .post(&self.base_url)
            .body(query.to_string())
            .send()
            .await
            .map_err(|e| format!("ClickHouse request failed: {}", e))?;

        if response.status().is_success() {
            Ok(())
        } else {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            error!(status = %status, body = %body, "ClickHouse query failed");
            Err(format!("ClickHouse returned {}: {}", status, body))
        }
    }
}

fn escape_string(s: &str) -> String {
    s.replace('\\', "\\\\").replace('\'', "\\'")
}
