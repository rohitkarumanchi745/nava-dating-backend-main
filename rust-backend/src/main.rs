mod auth;
mod config;
mod error;
mod graphql;
mod handlers;
mod models;
mod state;
mod vision;
mod websocket;

use async_graphql::http::GraphiQLSource;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    extract::Request,
    http::HeaderMap,
    middleware::{self, Next},
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use config::Config;
use redis::Client as RedisClient;
use sqlx::postgres::PgPoolOptions;
use state::{AppState, AppMetrics, CallSessions, ChatRooms};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::sync::atomic::Ordering;
use tokio::sync::{Mutex, RwLock};
use tower::ServiceBuilder;
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing::{error, info, warn, Span};
use vision::VisionAnalyzer;

use auth::decode_access_token;
use graphql::{build_schema, AppSchema};

use handlers::{
    // Auth
    health, send_otp, verify_otp,
    // Profile
    profile_me, profile_status, update_bio, update_profile, update_preferences,
    // Discovery & Matching
    discover, get_match, get_matches, like_user, pass_user,
    // Location
    get_my_location, get_nearby, purchase_pass, search_locations, update_location,
    // Student
    student_status, verify_student,
    // Calls
    create_call,
    // Spots
    create_spot, get_spots,
    // Vision
    verify_selfie, vision_analyze,
    // Admin
    admin_stats,
    // WebSocket
    ws_call, ws_chat,
    // ML Training
    update_user_embedding, get_user_embedding, get_batch_embeddings,
    update_bandit_arm, get_bandit_arm,
    log_reward, get_training_events, get_user_interactions,
    bulk_update_scores, update_spot_embedding,
    // Reels (private message based dating)
    create_reel, get_reel_feed, track_reel_view, like_reel, unlike_reel,
    send_reel_message, get_reel_inbox, reply_reel_message, mark_reel_message_read,
    get_reel_conversation, get_my_learned_patterns,
    // LLM Labeling
    queue_reel_labeling, get_labeling_batch, submit_reel_labels, submit_message_labels,
    submit_user_labels, mark_labeling_failed, get_reel_labels, export_training_snapshot,
    // Federated Learning
    register_fl_client, get_fl_round, submit_fl_update, start_fl_round,
    aggregate_fl_round, get_active_fl_model, report_local_data, get_ml_system_stats,
};

async fn metrics_middleware(
    State(state): axum::extract::State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    state.metrics.inc_requests();
    let response = next.run(request).await;
    state.metrics.dec_active_requests();

    if response.status().is_server_error() {
        state.metrics.inc_errors();
    }

    response
}

use axum::extract::State;

// Combined state for GraphQL routes
#[derive(Clone)]
struct GraphQLState {
    schema: AppSchema,
    app_state: AppState,
}

// GraphQL handler - extracts JWT and adds user_id to context
async fn graphql_handler(
    State(gql_state): State<GraphQLState>,
    headers: HeaderMap,
    req: GraphQLRequest,
) -> GraphQLResponse {
    let mut request = req.into_inner();

    // Extract user_id from Authorization header if present
    if let Some(auth_header) = headers.get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            let token = auth_str.trim_start_matches("Bearer ").trim();
            if !token.is_empty() {
                if let Ok(user_id) = decode_access_token(token, &gql_state.app_state.config.secret_key) {
                    // Convert i32 to i64 to match get_user_id_from_context expectation
                    request = request.data(i64::from(user_id));
                }
            }
        }
    }

    gql_state.schema.execute(request).await.into()
}

// GraphiQL playground (only in development)
async fn graphiql() -> impl IntoResponse {
    Html(GraphiQLSource::build().endpoint("/graphql").finish())
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();

    // Initialize tracing based on environment
    let config = Config::from_env();

    if config.is_production() {
        // JSON logging for production (better for log aggregators)
        tracing_subscriber::fmt()
            .json()
            .with_env_filter(
                std::env::var("RUST_LOG").unwrap_or_else(|_| "info,sqlx=warn".to_string()),
            )
            .init();
    } else {
        // Pretty logging for development
        tracing_subscriber::fmt()
            .with_env_filter(
                std::env::var("RUST_LOG").unwrap_or_else(|_| "info,sqlx=warn".to_string()),
            )
            .init();
    }

    // Validate configuration
    if let Err(e) = config.validate() {
        error!("Configuration error: {}", e);
        std::process::exit(1);
    }

    info!(
        "Starting {} server (env: {})",
        env!("CARGO_PKG_NAME"),
        config.environment
    );

    // Database connection pool with production settings
    let db = PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .min_connections(config.db_min_connections)
        .acquire_timeout(Duration::from_secs(config.db_acquire_timeout_secs))
        .idle_timeout(Duration::from_secs(config.db_idle_timeout_secs))
        .test_before_acquire(true)
        .connect(&config.database_url)
        .await
        .unwrap_or_else(|err| {
            error!("Failed to connect to database: {err}");
            std::process::exit(1);
        });

    info!("Connected to database (pool: {}-{} connections)",
          config.db_min_connections, config.db_max_connections);

    // Redis connection (optional but recommended for production)
    let redis = match RedisClient::open(config.redis_url.as_str()) {
        Ok(client) => match redis::aio::ConnectionManager::new(client).await {
            Ok(manager) => {
                info!("Connected to Redis");
                Some(manager)
            }
            Err(err) => {
                warn!("Failed to connect to Redis: {err}. Continuing without caching.");
                None
            }
        },
        Err(err) => {
            warn!("Invalid Redis URL: {err}. Continuing without caching.");
            None
        }
    };

    // Vision models
    let bind_addr = config.bind_addr.clone();
    let request_timeout = config.request_timeout_secs;
    let shutdown_timeout = config.shutdown_timeout_secs;

    let vision = if config.vision_enabled {
        match VisionAnalyzer::load(&config) {
            Ok(analyzer) => {
                info!("Vision models loaded successfully");
                Some(Arc::new(Mutex::new(analyzer)))
            }
            Err(err) => {
                warn!("Failed to load vision models: {err}. Continuing without vision features.");
                None
            }
        }
    } else {
        info!("Vision features disabled");
        None
    };

    let state = AppState {
        db,
        redis,
        config,
        vision,
        chat_rooms: Arc::new(RwLock::new(ChatRooms::new())),
        call_sessions: Arc::new(RwLock::new(CallSessions::new())),
        metrics: Arc::new(AppMetrics::new()),
        start_time: Instant::now(),
    };

    // Build GraphQL schema
    let schema = build_schema(state.clone());
    let is_dev = !state.config.is_production();
    info!("GraphQL schema built (playground: {})", if is_dev { "enabled" } else { "disabled" });

    let gql_state = GraphQLState {
        schema,
        app_state: state.clone(),
    };

    // Build GraphQL routes
    let graphql_routes = if is_dev {
        Router::new()
            .route("/graphql", post(graphql_handler))
            .route("/graphql", get(graphiql))
            .with_state(gql_state)
    } else {
        Router::new()
            .route("/graphql", post(graphql_handler))
            .with_state(gql_state)
    };

    // Build the application with middleware stack
    let app = Router::new()
        // Health & Metrics
        .route("/health", get(health))
        .route("/ready", get(readiness_check))
        .route("/metrics", get(prometheus_metrics))
        // Auth
        .route("/send-otp", post(send_otp))
        .route("/verify-otp", post(verify_otp))
        // Profile
        .route("/update-profile", post(update_profile))
        .route("/update-bio", post(update_bio))
        .route("/profile/status", get(profile_status))
        .route("/profile/me", get(profile_me))
        .route("/preferences", post(update_preferences))
        // Discovery & Matching
        .route("/discover", get(discover))
        .route("/profiles/discover", get(discover))
        .route("/match/like", post(like_user))
        .route("/match/pass", post(pass_user))
        .route("/profiles/like", post(like_user))
        .route("/profiles/pass", post(pass_user))
        .route("/match/{match_id}", get(get_match))
        .route("/matches", get(get_matches))
        // Location
        .route("/location/update", post(update_location))
        .route("/location/search", get(search_locations))
        .route("/location/nearby", get(get_nearby))
        .route("/location/purchase-pass", post(purchase_pass))
        .route("/me/location", get(get_my_location))
        // Student
        .route("/student/verify", post(verify_student))
        .route("/student/status", get(student_status))
        // Calls
        .route("/calls", post(create_call))
        // Spots
        .route("/spots", post(create_spot))
        .route("/spots", get(get_spots))
        // Vision/Verification
        .route("/verify/selfie", post(verify_selfie))
        .route("/vision/analyze", post(vision_analyze))
        // Admin
        .route("/admin/stats", get(admin_stats))
        // WebSocket
        .route("/ws/chat", get(ws_chat))
        .route("/ws/call", get(ws_call))
        // ML Training Endpoints
        .route("/ml/embeddings", post(update_user_embedding))
        .route("/ml/embeddings", get(get_user_embedding))
        .route("/ml/embeddings/batch", post(get_batch_embeddings))
        .route("/ml/bandit", post(update_bandit_arm))
        .route("/ml/bandit", get(get_bandit_arm))
        .route("/ml/reward", post(log_reward))
        .route("/ml/events", get(get_training_events))
        .route("/ml/interactions", get(get_user_interactions))
        .route("/ml/scores/bulk", post(bulk_update_scores))
        .route("/ml/spots/embedding", post(update_spot_embedding))
        // Reels (private message based dating)
        .route("/reels", post(create_reel))
        .route("/reels/feed", get(get_reel_feed))
        .route("/reels/view", post(track_reel_view))
        .route("/reels/like", post(like_reel))
        .route("/reels/unlike", post(unlike_reel))
        .route("/reels/message", post(send_reel_message))
        .route("/reels/inbox", get(get_reel_inbox))
        .route("/reels/reply", post(reply_reel_message))
        .route("/reels/message/read", post(mark_reel_message_read))
        .route("/reels/conversation", get(get_reel_conversation))
        .route("/reels/patterns", get(get_my_learned_patterns))
        // LLM Labeling System
        .route("/llm/queue", post(queue_reel_labeling))
        .route("/llm/batch", get(get_labeling_batch))
        .route("/llm/labels/reel", post(submit_reel_labels))
        .route("/llm/labels/reel", get(get_reel_labels))
        .route("/llm/labels/message", post(submit_message_labels))
        .route("/llm/labels/user", post(submit_user_labels))
        .route("/llm/failed", post(mark_labeling_failed))
        .route("/llm/export", post(export_training_snapshot))
        // Federated Learning System
        .route("/fl/register", post(register_fl_client))
        .route("/fl/round", get(get_fl_round))
        .route("/fl/round/start", post(start_fl_round))
        .route("/fl/update", post(submit_fl_update))
        .route("/fl/aggregate", post(aggregate_fl_round))
        .route("/fl/model", get(get_active_fl_model))
        .route("/fl/local-data", post(report_local_data))
        // ML System Stats
        .route("/ml/stats", get(get_ml_system_stats))
        .with_state(state.clone())
        // Merge GraphQL routes
        .merge(graphql_routes)
        // Middleware stack (order matters - bottom runs first)
        // CORS layer (must be applied first to handle preflight requests)
        .layer(
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any),
        )
        // Request tracing
        .layer(
            TraceLayer::new_for_http()
                .on_request(|request: &Request<_>, _span: &Span| {
                    tracing::info!(
                        method = %request.method(),
                        uri = %request.uri(),
                        "request"
                    );
                })
                .on_response(|response: &Response<_>, latency: Duration, _span: &Span| {
                    tracing::info!(
                        status = %response.status(),
                        latency = ?latency,
                        "response"
                    );
                })
        )
        // Request timeout
        .layer(TimeoutLayer::new(Duration::from_secs(request_timeout)))
        // Compression for responses
        .layer(CompressionLayer::new())
        // Request ID propagation
        .layer(SetRequestIdLayer::x_request_id(MakeRequestUuid))
        .layer(PropagateRequestIdLayer::x_request_id())
        .layer(middleware::from_fn_with_state(state.clone(), metrics_middleware));

    info!("Starting server on {}", bind_addr);
    let listener = tokio::net::TcpListener::bind(&bind_addr)
        .await
        .unwrap_or_else(|err| {
            error!("Failed to bind to {}: {err}", bind_addr);
            std::process::exit(1);
        });

    info!("Server listening on {} (timeout: {}s)", bind_addr, request_timeout);

    // Graceful shutdown handling
    let shutdown_signal = async {
        let ctrl_c = async {
            tokio::signal::ctrl_c()
                .await
                .expect("Failed to install Ctrl+C handler");
        };

        #[cfg(unix)]
        let terminate = async {
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())
                .expect("Failed to install SIGTERM handler")
                .recv()
                .await;
        };

        #[cfg(not(unix))]
        let terminate = std::future::pending::<()>();

        tokio::select! {
            _ = ctrl_c => info!("Received Ctrl+C, initiating graceful shutdown..."),
            _ = terminate => info!("Received SIGTERM, initiating graceful shutdown..."),
        }
    };

    // Run server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal)
        .await
        .unwrap_or_else(|err| {
            error!("Server error: {err}");
        });

    info!("Shutting down... (timeout: {}s)", shutdown_timeout);

    // Give in-flight requests time to complete
    tokio::time::sleep(Duration::from_secs(shutdown_timeout)).await;

    info!("Server shutdown complete");
}

// Readiness check - verifies database connectivity
async fn readiness_check(
    State(state): State<AppState>,
) -> axum::response::Result<axum::Json<serde_json::Value>, error::AppError> {
    // Check database
    sqlx::query("SELECT 1")
        .fetch_one(&state.db)
        .await
        .map_err(|_| error::AppError::internal("Database not ready"))?;

    // Check Redis if available
    let redis_status = if state.redis.is_some() {
        "connected"
    } else {
        "disabled"
    };

    Ok(axum::Json(serde_json::json!({
        "status": "ready",
        "database": "connected",
        "redis": redis_status
    })))
}

// Prometheus-compatible metrics endpoint
async fn prometheus_metrics(
    State(state): State<AppState>,
) -> String {
    let uptime = state.start_time.elapsed().as_secs();
    let metrics = &state.metrics;

    format!(
        r#"# HELP app_requests_total Total number of requests
# TYPE app_requests_total counter
app_requests_total {}

# HELP app_requests_active Currently active requests
# TYPE app_requests_active gauge
app_requests_active {}

# HELP app_errors_total Total number of errors
# TYPE app_errors_total counter
app_errors_total {}

# HELP app_db_queries_total Total database queries
# TYPE app_db_queries_total counter
app_db_queries_total {}

# HELP app_cache_hits Total cache hits
# TYPE app_cache_hits counter
app_cache_hits {}

# HELP app_cache_misses Total cache misses
# TYPE app_cache_misses counter
app_cache_misses {}

# HELP app_websocket_connections Active WebSocket connections
# TYPE app_websocket_connections gauge
app_websocket_connections {}

# HELP app_uptime_seconds Server uptime in seconds
# TYPE app_uptime_seconds counter
app_uptime_seconds {}
"#,
        metrics.requests_total.load(Ordering::Relaxed),
        metrics.requests_active.load(Ordering::Relaxed),
        metrics.errors_total.load(Ordering::Relaxed),
        metrics.db_queries_total.load(Ordering::Relaxed),
        metrics.cache_hits.load(Ordering::Relaxed),
        metrics.cache_misses.load(Ordering::Relaxed),
        metrics.websocket_connections.load(Ordering::Relaxed),
        uptime,
    )
}
