//! OpenTelemetry Telemetry Configuration
//!
//! Provides distributed tracing support for the Nava platform.
//! Enable with the `otel` feature flag.
//!
//! Environment variables:
//! - OTEL_EXPORTER_OTLP_ENDPOINT: OTLP collector endpoint (default: http://localhost:4317)
//! - OTEL_SERVICE_NAME: Service name for traces (default: nava-backend)
//! - OTEL_TRACES_SAMPLER: Sampling strategy (default: always_on)
//! - OTEL_TRACES_SAMPLER_ARG: Sampling rate if using ratio sampler (default: 1.0)
//!
//! Usage:
//!   cargo build --features otel
//!
//! Compatible with:
//! - Jaeger
//! - Zipkin
//! - AWS X-Ray
//! - Google Cloud Trace
//! - DataDog
//! - Any OTLP-compatible backend

use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry};

#[cfg(feature = "otel")]
use opentelemetry::KeyValue;
#[cfg(feature = "otel")]
use opentelemetry_otlp::WithExportConfig;
#[cfg(feature = "otel")]
use opentelemetry_sdk::{runtime::Tokio, trace::Sampler, Resource};
#[cfg(feature = "otel")]
use tracing_opentelemetry::OpenTelemetryLayer;

/// Initialize telemetry with or without OpenTelemetry
///
/// When the `otel` feature is enabled, this sets up distributed tracing
/// with OTLP export. Otherwise, it uses standard tracing-subscriber.
pub fn init_telemetry(service_name: &str, is_production: bool) {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,sqlx=warn,tower_http=debug"));

    #[cfg(feature = "otel")]
    {
        match init_otel_telemetry(service_name, is_production, env_filter) {
            Ok(_) => tracing::info!("OpenTelemetry tracing initialized"),
            Err(e) => {
                eprintln!("Failed to initialize OpenTelemetry: {}. Using standard tracing.", e);
                init_standard_telemetry(is_production, env_filter);
            }
        }
    }

    #[cfg(not(feature = "otel"))]
    {
        init_standard_telemetry(is_production, env_filter);
    }
}

/// Initialize standard tracing without OpenTelemetry
fn init_standard_telemetry(is_production: bool, env_filter: EnvFilter) {
    let subscriber = Registry::default().with(env_filter);

    if is_production {
        // JSON logging for production (better for log aggregation)
        let json_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_file(true)
            .with_line_number(true)
            .with_thread_ids(true)
            .with_target(true);

        subscriber.with(json_layer).init();
    } else {
        // Pretty logging for development
        let fmt_layer = tracing_subscriber::fmt::layer()
            .pretty()
            .with_file(true)
            .with_line_number(true);

        subscriber.with(fmt_layer).init();
    }
}

/// Initialize OpenTelemetry tracing
#[cfg(feature = "otel")]
fn init_otel_telemetry(
    service_name: &str,
    is_production: bool,
    env_filter: EnvFilter,
) -> Result<(), Box<dyn std::error::Error>> {
    use opentelemetry_sdk::trace::{BatchConfig, TracerProvider};

    // Get OTLP endpoint from environment
    let endpoint = std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT")
        .unwrap_or_else(|_| "http://localhost:4317".to_string());

    // Get sampling rate
    let sampling_rate: f64 = std::env::var("OTEL_TRACES_SAMPLER_ARG")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1.0);

    // Build the sampler
    let sampler = if sampling_rate >= 1.0 {
        Sampler::AlwaysOn
    } else if sampling_rate <= 0.0 {
        Sampler::AlwaysOff
    } else {
        Sampler::TraceIdRatioBased(sampling_rate)
    };

    // Build resource with service info
    let resource = Resource::new(vec![
        KeyValue::new(
            opentelemetry_semantic_conventions::resource::SERVICE_NAME,
            service_name.to_string(),
        ),
        KeyValue::new(
            opentelemetry_semantic_conventions::resource::SERVICE_VERSION,
            env!("CARGO_PKG_VERSION"),
        ),
        KeyValue::new(
            opentelemetry_semantic_conventions::resource::DEPLOYMENT_ENVIRONMENT,
            if is_production { "production" } else { "development" },
        ),
    ]);

    // Create OTLP exporter
    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(&endpoint);

    // Build tracer provider with batch export
    let batch_config = BatchConfig::default()
        .with_max_queue_size(4096)
        .with_max_export_batch_size(512)
        .with_scheduled_delay(std::time::Duration::from_secs(5));

    let tracer_provider = TracerProvider::builder()
        .with_batch_exporter(
            opentelemetry_otlp::new_pipeline()
                .tracing()
                .with_exporter(exporter)
                .build_batch(Tokio)?,
            Tokio,
        )
        .with_config(
            opentelemetry_sdk::trace::Config::default()
                .with_sampler(sampler)
                .with_resource(resource),
        )
        .build();

    let tracer = tracer_provider.tracer(service_name.to_string());

    // Create OpenTelemetry layer
    let otel_layer = OpenTelemetryLayer::new(tracer);

    // Build subscriber with OpenTelemetry layer
    let subscriber = Registry::default()
        .with(env_filter)
        .with(otel_layer);

    if is_production {
        let json_layer = tracing_subscriber::fmt::layer()
            .json()
            .with_file(true)
            .with_line_number(true)
            .with_thread_ids(true);

        subscriber.with(json_layer).init();
    } else {
        let fmt_layer = tracing_subscriber::fmt::layer()
            .pretty()
            .with_file(true)
            .with_line_number(true);

        subscriber.with(fmt_layer).init();
    }

    // Register shutdown handler
    opentelemetry::global::set_tracer_provider(tracer_provider);

    Ok(())
}

/// Shutdown OpenTelemetry tracer (call on application shutdown)
pub fn shutdown_telemetry() {
    #[cfg(feature = "otel")]
    {
        opentelemetry::global::shutdown_tracer_provider();
        tracing::info!("OpenTelemetry tracer shut down");
    }
}

/// Create a traced span for HTTP requests
#[macro_export]
macro_rules! trace_request {
    ($method:expr, $path:expr) => {
        tracing::info_span!(
            "http_request",
            http.method = %$method,
            http.target = %$path,
            otel.kind = "server"
        )
    };
}

/// Create a traced span for database operations
#[macro_export]
macro_rules! trace_db {
    ($operation:expr, $table:expr) => {
        tracing::info_span!(
            "db_query",
            db.operation = %$operation,
            db.table = %$table,
            otel.kind = "client"
        )
    };
}

/// Create a traced span for external API calls
#[macro_export]
macro_rules! trace_external {
    ($service:expr, $operation:expr) => {
        tracing::info_span!(
            "external_call",
            peer.service = %$service,
            external.operation = %$operation,
            otel.kind = "client"
        )
    };
}

/// Create a traced span for payment operations
#[macro_export]
macro_rules! trace_payment {
    ($gateway:expr, $operation:expr, $order_id:expr) => {
        tracing::info_span!(
            "payment",
            payment.gateway = %$gateway,
            payment.operation = %$operation,
            payment.order_id = %$order_id,
            otel.kind = "client"
        )
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_standard_telemetry_dev() {
        // This would initialize telemetry, so we just verify it compiles
        // In actual tests, you'd use a test subscriber
    }
}
