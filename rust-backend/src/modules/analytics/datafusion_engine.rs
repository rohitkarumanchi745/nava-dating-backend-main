//! DataFusion SQL analytics engine.
//!
//! Loads Postgres tables into Arrow record-batches and exposes them as
//! DataFusion tables for fast in-process SQL analytics.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use datafusion::prelude::*;
use datafusion::arrow::array::*;
use datafusion::arrow::datatypes::{DataType, Field, Schema};
use datafusion::arrow::record_batch::RecordBatch;
use sqlx::PgPool;
use tracing::info;

/// In-process SQL analytics engine powered by Apache DataFusion.
pub struct DataFusionEngine {
    db: PgPool,
    ctx: SessionContext,
    last_refresh: Option<DateTime<Utc>>,
}

impl DataFusionEngine {
    pub fn new(db: PgPool) -> Self {
        Self {
            db,
            ctx: SessionContext::new(),
            last_refresh: None,
        }
    }

    pub fn last_refresh(&self) -> Option<DateTime<Utc>> {
        self.last_refresh
    }

    /// Pull recent data from Postgres into DataFusion memory tables.
    pub async fn refresh(&mut self, lookback_hours: i64) -> Result<(), String> {
        let cutoff = Utc::now() - chrono::Duration::hours(lookback_hours);
        let cutoff_str = cutoff.format("%Y-%m-%d %H:%M:%S").to_string();

        // Load interaction_events
        let ie_batch = self.load_interaction_events(&cutoff_str).await?;
        // Load user_activities
        let ua_batch = self.load_user_activities(&cutoff_str).await?;
        // Load users
        let u_batch = self.load_users().await?;
        // Load matches
        let m_batch = self.load_matches(&cutoff_str).await?;

        // Recreate context to drop stale tables
        self.ctx = SessionContext::new();

        self.register_batch("interaction_events", ie_batch)?;
        self.register_batch("user_activities", ua_batch)?;
        self.register_batch("users", u_batch)?;
        self.register_batch("matches", m_batch)?;

        self.last_refresh = Some(Utc::now());
        info!("DataFusion tables refreshed (lookback={}h)", lookback_hours);
        Ok(())
    }

    /// Execute a SQL query and return results as JSON rows.
    pub async fn query(&self, sql: &str) -> Result<Vec<serde_json::Value>, String> {
        let df = self.ctx.sql(sql).await.map_err(|e| format!("SQL error: {}", e))?;
        let batches = df.collect().await.map_err(|e| format!("Collect error: {}", e))?;

        let mut rows = Vec::new();
        for batch in &batches {
            let schema = batch.schema();
            for row_idx in 0..batch.num_rows() {
                let mut row = serde_json::Map::new();
                for (col_idx, field) in schema.fields().iter().enumerate() {
                    let col = batch.column(col_idx);
                    let value = arrow_value_to_json(col, row_idx);
                    row.insert(field.name().clone(), value);
                }
                rows.push(serde_json::Value::Object(row));
            }
        }

        Ok(rows)
    }

    // ─── Pre-built analytics queries ────────────────────────────────

    pub async fn dau(&self, days: i64) -> Result<Vec<serde_json::Value>, String> {
        self.query(&format!(
            "SELECT CAST(created_at AS DATE) AS day, COUNT(DISTINCT user_id) AS dau \
             FROM user_activities GROUP BY CAST(created_at AS DATE) \
             ORDER BY day DESC LIMIT {}",
            days
        )).await
    }

    pub async fn engagement_funnel(&self) -> Result<Vec<serde_json::Value>, String> {
        self.query(
            "SELECT event_type, COUNT(*) AS total, COUNT(DISTINCT user_id) AS unique_users \
             FROM interaction_events \
             WHERE event_type IN ('impression', 'like', 'pass', 'message') \
             GROUP BY event_type ORDER BY total DESC"
        ).await
    }

    pub async fn match_rate_by_gender(&self) -> Result<Vec<serde_json::Value>, String> {
        self.query(
            "SELECT u.gender, COUNT(*) AS total_matches, \
             COUNT(CASE WHEN m.status = 'accepted' THEN 1 END) AS accepted \
             FROM matches m JOIN users u ON m.user1_id = u.id \
             GROUP BY u.gender ORDER BY total_matches DESC"
        ).await
    }

    pub async fn surface_performance(&self) -> Result<Vec<serde_json::Value>, String> {
        self.query(
            "SELECT surface, COUNT(*) AS events, \
             AVG(reward) AS avg_reward, AVG(CAST(delay_ms AS DOUBLE)) AS avg_delay_ms \
             FROM interaction_events WHERE surface IS NOT NULL \
             GROUP BY surface ORDER BY events DESC"
        ).await
    }

    pub async fn student_demographics(&self) -> Result<Vec<serde_json::Value>, String> {
        self.query(
            "SELECT student_tier, university, COUNT(*) AS user_count, \
             AVG(attractiveness_score) AS avg_attractiveness \
             FROM users WHERE is_student_verified = true \
             GROUP BY student_tier, university ORDER BY user_count DESC LIMIT 25"
        ).await
    }

    pub async fn hourly_activity(&self) -> Result<Vec<serde_json::Value>, String> {
        self.query(
            "SELECT date_trunc('hour', created_at) AS hour, event_type, COUNT(*) AS count \
             FROM interaction_events \
             GROUP BY date_trunc('hour', created_at), event_type \
             ORDER BY hour DESC LIMIT 200"
        ).await
    }

    pub async fn bandit_arm_performance(&self) -> Result<Vec<serde_json::Value>, String> {
        self.query(
            "SELECT rank, COUNT(*) AS pulls, AVG(reward) AS avg_reward, \
             AVG(CAST(delay_ms AS DOUBLE)) AS avg_response_ms \
             FROM interaction_events WHERE rank IS NOT NULL AND reward IS NOT NULL \
             GROUP BY rank ORDER BY rank"
        ).await
    }

    // ─── Data loaders ───────────────────────────────────────────────

    fn register_batch(&self, name: &str, batch: RecordBatch) -> Result<(), String> {
        let mem_table = datafusion::datasource::MemTable::try_new(
            batch.schema(),
            vec![vec![batch]],
        ).map_err(|e| format!("MemTable error: {}", e))?;

        self.ctx
            .register_table(name, Arc::new(mem_table))
            .map_err(|e| format!("Register error: {}", e))?;

        Ok(())
    }

    async fn load_interaction_events(&self, cutoff: &str) -> Result<RecordBatch, String> {
        let rows: Vec<(i64, i32, i32, String, Option<String>, Option<i32>, Option<String>, Option<f64>, Option<i32>, chrono::NaiveDateTime)> =
            sqlx::query_as(
                "SELECT id::bigint, user_id, target_user_id, event_type, \
                 slate_id, rank, surface, reward, delay_ms, created_at::timestamp \
                 FROM interaction_events WHERE created_at >= $1::timestamp \
                 ORDER BY created_at DESC LIMIT 500000"
            )
            .bind(cutoff)
            .fetch_all(&self.db)
            .await
            .map_err(|e| format!("PG query failed: {}", e))?;

        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("user_id", DataType::Int32, false),
            Field::new("target_user_id", DataType::Int32, false),
            Field::new("event_type", DataType::Utf8, false),
            Field::new("slate_id", DataType::Utf8, true),
            Field::new("rank", DataType::Int32, true),
            Field::new("surface", DataType::Utf8, true),
            Field::new("reward", DataType::Float64, true),
            Field::new("delay_ms", DataType::Int32, true),
            Field::new("created_at", DataType::Utf8, false),
        ]));

        let id: Vec<i64> = rows.iter().map(|r| r.0).collect();
        let user_id: Vec<i32> = rows.iter().map(|r| r.1).collect();
        let target: Vec<i32> = rows.iter().map(|r| r.2).collect();
        let event_type: Vec<String> = rows.iter().map(|r| r.3.clone()).collect();
        let slate_id: Vec<Option<String>> = rows.iter().map(|r| r.4.clone()).collect();
        let rank: Vec<Option<i32>> = rows.iter().map(|r| r.5).collect();
        let surface: Vec<Option<String>> = rows.iter().map(|r| r.6.clone()).collect();
        let reward: Vec<Option<f64>> = rows.iter().map(|r| r.7).collect();
        let delay_ms: Vec<Option<i32>> = rows.iter().map(|r| r.8).collect();
        let created_at: Vec<String> = rows.iter().map(|r| r.9.to_string()).collect();

        RecordBatch::try_new(schema, vec![
            Arc::new(Int64Array::from(id)),
            Arc::new(Int32Array::from(user_id)),
            Arc::new(Int32Array::from(target)),
            Arc::new(StringArray::from(event_type)),
            Arc::new(StringArray::from(slate_id)),
            Arc::new(Int32Array::from(rank)),
            Arc::new(StringArray::from(surface)),
            Arc::new(Float64Array::from(reward)),
            Arc::new(Int32Array::from(delay_ms)),
            Arc::new(StringArray::from(created_at)),
        ]).map_err(|e| format!("RecordBatch error: {}", e))
    }

    async fn load_user_activities(&self, cutoff: &str) -> Result<RecordBatch, String> {
        let rows: Vec<(i64, i32, String, chrono::NaiveDateTime)> =
            sqlx::query_as(
                "SELECT id::bigint, user_id, activity_type, created_at::timestamp \
                 FROM user_activities WHERE created_at >= $1::timestamp \
                 ORDER BY created_at DESC LIMIT 500000"
            )
            .bind(cutoff)
            .fetch_all(&self.db)
            .await
            .map_err(|e| format!("PG query failed: {}", e))?;

        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("user_id", DataType::Int32, false),
            Field::new("activity_type", DataType::Utf8, false),
            Field::new("created_at", DataType::Utf8, false),
        ]));

        RecordBatch::try_new(schema, vec![
            Arc::new(Int64Array::from(rows.iter().map(|r| r.0).collect::<Vec<_>>())),
            Arc::new(Int32Array::from(rows.iter().map(|r| r.1).collect::<Vec<_>>())),
            Arc::new(StringArray::from(rows.iter().map(|r| r.2.clone()).collect::<Vec<_>>())),
            Arc::new(StringArray::from(rows.iter().map(|r| r.3.to_string()).collect::<Vec<_>>())),
        ]).map_err(|e| format!("RecordBatch error: {}", e))
    }

    async fn load_users(&self) -> Result<RecordBatch, String> {
        let rows: Vec<(i64, Option<String>, bool, bool, bool, Option<String>, Option<String>, Option<f64>)> =
            sqlx::query_as(
                "SELECT id::bigint, gender, is_active, is_verified, \
                 is_student_verified, university, student_tier, attractiveness_score \
                 FROM users"
            )
            .fetch_all(&self.db)
            .await
            .map_err(|e| format!("PG query failed: {}", e))?;

        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("gender", DataType::Utf8, true),
            Field::new("is_active", DataType::Boolean, false),
            Field::new("is_verified", DataType::Boolean, false),
            Field::new("is_student_verified", DataType::Boolean, false),
            Field::new("university", DataType::Utf8, true),
            Field::new("student_tier", DataType::Utf8, true),
            Field::new("attractiveness_score", DataType::Float64, true),
        ]));

        RecordBatch::try_new(schema, vec![
            Arc::new(Int64Array::from(rows.iter().map(|r| r.0).collect::<Vec<_>>())),
            Arc::new(StringArray::from(rows.iter().map(|r| r.1.clone()).collect::<Vec<_>>())),
            Arc::new(BooleanArray::from(rows.iter().map(|r| r.2).collect::<Vec<_>>())),
            Arc::new(BooleanArray::from(rows.iter().map(|r| r.3).collect::<Vec<_>>())),
            Arc::new(BooleanArray::from(rows.iter().map(|r| r.4).collect::<Vec<_>>())),
            Arc::new(StringArray::from(rows.iter().map(|r| r.5.clone()).collect::<Vec<_>>())),
            Arc::new(StringArray::from(rows.iter().map(|r| r.6.clone()).collect::<Vec<_>>())),
            Arc::new(Float64Array::from(rows.iter().map(|r| r.7).collect::<Vec<_>>())),
        ]).map_err(|e| format!("RecordBatch error: {}", e))
    }

    async fn load_matches(&self, cutoff: &str) -> Result<RecordBatch, String> {
        let rows: Vec<(i64, i32, i32, String, chrono::NaiveDateTime)> =
            sqlx::query_as(
                "SELECT id::bigint, user1_id, user2_id, status, created_at::timestamp \
                 FROM matches WHERE created_at >= $1::timestamp \
                 ORDER BY created_at DESC LIMIT 500000"
            )
            .bind(cutoff)
            .fetch_all(&self.db)
            .await
            .map_err(|e| format!("PG query failed: {}", e))?;

        let schema = Arc::new(Schema::new(vec![
            Field::new("id", DataType::Int64, false),
            Field::new("user1_id", DataType::Int32, false),
            Field::new("user2_id", DataType::Int32, false),
            Field::new("status", DataType::Utf8, false),
            Field::new("created_at", DataType::Utf8, false),
        ]));

        RecordBatch::try_new(schema, vec![
            Arc::new(Int64Array::from(rows.iter().map(|r| r.0).collect::<Vec<_>>())),
            Arc::new(Int32Array::from(rows.iter().map(|r| r.1).collect::<Vec<_>>())),
            Arc::new(Int32Array::from(rows.iter().map(|r| r.2).collect::<Vec<_>>())),
            Arc::new(StringArray::from(rows.iter().map(|r| r.3.clone()).collect::<Vec<_>>())),
            Arc::new(StringArray::from(rows.iter().map(|r| r.4.to_string()).collect::<Vec<_>>())),
        ]).map_err(|e| format!("RecordBatch error: {}", e))
    }
}

/// Convert an Arrow column value at a given row index to JSON.
fn arrow_value_to_json(col: &dyn datafusion::arrow::array::Array, idx: usize) -> serde_json::Value {
    use datafusion::arrow::datatypes::DataType;

    if col.is_null(idx) {
        return serde_json::Value::Null;
    }

    match col.data_type() {
        DataType::Int32 => {
            let arr = col.as_any().downcast_ref::<Int32Array>().unwrap();
            serde_json::json!(arr.value(idx))
        }
        DataType::Int64 => {
            let arr = col.as_any().downcast_ref::<Int64Array>().unwrap();
            serde_json::json!(arr.value(idx))
        }
        DataType::UInt64 => {
            let arr = col.as_any().downcast_ref::<UInt64Array>().unwrap();
            serde_json::json!(arr.value(idx))
        }
        DataType::Float64 => {
            let arr = col.as_any().downcast_ref::<Float64Array>().unwrap();
            serde_json::json!(arr.value(idx))
        }
        DataType::Boolean => {
            let arr = col.as_any().downcast_ref::<BooleanArray>().unwrap();
            serde_json::json!(arr.value(idx))
        }
        DataType::Utf8 => {
            let arr = col.as_any().downcast_ref::<StringArray>().unwrap();
            serde_json::json!(arr.value(idx))
        }
        DataType::Date32 => {
            let arr = col.as_any().downcast_ref::<Date32Array>().unwrap();
            serde_json::json!(arr.value_as_date(idx).map(|d| d.to_string()))
        }
        _ => {
            // Fallback: use debug representation
            serde_json::json!(format!("{:?}", col))
        }
    }
}
