//! Event-driven architecture module
//!
//! Provides Kafka producer and consumer abstractions for microservices.
//!
//! # Features
//! - Type-safe event publishing and consumption
//! - Automatic offset management with PostgreSQL-backed storage
//! - Dead Letter Queue (DLQ) with exponential backoff and circuit breaker
//! - Deadlock prevention with processing locks and timeouts
//!
//! # Event Types
//! - User events: registration, verification, profile updates
//! - Payment events: orders, payments, subscriptions
//! - Match events: swipes, matches, unmatches
//! - Chat events: messages, read receipts
//! - Notification commands: push, email, sms

pub mod consumer;
pub mod dlq_processor;
pub mod offset_store;
pub mod producer;
pub mod types;

pub use consumer::{ConsumerConfig, ConsumerError, EventConsumer, EventHandler};
pub use dlq_processor::{DlqProcessor, DlqProcessorConfig, DlqStats, run_dlq_processor};
pub use offset_store::{OffsetCheckpoint, OffsetRecord, OffsetStore};
pub use producer::{EventProducer, ProducerConfig, ProducerError};
pub use types::*;
