//! Shared library for NAVA microservices
//!
//! Contains common models, utilities, error handling, and cross-cutting concerns.
//!
//! # Modules
//! - `auth`: JWT authentication, RBAC authorization, and request signing
//! - `config`: Service configuration management
//! - `error`: Error types and handling
//! - `events`: Kafka event producer/consumer with DLQ and offset management
//! - `models`: Shared data models
//! - `saga`: Saga pattern for distributed transactions
//! - `tracing`: Distributed tracing with OpenTelemetry
//! - `versioning`: API versioning strategies
//! - `testing`: Integration test utilities

pub mod auth;
pub mod config;
pub mod error;
pub mod events;
pub mod models;
pub mod saga;
pub mod tracing;
pub mod versioning;

#[cfg(feature = "testing")]
pub mod testing;

// Re-exports for convenience
pub use error::{AppError, AppResult};
pub use events::{EventConsumer, EventProducer, DlqProcessor, OffsetStore};
pub use saga::{SagaOrchestrator, SagaContext, SagaError, SagaStatus};
pub use auth::{Permission, Role, RbacPolicy, UserContext, RequestSigner, RequestVerifier};
pub use versioning::{ApiVersion, VersioningConfig, VersionExtractor};
