//! Kafka event consumer for subscribing to topics
//!
//! Provides a robust consumer with automatic deserialization,
//! offset management, and error handling with DLQ support.

use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use futures::StreamExt;
use rdkafka::config::ClientConfig;
use rdkafka::consumer::{CommitMode, Consumer, StreamConsumer};
use rdkafka::message::Message;
use rdkafka::TopicPartitionList;
use serde::de::DeserializeOwned;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use super::producer::{EventProducer, ProducerConfig};
use super::types::{topics, DlqEvent, EventEnvelope};

/// Result type for consumer operations
pub type ConsumerResult<T> = Result<T, ConsumerError>;

/// Errors that can occur during event consumption
#[derive(Debug, thiserror::Error)]
pub enum ConsumerError {
    #[error("Failed to create Kafka consumer: {0}")]
    Creation(String),

    #[error("Failed to subscribe to topics: {0}")]
    Subscription(String),

    #[error("Failed to deserialize event: {0}")]
    Deserialization(String),

    #[error("Failed to commit offset: {0}")]
    Commit(String),

    #[error("Handler error: {0}")]
    Handler(String),

    #[error("Consumer stream ended unexpectedly")]
    StreamEnded,
}

/// Configuration for the Kafka consumer
#[derive(Debug, Clone)]
pub struct ConsumerConfig {
    /// Kafka bootstrap servers (comma-separated)
    pub bootstrap_servers: String,
    /// Consumer group ID
    pub group_id: String,
    /// Client ID for this consumer
    pub client_id: String,
    /// Auto offset reset (earliest, latest)
    pub auto_offset_reset: String,
    /// Enable auto commit
    pub enable_auto_commit: bool,
    /// Auto commit interval (ms)
    pub auto_commit_interval_ms: u32,
    /// Session timeout (ms)
    pub session_timeout_ms: u32,
    /// Heartbeat interval (ms)
    pub heartbeat_interval_ms: u32,
    /// Max poll interval (ms)
    pub max_poll_interval_ms: u32,
    /// Max records per poll
    pub max_poll_records: u32,
    /// Enable DLQ for failed messages
    pub enable_dlq: bool,
    /// Max retries before sending to DLQ
    pub max_retries: u32,
}

impl Default for ConsumerConfig {
    fn default() -> Self {
        Self {
            bootstrap_servers: "localhost:9092".to_string(),
            group_id: "nava-consumer-group".to_string(),
            client_id: "nava-consumer".to_string(),
            auto_offset_reset: "earliest".to_string(),
            enable_auto_commit: false, // Manual commits for at-least-once
            auto_commit_interval_ms: 5000,
            session_timeout_ms: 45000,
            heartbeat_interval_ms: 3000,
            max_poll_interval_ms: 300000,
            max_poll_records: 500,
            enable_dlq: true,
            max_retries: 3,
        }
    }
}

impl ConsumerConfig {
    /// Create config from environment variables
    pub fn from_env(group_id: &str) -> Self {
        Self {
            bootstrap_servers: std::env::var("KAFKA_BOOTSTRAP_SERVERS")
                .unwrap_or_else(|_| "localhost:9092".to_string()),
            group_id: group_id.to_string(),
            client_id: std::env::var("KAFKA_CLIENT_ID")
                .unwrap_or_else(|_| format!("{}-consumer", group_id)),
            auto_offset_reset: std::env::var("KAFKA_AUTO_OFFSET_RESET")
                .unwrap_or_else(|_| "earliest".to_string()),
            enable_auto_commit: std::env::var("KAFKA_ENABLE_AUTO_COMMIT")
                .map(|v| v == "true")
                .unwrap_or(false),
            auto_commit_interval_ms: std::env::var("KAFKA_AUTO_COMMIT_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5000),
            session_timeout_ms: std::env::var("KAFKA_SESSION_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(45000),
            heartbeat_interval_ms: std::env::var("KAFKA_HEARTBEAT_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3000),
            max_poll_interval_ms: std::env::var("KAFKA_MAX_POLL_INTERVAL_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(300000),
            max_poll_records: std::env::var("KAFKA_MAX_POLL_RECORDS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(500),
            enable_dlq: std::env::var("KAFKA_ENABLE_DLQ")
                .map(|v| v != "false")
                .unwrap_or(true),
            max_retries: std::env::var("KAFKA_MAX_RETRIES")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(3),
        }
    }
}

/// Handler trait for processing events
#[async_trait]
pub trait EventHandler<T>: Send + Sync {
    /// Process an event
    async fn handle(&self, event: EventEnvelope<T>) -> Result<(), String>;
}

/// Kafka event consumer
pub struct EventConsumer {
    consumer: StreamConsumer,
    config: ConsumerConfig,
    dlq_producer: Option<EventProducer>,
}

impl EventConsumer {
    /// Create a new event consumer
    pub fn new(config: ConsumerConfig) -> ConsumerResult<Self> {
        let consumer: StreamConsumer = ClientConfig::new()
            .set("bootstrap.servers", &config.bootstrap_servers)
            .set("group.id", &config.group_id)
            .set("client.id", &config.client_id)
            .set("auto.offset.reset", &config.auto_offset_reset)
            .set(
                "enable.auto.commit",
                config.enable_auto_commit.to_string(),
            )
            .set(
                "auto.commit.interval.ms",
                config.auto_commit_interval_ms.to_string(),
            )
            .set("session.timeout.ms", config.session_timeout_ms.to_string())
            .set(
                "heartbeat.interval.ms",
                config.heartbeat_interval_ms.to_string(),
            )
            .set(
                "max.poll.interval.ms",
                config.max_poll_interval_ms.to_string(),
            )
            .create()
            .map_err(|e| ConsumerError::Creation(e.to_string()))?;

        // Create DLQ producer if enabled
        let dlq_producer = if config.enable_dlq {
            let producer_config = ProducerConfig {
                bootstrap_servers: config.bootstrap_servers.clone(),
                client_id: format!("{}-dlq-producer", config.client_id),
                ..Default::default()
            };
            EventProducer::new(producer_config, "dlq-handler").ok()
        } else {
            None
        };

        info!(
            bootstrap_servers = %config.bootstrap_servers,
            group_id = %config.group_id,
            "Kafka consumer created"
        );

        Ok(Self {
            consumer,
            config,
            dlq_producer,
        })
    }

    /// Subscribe to topics
    pub fn subscribe(&self, topics: &[&str]) -> ConsumerResult<()> {
        self.consumer
            .subscribe(topics)
            .map_err(|e| ConsumerError::Subscription(e.to_string()))?;

        info!(topics = ?topics, "Subscribed to topics");
        Ok(())
    }

    /// Start consuming events with a handler
    #[instrument(skip(self, handler))]
    pub async fn consume<T, H>(&self, handler: H) -> ConsumerResult<()>
    where
        T: DeserializeOwned + Send + 'static,
        H: EventHandler<T>,
    {
        let mut stream = self.consumer.stream();

        info!("Starting event consumption loop");

        while let Some(result) = stream.next().await {
            match result {
                Ok(message) => {
                    let topic = message.topic().to_string();
                    let partition = message.partition();
                    let offset = message.offset();

                    // Extract payload
                    let payload = match message.payload() {
                        Some(p) => p,
                        None => {
                            warn!(topic = %topic, partition, offset, "Empty message payload");
                            self.commit_message(&message)?;
                            continue;
                        }
                    };

                    // Deserialize envelope
                    let envelope: EventEnvelope<T> = match serde_json::from_slice(payload) {
                        Ok(e) => e,
                        Err(e) => {
                            error!(
                                topic = %topic,
                                partition,
                                offset,
                                error = %e,
                                "Failed to deserialize event"
                            );

                            // Send to DLQ
                            self.send_to_dlq(
                                &topic,
                                partition,
                                offset,
                                payload,
                                &format!("Deserialization error: {}", e),
                                0,
                            )
                            .await;

                            self.commit_message(&message)?;
                            continue;
                        }
                    };

                    let event_id = envelope.event_id;
                    let event_type = envelope.event_type.clone();

                    // Process with handler
                    match handler.handle(envelope).await {
                        Ok(_) => {
                            info!(
                                event_id = %event_id,
                                event_type = %event_type,
                                topic = %topic,
                                "Event processed successfully"
                            );
                        }
                        Err(e) => {
                            error!(
                                event_id = %event_id,
                                event_type = %event_type,
                                topic = %topic,
                                error = %e,
                                "Handler error"
                            );

                            // Send to DLQ
                            self.send_to_dlq(&topic, partition, offset, payload, &e, 0)
                                .await;
                        }
                    }

                    // Commit offset after processing
                    self.commit_message(&message)?;
                }
                Err(e) => {
                    error!(error = %e, "Kafka consumer error");
                }
            }
        }

        Err(ConsumerError::StreamEnded)
    }

    /// Consume with retry logic
    #[instrument(skip(self, handler))]
    pub async fn consume_with_retry<T, H>(&self, handler: H) -> ConsumerResult<()>
    where
        T: DeserializeOwned + Send + Clone + 'static,
        H: EventHandler<T>,
    {
        let mut stream = self.consumer.stream();
        let max_retries = self.config.max_retries;

        info!(max_retries, "Starting event consumption with retry");

        while let Some(result) = stream.next().await {
            match result {
                Ok(message) => {
                    let topic = message.topic().to_string();
                    let partition = message.partition();
                    let offset = message.offset();

                    let payload = match message.payload() {
                        Some(p) => p,
                        None => {
                            self.commit_message(&message)?;
                            continue;
                        }
                    };

                    let envelope: EventEnvelope<T> = match serde_json::from_slice(payload) {
                        Ok(e) => e,
                        Err(e) => {
                            self.send_to_dlq(
                                &topic,
                                partition,
                                offset,
                                payload,
                                &format!("Deserialization error: {}", e),
                                0,
                            )
                            .await;
                            self.commit_message(&message)?;
                            continue;
                        }
                    };

                    let event_id = envelope.event_id;

                    // Retry loop
                    let mut last_error = String::new();
                    let mut success = false;

                    for attempt in 0..=max_retries {
                        if attempt > 0 {
                            // Exponential backoff with jitter
                            let base_delay = Duration::from_millis(100 * 2u64.pow(attempt - 1));
                            let jitter = Duration::from_millis(rand::random::<u64>() % 100);
                            tokio::time::sleep(base_delay + jitter).await;

                            warn!(
                                event_id = %event_id,
                                attempt,
                                max_retries,
                                "Retrying event processing"
                            );
                        }

                        match handler.handle(envelope.clone()).await {
                            Ok(_) => {
                                success = true;
                                break;
                            }
                            Err(e) => {
                                last_error = e;
                            }
                        }
                    }

                    if !success {
                        error!(
                            event_id = %event_id,
                            error = %last_error,
                            "Event processing failed after all retries"
                        );
                        self.send_to_dlq(
                            &topic,
                            partition,
                            offset,
                            payload,
                            &last_error,
                            max_retries as i32,
                        )
                        .await;
                    }

                    self.commit_message(&message)?;
                }
                Err(e) => {
                    error!(error = %e, "Kafka consumer error");
                }
            }
        }

        Err(ConsumerError::StreamEnded)
    }

    /// Commit a message offset
    fn commit_message<M: Message>(&self, message: &M) -> ConsumerResult<()> {
        self.consumer
            .commit_message(message, CommitMode::Async)
            .map_err(|e| ConsumerError::Commit(e.to_string()))
    }

    /// Send a failed message to DLQ
    async fn send_to_dlq(
        &self,
        original_topic: &str,
        partition: i32,
        offset: i64,
        payload: &[u8],
        error_message: &str,
        retry_count: i32,
    ) {
        let Some(producer) = &self.dlq_producer else {
            warn!("DLQ producer not available, dropping failed message");
            return;
        };

        let dlq_event = DlqEvent {
            original_topic: original_topic.to_string(),
            original_partition: partition,
            original_offset: offset,
            original_payload: String::from_utf8_lossy(payload).to_string(),
            error_message: error_message.to_string(),
            retry_count,
            first_failed_at: Utc::now(),
            last_failed_at: Utc::now(),
        };

        match producer
            .publish(topics::DLQ_EVENTS, "dlq.event", dlq_event, None)
            .await
        {
            Ok(event_id) => {
                info!(
                    event_id = %event_id,
                    original_topic = %original_topic,
                    "Message sent to DLQ"
                );
            }
            Err(e) => {
                error!(
                    error = %e,
                    original_topic = %original_topic,
                    "Failed to send message to DLQ"
                );
            }
        }
    }

    /// Get current partition assignment
    pub fn assignment(&self) -> ConsumerResult<TopicPartitionList> {
        self.consumer
            .assignment()
            .map_err(|e| ConsumerError::Subscription(e.to_string()))
    }

    /// Pause consumption (useful for backpressure)
    pub fn pause(&self) -> ConsumerResult<()> {
        let assignment = self.assignment()?;
        self.consumer
            .pause(&assignment)
            .map_err(|e| ConsumerError::Subscription(e.to_string()))
    }

    /// Resume consumption
    pub fn resume(&self) -> ConsumerResult<()> {
        let assignment = self.assignment()?;
        self.consumer
            .resume(&assignment)
            .map_err(|e| ConsumerError::Subscription(e.to_string()))
    }
}

/// Simple function-based handler wrapper
pub struct FnHandler<F, T> {
    handler: F,
    _phantom: std::marker::PhantomData<T>,
}

impl<F, T, Fut> FnHandler<F, T>
where
    F: Fn(EventEnvelope<T>) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<(), String>> + Send,
    T: Send + 'static,
{
    pub fn new(handler: F) -> Self {
        Self {
            handler,
            _phantom: std::marker::PhantomData,
        }
    }
}

#[async_trait]
impl<F, T, Fut> EventHandler<T> for FnHandler<F, T>
where
    F: Fn(EventEnvelope<T>) -> Fut + Send + Sync,
    Fut: std::future::Future<Output = Result<(), String>> + Send,
    T: Send + 'static,
{
    async fn handle(&self, event: EventEnvelope<T>) -> Result<(), String> {
        (self.handler)(event).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_consumer_config_defaults() {
        let config = ConsumerConfig::default();
        assert_eq!(config.bootstrap_servers, "localhost:9092");
        assert!(!config.enable_auto_commit);
        assert!(config.enable_dlq);
        assert_eq!(config.max_retries, 3);
    }
}
