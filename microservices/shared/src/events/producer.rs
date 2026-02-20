//! Kafka event producer for publishing events to topics
//!
//! Provides a type-safe interface for producing events with automatic
//! serialization, partitioning, and delivery guarantees.

use std::time::Duration;

use rdkafka::config::ClientConfig;
use rdkafka::producer::{FutureProducer, FutureRecord};
use rdkafka::util::Timeout;
use serde::Serialize;
use tracing::{error, info, instrument, warn};
use uuid::Uuid;

use super::types::{EventEnvelope, EventMetadata};

/// Result type for producer operations
pub type ProducerResult<T> = Result<T, ProducerError>;

/// Errors that can occur during event production
#[derive(Debug, thiserror::Error)]
pub enum ProducerError {
    #[error("Failed to create Kafka producer: {0}")]
    Creation(String),

    #[error("Failed to serialize event: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Failed to deliver message: {0}")]
    Delivery(String),

    #[error("Producer timeout after {0:?}")]
    Timeout(Duration),
}

/// Configuration for the Kafka producer
#[derive(Debug, Clone)]
pub struct ProducerConfig {
    /// Kafka bootstrap servers (comma-separated)
    pub bootstrap_servers: String,
    /// Client ID for this producer
    pub client_id: String,
    /// Message timeout
    pub message_timeout_ms: u32,
    /// Enable idempotence for exactly-once semantics
    pub enable_idempotence: bool,
    /// Number of acknowledgments required
    pub acks: String,
    /// Compression type (none, gzip, snappy, lz4, zstd)
    pub compression_type: String,
    /// Maximum number of in-flight requests per connection
    pub max_in_flight: u32,
    /// Batch size in bytes
    pub batch_size: u32,
    /// Linger time in ms (wait before sending batch)
    pub linger_ms: u32,
}

impl Default for ProducerConfig {
    fn default() -> Self {
        Self {
            bootstrap_servers: "localhost:9092".to_string(),
            client_id: "nava-producer".to_string(),
            message_timeout_ms: 30000,
            enable_idempotence: true,
            acks: "all".to_string(),
            compression_type: "lz4".to_string(),
            max_in_flight: 5,
            batch_size: 16384,
            linger_ms: 5,
        }
    }
}

impl ProducerConfig {
    /// Create config from environment variables
    pub fn from_env() -> Self {
        Self {
            bootstrap_servers: std::env::var("KAFKA_BOOTSTRAP_SERVERS")
                .unwrap_or_else(|_| "localhost:9092".to_string()),
            client_id: std::env::var("KAFKA_CLIENT_ID")
                .unwrap_or_else(|_| "nava-producer".to_string()),
            message_timeout_ms: std::env::var("KAFKA_MESSAGE_TIMEOUT_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(30000),
            enable_idempotence: std::env::var("KAFKA_ENABLE_IDEMPOTENCE")
                .map(|v| v == "true")
                .unwrap_or(true),
            acks: std::env::var("KAFKA_ACKS").unwrap_or_else(|_| "all".to_string()),
            compression_type: std::env::var("KAFKA_COMPRESSION")
                .unwrap_or_else(|_| "lz4".to_string()),
            max_in_flight: std::env::var("KAFKA_MAX_IN_FLIGHT")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
            batch_size: std::env::var("KAFKA_BATCH_SIZE")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(16384),
            linger_ms: std::env::var("KAFKA_LINGER_MS")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(5),
        }
    }
}

/// Kafka event producer with type-safe event publishing
pub struct EventProducer {
    producer: FutureProducer,
    service_name: String,
    default_timeout: Duration,
}

impl EventProducer {
    /// Create a new event producer
    pub fn new(config: ProducerConfig, service_name: impl Into<String>) -> ProducerResult<Self> {
        let producer: FutureProducer = ClientConfig::new()
            .set("bootstrap.servers", &config.bootstrap_servers)
            .set("client.id", &config.client_id)
            .set("message.timeout.ms", config.message_timeout_ms.to_string())
            .set(
                "enable.idempotence",
                config.enable_idempotence.to_string(),
            )
            .set("acks", &config.acks)
            .set("compression.type", &config.compression_type)
            .set(
                "max.in.flight.requests.per.connection",
                config.max_in_flight.to_string(),
            )
            .set("batch.size", config.batch_size.to_string())
            .set("linger.ms", config.linger_ms.to_string())
            .create()
            .map_err(|e| ProducerError::Creation(e.to_string()))?;

        info!(
            bootstrap_servers = %config.bootstrap_servers,
            client_id = %config.client_id,
            "Kafka producer created"
        );

        Ok(Self {
            producer,
            service_name: service_name.into(),
            default_timeout: Duration::from_millis(config.message_timeout_ms as u64),
        })
    }

    /// Publish an event to a topic
    #[instrument(skip(self, data), fields(topic = %topic, event_type = %event_type))]
    pub async fn publish<T: Serialize>(
        &self,
        topic: &str,
        event_type: impl Into<String>,
        data: T,
        partition_key: Option<&str>,
    ) -> ProducerResult<Uuid> {
        let event_type = event_type.into();
        let envelope = EventEnvelope::new(&event_type, data, &self.service_name);
        let event_id = envelope.event_id;

        self.publish_envelope(topic, envelope, partition_key).await?;

        Ok(event_id)
    }

    /// Publish an event with custom correlation ID (for distributed tracing)
    #[instrument(skip(self, data), fields(topic = %topic, event_type = %event_type, correlation_id = %correlation_id))]
    pub async fn publish_with_correlation<T: Serialize>(
        &self,
        topic: &str,
        event_type: impl Into<String>,
        data: T,
        correlation_id: Uuid,
        causation_id: Option<Uuid>,
        partition_key: Option<&str>,
    ) -> ProducerResult<Uuid> {
        let event_type = event_type.into();
        let mut envelope = EventEnvelope::new(&event_type, data, &self.service_name);
        envelope = envelope.with_correlation_id(correlation_id);

        if let Some(causation) = causation_id {
            envelope = envelope.with_causation_id(causation);
        }

        let event_id = envelope.event_id;
        self.publish_envelope(topic, envelope, partition_key).await?;

        Ok(event_id)
    }

    /// Publish an event with idempotency key
    #[instrument(skip(self, data), fields(topic = %topic, idempotency_key = %idempotency_key))]
    pub async fn publish_idempotent<T: Serialize>(
        &self,
        topic: &str,
        event_type: impl Into<String>,
        data: T,
        idempotency_key: String,
        partition_key: Option<&str>,
    ) -> ProducerResult<Uuid> {
        let event_type = event_type.into();
        let envelope =
            EventEnvelope::new(&event_type, data, &self.service_name).with_idempotency_key(idempotency_key);

        let event_id = envelope.event_id;
        self.publish_envelope(topic, envelope, partition_key).await?;

        Ok(event_id)
    }

    /// Internal method to publish an envelope
    async fn publish_envelope<T: Serialize>(
        &self,
        topic: &str,
        envelope: EventEnvelope<T>,
        partition_key: Option<&str>,
    ) -> ProducerResult<()> {
        let payload = serde_json::to_string(&envelope)?;
        let event_id = envelope.event_id;

        let mut record = FutureRecord::to(topic).payload(&payload);

        // Use provided partition key or event_id for consistent partitioning
        let key_string;
        if let Some(key) = partition_key {
            record = record.key(key);
        } else {
            key_string = event_id.to_string();
            record = record.key(&key_string);
        }

        match self
            .producer
            .send(record, Timeout::After(self.default_timeout))
            .await
        {
            Ok((partition, offset)) => {
                info!(
                    event_id = %event_id,
                    topic = %topic,
                    partition = partition,
                    offset = offset,
                    "Event published successfully"
                );
                Ok(())
            }
            Err((err, _)) => {
                error!(
                    event_id = %event_id,
                    topic = %topic,
                    error = %err,
                    "Failed to publish event"
                );
                Err(ProducerError::Delivery(err.to_string()))
            }
        }
    }

    /// Publish multiple events in a batch (fire-and-forget with logging)
    pub async fn publish_batch<T: Serialize + Clone>(
        &self,
        topic: &str,
        events: Vec<(String, T, Option<String>)>,
    ) -> Vec<ProducerResult<Uuid>> {
        let futures: Vec<_> = events
            .into_iter()
            .map(|(event_type, data, partition_key)| {
                self.publish(topic, event_type, data, partition_key.as_deref())
            })
            .collect();

        futures::future::join_all(futures).await
    }

    /// Flush any pending messages (useful before shutdown)
    pub fn flush(&self, timeout: Duration) {
        match self.producer.flush(Timeout::After(timeout)) {
            Ok(_) => info!("Producer flushed successfully"),
            Err(e) => warn!(error = %e, "Producer flush incomplete"),
        }
    }
}

impl Drop for EventProducer {
    fn drop(&mut self) {
        self.flush(Duration::from_secs(5));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_producer_config_defaults() {
        let config = ProducerConfig::default();
        assert_eq!(config.bootstrap_servers, "localhost:9092");
        assert!(config.enable_idempotence);
        assert_eq!(config.acks, "all");
    }
}
