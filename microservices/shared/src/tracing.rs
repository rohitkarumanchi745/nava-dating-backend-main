//! Distributed tracing with OpenTelemetry and Jaeger
//!
//! Provides end-to-end request tracing across all microservices.
//! Supports correlation IDs, span context propagation, and Jaeger export.

use std::time::Duration;

use tracing::Level;
use tracing_subscriber::{
    fmt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter, Registry,
};

#[cfg(feature = "otel")]
use opentelemetry::{
    global,
    sdk::{
        propagation::TraceContextPropagator,
        trace::{self, RandomIdGenerator, Sampler},
        Resource,
    },
    KeyValue,
};

#[cfg(feature = "otel")]
use opentelemetry_otlp::WithExportConfig;

#[cfg(feature = "otel")]
use tracing_opentelemetry::OpenTelemetryLayer;

/// Tracing configuration
#[derive(Debug, Clone)]
pub struct TracingConfig {
    /// Service name for tracing
    pub service_name: String,
    /// Service version
    pub service_version: String,
    /// Environment (dev, staging, prod)
    pub environment: String,
    /// OTLP endpoint (e.g., http://jaeger:4317)
    pub otlp_endpoint: Option<String>,
    /// Sampling ratio (0.0 to 1.0)
    pub sampling_ratio: f64,
    /// Enable JSON logging
    pub json_logs: bool,
    /// Log level
    pub log_level: String,
}

impl Default for TracingConfig {
    fn default() -> Self {
        Self {
            service_name: "nava-service".to_string(),
            service_version: env!("CARGO_PKG_VERSION").to_string(),
            environment: "development".to_string(),
            otlp_endpoint: None,
            sampling_ratio: 1.0,
            json_logs: false,
            log_level: "info".to_string(),
        }
    }
}

impl TracingConfig {
    /// Create config from environment variables
    pub fn from_env(service_name: &str) -> Self {
        Self {
            service_name: service_name.to_string(),
            service_version: std::env::var("SERVICE_VERSION")
                .unwrap_or_else(|_| env!("CARGO_PKG_VERSION").to_string()),
            environment: std::env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string()),
            otlp_endpoint: std::env::var("OTEL_EXPORTER_OTLP_ENDPOINT").ok(),
            sampling_ratio: std::env::var("OTEL_SAMPLING_RATIO")
                .ok()
                .and_then(|v| v.parse().ok())
                .unwrap_or(1.0),
            json_logs: std::env::var("JSON_LOGS")
                .map(|v| v == "true" || v == "1")
                .unwrap_or(false),
            log_level: std::env::var("RUST_LOG").unwrap_or_else(|_| "info".to_string()),
        }
    }
}

/// Initialize tracing with optional OpenTelemetry
pub fn init_tracing(config: TracingConfig) -> Result<(), Box<dyn std::error::Error>> {
    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.log_level));

    #[cfg(feature = "otel")]
    {
        if let Some(endpoint) = &config.otlp_endpoint {
            return init_otel_tracing(config, endpoint);
        }
    }

    // Standard tracing without OpenTelemetry
    if config.json_logs {
        Registry::default()
            .with(env_filter)
            .with(fmt::layer().json())
            .init();
    } else {
        Registry::default()
            .with(env_filter)
            .with(fmt::layer())
            .init();
    }

    Ok(())
}

#[cfg(feature = "otel")]
fn init_otel_tracing(
    config: TracingConfig,
    endpoint: &str,
) -> Result<(), Box<dyn std::error::Error>> {
    use opentelemetry_sdk::trace::TracerProvider;

    // Set up trace context propagation
    global::set_text_map_propagator(TraceContextPropagator::new());

    // Create OTLP exporter
    let exporter = opentelemetry_otlp::new_exporter()
        .tonic()
        .with_endpoint(endpoint)
        .with_timeout(Duration::from_secs(3));

    // Create tracer provider
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_exporter(exporter)
        .with_trace_config(
            trace::Config::default()
                .with_sampler(Sampler::TraceIdRatioBased(config.sampling_ratio))
                .with_id_generator(RandomIdGenerator::default())
                .with_resource(Resource::new(vec![
                    KeyValue::new("service.name", config.service_name.clone()),
                    KeyValue::new("service.version", config.service_version.clone()),
                    KeyValue::new("deployment.environment", config.environment.clone()),
                ])),
        )
        .install_batch(opentelemetry_sdk::runtime::Tokio)?;

    let otel_layer = OpenTelemetryLayer::new(tracer);

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new(&config.log_level));

    if config.json_logs {
        Registry::default()
            .with(env_filter)
            .with(fmt::layer().json())
            .with(otel_layer)
            .init();
    } else {
        Registry::default()
            .with(env_filter)
            .with(fmt::layer())
            .with(otel_layer)
            .init();
    }

    tracing::info!(
        service = %config.service_name,
        endpoint = %endpoint,
        "OpenTelemetry tracing initialized"
    );

    Ok(())
}

/// Shutdown tracing (flush pending spans)
pub fn shutdown_tracing() {
    #[cfg(feature = "otel")]
    {
        global::shutdown_tracer_provider();
    }
}

/// Extract trace context from HTTP headers for propagation
#[cfg(feature = "otel")]
pub fn extract_context(headers: &axum::http::HeaderMap) -> opentelemetry::Context {
    use opentelemetry::propagation::TextMapPropagator;

    let propagator = TraceContextPropagator::new();
    let extractor = HeaderExtractor(headers);
    propagator.extract(&extractor)
}

#[cfg(feature = "otel")]
struct HeaderExtractor<'a>(&'a axum::http::HeaderMap);

#[cfg(feature = "otel")]
impl<'a> opentelemetry::propagation::Extractor for HeaderExtractor<'a> {
    fn get(&self, key: &str) -> Option<&str> {
        self.0.get(key).and_then(|v| v.to_str().ok())
    }

    fn keys(&self) -> Vec<&str> {
        self.0.keys().map(|k| k.as_str()).collect()
    }
}

/// Inject trace context into HTTP headers for propagation
#[cfg(feature = "otel")]
pub fn inject_context(
    context: &opentelemetry::Context,
    headers: &mut axum::http::HeaderMap,
) {
    use opentelemetry::propagation::TextMapPropagator;

    let propagator = TraceContextPropagator::new();
    let mut injector = HeaderInjector(headers);
    propagator.inject_context(context, &mut injector);
}

#[cfg(feature = "otel")]
struct HeaderInjector<'a>(&'a mut axum::http::HeaderMap);

#[cfg(feature = "otel")]
impl<'a> opentelemetry::propagation::Injector for HeaderInjector<'a> {
    fn set(&mut self, key: &str, value: String) {
        if let Ok(header_name) = axum::http::header::HeaderName::try_from(key) {
            if let Ok(header_value) = axum::http::header::HeaderValue::from_str(&value) {
                self.0.insert(header_name, header_value);
            }
        }
    }
}

/// Tracing middleware for Axum
pub fn tracing_layer() -> tower_http::trace::TraceLayer<
    tower_http::classify::SharedClassifier<tower_http::classify::ServerErrorsAsFailures>,
> {
    use tower_http::trace::TraceLayer;

    TraceLayer::new_for_http()
        .make_span_with(|request: &axum::http::Request<_>| {
            let matched_path = request
                .extensions()
                .get::<axum::extract::MatchedPath>()
                .map(|p| p.as_str())
                .unwrap_or(request.uri().path());

            tracing::info_span!(
                "http_request",
                method = %request.method(),
                path = %matched_path,
                request_id = %uuid::Uuid::new_v4(),
            )
        })
        .on_response(|response: &axum::http::Response<_>, latency: Duration, _span: &tracing::Span| {
            tracing::info!(
                status = %response.status().as_u16(),
                latency_ms = %latency.as_millis(),
                "response"
            );
        })
        .on_failure(|error, latency: Duration, _span: &tracing::Span| {
            tracing::error!(
                error = %error,
                latency_ms = %latency.as_millis(),
                "request failed"
            );
        })
}
