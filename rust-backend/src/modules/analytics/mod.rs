//! Analytics module — ported from microservices/analytics-service + DataFusion.
//!
//! Two complementary systems:
//! 1. **ClickHouse sink** — event-driven OLAP storage (was Kafka consumer, now EventBus subscriber)
//! 2. **DataFusion engine** — in-process SQL analytics over Postgres data (loaded into Arrow tables)
//!
//! Routes:
//! - POST /analytics/refresh         — reload Postgres data into DataFusion
//! - GET  /analytics/dau             — daily active users
//! - GET  /analytics/funnel          — engagement funnel
//! - GET  /analytics/match-rate      — match rate by gender
//! - GET  /analytics/surfaces        — reward/latency per UI surface
//! - GET  /analytics/students        — student demographics
//! - GET  /analytics/hourly          — hourly event volume
//! - GET  /analytics/bandit          — bandit arm performance
//! - POST /analytics/query           — custom SQL query (SELECT only)

pub mod clickhouse;
pub mod datafusion_engine;
pub mod routes;

use std::sync::Arc;

use sqlx::PgPool;
use tokio::sync::broadcast;
use tracing::{error, info, warn};

use crate::modules::events::EventEnvelope;
use self::clickhouse::ClickHouseStorage;

/// Analytics module that sinks events to ClickHouse and exposes DataFusion queries.
pub struct AnalyticsModule {
    pub clickhouse: Option<Arc<ClickHouseStorage>>,
    pub engine: Arc<tokio::sync::RwLock<datafusion_engine::DataFusionEngine>>,
}

impl AnalyticsModule {
    pub async fn new(db: PgPool) -> Self {
        let clickhouse = ClickHouseStorage::new().await;
        let engine = datafusion_engine::DataFusionEngine::new(db);

        Self {
            clickhouse: Some(Arc::new(clickhouse)),
            engine: Arc::new(tokio::sync::RwLock::new(engine)),
        }
    }

    /// Start listening to events and sinking to ClickHouse.
    pub fn start_listener(self: &Arc<Self>, mut rx: broadcast::Receiver<EventEnvelope>) {
        let this = Arc::clone(self);
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(envelope) => {
                        if let Some(ref ch) = this.clickhouse {
                            let ch = ch.clone();
                            let event_id = envelope.event_id.clone();
                            let event_type = envelope.event_type.clone();
                            let timestamp = envelope.timestamp;
                            let data = serde_json::to_value(&envelope.data).unwrap_or_default();

                            tokio::spawn(async move {
                                if let Err(e) = ch.store_event(
                                    &event_id,
                                    &event_type,
                                    &timestamp,
                                    &data,
                                    envelope.source,
                                ).await {
                                    error!(error = %e, "ClickHouse store failed");
                                }
                            });
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(n)) => {
                        warn!(skipped = n, "Analytics listener lagged");
                    }
                    Err(broadcast::error::RecvError::Closed) => {
                        info!("Event bus closed, analytics listener shutting down");
                        break;
                    }
                }
            }
        });
    }
}
