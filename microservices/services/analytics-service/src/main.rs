//! Analytics Service
//!
//! Event-driven analytics service that consumes all events from Kafka
//! and stores them in ClickHouse for analysis.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{routing::get, Router};
use serde_json::Value;
use shared::events::{
    consumer::{ConsumerConfig, EventConsumer, EventHandler},
    types::{topics, AnalyticsEvent, EventEnvelope},
};
use tokio::signal;
use tower_http::trace::TraceLayer;
use tracing::{error, info, Level};
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

mod storage;

use storage::ClickHouseStorage;

#[tokio::main]
async fn main() {
    // Initialize logging
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "analytics_service=debug,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer())
        .init();

    dotenvy::dotenv().ok();

    let port = std::env::var("ANALYTICS_SERVICE_PORT")
        .unwrap_or_else(|_| "8008".to_string())
        .parse::<u16>()
        .expect("Invalid port");

    // Initialize storage
    let storage = Arc::new(ClickHouseStorage::new().await);

    // Start Kafka consumers in background
    let storage_clone = storage.clone();
    tokio::spawn(async move {
        start_kafka_consumers(storage_clone).await;
    });

    // Health check and metrics API
    let app = Router::new()
        .route("/health", get(health_check))
        .route("/ready", get(readiness_check))
        .route("/metrics", get(metrics))
        .layer(TraceLayer::new_for_http());

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port))
        .await
        .expect("Failed to bind");

    info!("Analytics service listening on port {}", port);

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .expect("Server error");
}

async fn health_check() -> &'static str {
    "OK"
}

async fn readiness_check() -> &'static str {
    "READY"
}

async fn metrics() -> &'static str {
    // TODO: Expose Prometheus metrics
    "# Analytics service metrics\n"
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {},
        _ = terminate => {},
    }

    info!("Shutdown signal received");
}

async fn start_kafka_consumers(storage: Arc<ClickHouseStorage>) {
    let group_id = std::env::var("KAFKA_GROUP_ID")
        .unwrap_or_else(|_| "analytics-consumer-group".to_string());

    // Generic event consumer for all topics
    let all_topics = vec![
        topics::USER_EVENTS,
        topics::PAYMENT_EVENTS,
        topics::MATCH_EVENTS,
        topics::CHAT_EVENTS,
        topics::ANALYTICS_EVENTS,
    ];

    let config = ConsumerConfig::from_env(&group_id);

    match EventConsumer::new(config) {
        Ok(consumer) => {
            if let Err(e) = consumer.subscribe(&all_topics) {
                error!("Failed to subscribe to topics: {}", e);
                return;
            }

            let handler = GenericEventHandler { storage };
            if let Err(e) = consumer.consume(handler).await {
                error!("Analytics consumer error: {}", e);
            }
        }
        Err(e) => {
            error!("Failed to create analytics consumer: {}", e);
        }
    }
}

struct GenericEventHandler {
    storage: Arc<ClickHouseStorage>,
}

#[async_trait]
impl EventHandler<Value> for GenericEventHandler {
    async fn handle(&self, event: EventEnvelope<Value>) -> Result<(), String> {
        info!(
            event_id = %event.event_id,
            event_type = %event.event_type,
            "Processing analytics event"
        );

        // Store raw event in ClickHouse
        self.storage
            .store_event(
                &event.event_id.to_string(),
                &event.event_type,
                &event.timestamp,
                &event.data,
                &event.metadata,
            )
            .await
    }
}
