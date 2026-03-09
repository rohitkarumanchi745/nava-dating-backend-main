mod auth;
mod config;
mod error;
mod graphql;
mod handlers;
mod jobs;
mod middleware;
mod models;
mod redis_service;
mod services;
mod state;
mod storage;
mod telemetry;
mod ml;
mod vision;
mod websocket;

use async_graphql::http::GraphiQLSource;
use async_graphql_axum::{GraphQLRequest, GraphQLResponse};
use axum::{
    extract::Request,
    http::HeaderMap,
    middleware as axum_middleware,
    response::{Html, IntoResponse, Response},
    routing::{get, post},
    Router,
};
use axum_middleware::Next;
use config::Config;
// neo4rs Bolt imports commented out - using HTTP API for Neo4j 5.x compatibility
// use neo4rs::{Graph, ConfigBuilder as Neo4jConfigBuilder};
use redis::Client as RedisClient;
use sqlx::postgres::PgPoolOptions;
use state::{AppState, AppMetrics, CallSessions, ChatRooms};
use std::sync::Arc;
use std::time::{Duration, Instant};
use std::sync::atomic::Ordering;
use tokio::sync::{Mutex, RwLock};
use tower_http::{
    compression::CompressionLayer,
    cors::{Any, CorsLayer},
    request_id::{MakeRequestUuid, PropagateRequestIdLayer, SetRequestIdLayer},
    timeout::TimeoutLayer,
    trace::TraceLayer,
};
use tracing::{error, info, warn, Span};
use vision::VisionAnalyzer;
use services::graph_service::GraphService;
use services::payments::PaymentService;
use services::ads::AdsService;
use services::trust_safety::TrustSafetyService;
use services::moderation::ModerationPipeline;
use services::freshness::FreshnessDecayJob;
use middleware::dual_write::DualWriteManager;
use middleware::security::security_headers_middleware;
use handlers::graph_handlers::graph_routes;
use jobs::sync_job::{SyncJobRunner, SyncJobConfig};
use jobs::dlq_processor::{DlqProcessorRunner, DlqProcessorConfig};

use auth::decode_access_token;
use redis_service::RedisService;
use graphql::{build_schema, AppSchema};

use handlers::{
    // Health & Probes
    health, health_detailed, readiness_probe, liveness_probe,
    // Auth
    send_otp, verify_otp,
    // Profile
    profile_me, profile_status, update_bio, update_profile, update_preferences,
    // Voice Intro
    upload_voice_intro, upload_voice_intro_json, track_voice_play,
    // Discovery & Matching
    discover, get_match, get_matches, like_user, pass_user,
    // Location
    get_my_location, get_nearby, purchase_pass, search_locations, update_location,
    // Student
    student_status, verify_student, verify_student_otp,
    // University Discovery
    search_universities, get_university_countries, discover_university_profiles,
    get_university_reels, purchase_university_pass, get_my_university_passes,
    // Calls
    create_call,
    // Spots
    create_spot, get_spots,
    // Vision
    verify_selfie, vision_analyze,
    // Admin
    admin_stats, secrets_status,
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
    aggregate_fl_round, get_active_fl_model, report_local_data, get_fl_training_data,
    get_ml_system_stats,
    // RevenueCat Subscriptions
    sync_subscription, revenuecat_webhook,
    // Web Payments (Razorpay + Stripe)
    payments::{
        create_order as payment_create_order,
        verify_payment, verify_apple_payment, list_products, get_subscriptions,
        cancel_subscription, razorpay_webhook, stripe_webhook,
        dlq_stats, delete_account,
    },
    // Ads
    ads::{request_ad, record_impression, rewarded_complete, get_rewards_balance},
    // Ambassador Program
    ambassador::{
        ambassador_login, get_ambassador_profile, get_performance, get_daily_breakdown,
        get_hourly_breakdown, get_leaderboard, get_subscribers, get_active_subscribers,
        admin_get_daily_signups, record_referral, verify_referral,
    },
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

/// Rate limiting middleware using Redis
async fn rate_limit_middleware(
    State(state): axum::extract::State<AppState>,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    // Get identifier (IP or user ID)
    let identifier = if let Some(auth_header) = headers.get("authorization") {
        if let Ok(auth_str) = auth_header.to_str() {
            let token = auth_str.trim_start_matches("Bearer ").trim();
            if !token.is_empty() {
                if let Ok(user_id) = decode_access_token(token, &state.config.secret_key) {
                    format!("user:{}", user_id)
                } else {
                    get_client_ip(&headers)
                }
            } else {
                get_client_ip(&headers)
            }
        } else {
            get_client_ip(&headers)
        }
    } else {
        get_client_ip(&headers)
    };

    // Check rate limit if Redis is available
    if let Some(ref redis) = state.redis {
        let redis_service = RedisService::new(redis.clone());
        match redis_service
            .check_rate_limit(
                &identifier,
                state.config.rate_limit_requests_per_minute + state.config.rate_limit_burst,
                60,
            )
            .await
        {
            Ok((allowed, remaining, reset_in)) => {
                if !allowed {
                    // Return 429 Too Many Requests
                    let response = axum::http::Response::builder()
                        .status(axum::http::StatusCode::TOO_MANY_REQUESTS)
                        .header("X-RateLimit-Limit", state.config.rate_limit_requests_per_minute.to_string())
                        .header("X-RateLimit-Remaining", "0")
                        .header("X-RateLimit-Reset", reset_in.to_string())
                        .header("Retry-After", reset_in.to_string())
                        .header(axum::http::header::CONTENT_TYPE, "application/json")
                        .body(axum::body::Body::from(r#"{"error":"Too many requests. Please slow down."}"#))
                        .unwrap_or_else(|_| {
                            axum::http::Response::new(axum::body::Body::from(r#"{"error":"Rate limited"}"#))
                        });

                    return response.into_response();
                }

                // Add rate limit headers to successful response
                let mut response = next.run(request).await;
                let headers = response.headers_mut();
                // These header value parses should never fail for numeric strings
                if let Ok(v) = state.config.rate_limit_requests_per_minute.to_string().parse() {
                    headers.insert("X-RateLimit-Limit", v);
                }
                if let Ok(v) = remaining.to_string().parse() {
                    headers.insert("X-RateLimit-Remaining", v);
                }
                if let Ok(v) = reset_in.to_string().parse() {
                    headers.insert("X-RateLimit-Reset", v);
                }
                return response;
            }
            Err(e) => {
                warn!("Rate limit check failed: {}. Allowing request.", e);
            }
        }
    }

    // If Redis is unavailable, allow the request
    next.run(request).await
}

/// Extract client IP from headers
fn get_client_ip(headers: &HeaderMap) -> String {
    // Check X-Forwarded-For first (for proxies/load balancers)
    if let Some(xff) = headers.get("x-forwarded-for") {
        if let Ok(xff_str) = xff.to_str() {
            if let Some(ip) = xff_str.split(',').next() {
                return format!("ip:{}", ip.trim());
            }
        }
    }

    // Check X-Real-IP
    if let Some(real_ip) = headers.get("x-real-ip") {
        if let Ok(ip) = real_ip.to_str() {
            return format!("ip:{}", ip.trim());
        }
    }

    // Fallback
    "ip:unknown".to_string()
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

    // Load configuration first
    let config = Config::from_env();

    // Initialize telemetry (with optional OpenTelemetry support)
    // Enable distributed tracing with: cargo build --features otel
    telemetry::init_telemetry("nava-backend", config.is_production());

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

    // Database connection pool with production settings (scaled for 10k+ users)
    let mut pool_options = PgPoolOptions::new()
        .max_connections(config.db_max_connections)
        .min_connections(config.db_min_connections)
        .acquire_timeout(Duration::from_secs(config.db_acquire_timeout_secs))
        .idle_timeout(Duration::from_secs(config.db_idle_timeout_secs))
        .max_lifetime(Duration::from_secs(config.db_max_lifetime_secs));

    // PgBouncer compatibility: disable test_before_acquire for transaction pooling
    if !config.pgbouncer_mode {
        pool_options = pool_options.test_before_acquire(true);
    }

    let db = pool_options
        .connect(&config.database_url)
        .await
        .unwrap_or_else(|err| {
            error!("Failed to connect to database: {err}");
            std::process::exit(1);
        });

    // Statement timeout: set per-connection only when NOT using PgBouncer.
    // With PgBouncer (transaction pooling), session-level SET is lost after each txn.
    // In that case, rely on postgresql.conf: statement_timeout = <value>.
    if !config.pgbouncer_mode {
        if let Err(e) = sqlx::query(&format!(
            "SET statement_timeout = '{}ms'",
            config.db_statement_timeout_ms
        ))
        .execute(&db)
        .await
        {
            warn!("Failed to set statement_timeout: {e}");
        }
    }

    info!(
        "Connected to database (pool: {}-{} connections, pgbouncer: {}, stmt_timeout: {}ms{}, instance: {})",
        config.db_min_connections,
        config.db_max_connections,
        config.pgbouncer_mode,
        config.db_statement_timeout_ms,
        if config.pgbouncer_mode { " [via postgresql.conf]" } else { " [per-session]" },
        config.instance_id
    );

    // Read replica pool (optional, for read-heavy workloads)
    let db_read_replica = if config.db_read_replica_enabled {
        if let Some(ref replica_url) = config.db_read_replica_url {
            match PgPoolOptions::new()
                .max_connections(config.db_read_max_connections)
                .min_connections(config.db_read_min_connections)
                .acquire_timeout(Duration::from_secs(config.db_acquire_timeout_secs))
                .idle_timeout(Duration::from_secs(config.db_idle_timeout_secs))
                .connect(replica_url)
                .await
            {
                Ok(pool) => {
                    info!("Connected to read replica database");
                    Some(pool)
                }
                Err(err) => {
                    warn!("Failed to connect to read replica: {err}. Using primary for reads.");
                    None
                }
            }
        } else {
            None
        }
    } else {
        None
    };
    // db_read_replica is stored in AppState and accessed via state.read_pool()

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

    // Neo4j connection (optional - for graph-powered features)
    // Using HTTP API for Neo4j 5.x compatibility (Bolt protocol has auth issues with neo4rs)
    let neo4j_uri = std::env::var("NEO4J_URI").ok();
    let neo4j_user = std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".to_string());
    let neo4j_password = std::env::var("NEO4J_PASSWORD").ok();

    let (neo4j, graph_service) = if let (Some(uri), Some(password)) = (neo4j_uri, neo4j_password) {
        // Parse host from URI
        let uri_clean = uri.replace("bolt://", "").replace("neo4j://", "");
        let host = uri_clean.split(':').next().unwrap_or("127.0.0.1");

        // Create HTTP-based Neo4j service (works with Neo4j 5.x)
        let service = GraphService::with_http(host, &neo4j_user, &password, db.clone());

        // Test connection
        if service.health_check().await.neo4j_healthy {
            info!("Connected to Neo4j at {} via HTTP API", uri);
            (None, Some(Arc::new(service)))
        } else {
            warn!("Failed to connect to Neo4j HTTP API. Continuing without graph features.");
            (None, None)
        }
    } else {
        info!("Neo4j not configured. Graph features disabled.");
        (None, None)
    };

    // Initialize dual-write manager for fault tolerance
    let dual_write = Arc::new(DualWriteManager::new());

    // Create WebSocket managers with scaled buffer sizes from config
    let chat_rooms = ChatRooms::with_buffer_size(config.ws_chat_buffer_size);
    let call_sessions = CallSessions::with_buffer_size(config.ws_call_buffer_size);

    info!(
        "WebSocket buffers configured (chat: {}, call: {})",
        config.ws_chat_buffer_size, config.ws_call_buffer_size
    );

    // Initialize Payment Service (Razorpay + Stripe)
    let payment_service = {
        let razorpay_key_id = if !config.razorpay_key_id.is_empty() {
            Some(config.razorpay_key_id.clone())
        } else {
            None
        };
        let razorpay_key_secret = if !config.razorpay_key_secret.is_empty() {
            Some(config.razorpay_key_secret.clone())
        } else {
            None
        };
        let stripe_secret = if !config.stripe_secret_key.is_empty() {
            Some(config.stripe_secret_key.clone())
        } else {
            None
        };
        let stripe_publishable = if !config.stripe_publishable_key.is_empty() {
            Some(config.stripe_publishable_key.clone())
        } else {
            None
        };
        let stripe_webhook = if !config.stripe_webhook_secret.is_empty() {
            Some(config.stripe_webhook_secret.clone())
        } else {
            None
        };

        let service = PaymentService::new(
            razorpay_key_id,
            razorpay_key_secret,
            stripe_secret,
            stripe_publishable,
            stripe_webhook,
        );

        if service.has_razorpay() || service.has_stripe() {
            info!(
                "Payment service initialized (Razorpay: {}, Stripe: {})",
                service.has_razorpay(),
                service.has_stripe()
            );
            Some(Arc::new(service))
        } else {
            warn!("No payment gateways configured. Payment features disabled.");
            None
        }
    };

    // Initialize Ads Service
    let ads_service = if config.ads_enabled {
        info!("Ads service initialized");
        Some(Arc::new(AdsService::new(db.clone())))
    } else {
        info!("Ads service disabled");
        None
    };

    // Initialize ML service with warm-start from DB (before db moves into AppState)
    let ml_service = ml::MlService::new(&db).await;

    // Initialize Trust & Safety service
    let trust_safety = if config.trust_safety_enabled {
        info!("Trust & Safety service initialized (auto-ban threshold: {})", config.trust_safety_auto_ban_threshold);
        Some(Arc::new(TrustSafetyService::new(config.trust_safety_auto_ban_threshold)))
    } else {
        info!("Trust & Safety service disabled");
        None
    };

    // Initialize Content Moderation pipeline
    let moderation = if config.moderation_enabled {
        info!("Content moderation pipeline initialized");
        Some(Arc::new(ModerationPipeline::new(
            config.moderation_toxicity_threshold,
            config.moderation_nsfw_threshold as f32,
        )))
    } else {
        info!("Content moderation disabled");
        None
    };

    let state = AppState {
        db,
        db_read: db_read_replica,
        redis,
        neo4j,
        graph_service,
        dual_write: dual_write.clone(),
        config,
        vision,
        chat_rooms: Arc::new(RwLock::new(chat_rooms)),
        call_sessions: Arc::new(RwLock::new(call_sessions)),
        metrics: Arc::new(AppMetrics::new()),
        start_time: Instant::now(),
        payment_service,
        ads_service,
        ml: Arc::new(RwLock::new(ml_service)),
        trust_safety,
        moderation,
    };

    // Run startup sync if Neo4j is connected
    if state.graph_service.is_some() {
        if let Err(e) = jobs::sync_job::run_startup_sync(&state).await {
            warn!("Startup sync failed: {}. Will sync in background.", e);
        }

        // Start background sync jobs
        let sync_runner = SyncJobRunner::new(Arc::new(state.clone()), SyncJobConfig::default());
        tokio::spawn(async move {
            sync_runner.start().await;
        });
        info!("Background sync jobs started");
    }

    // Start DLQ processor for payment retry handling
    if state.payment_service.is_some() {
        let dlq_runner = DlqProcessorRunner::new(Arc::new(state.clone()), DlqProcessorConfig::default());
        tokio::spawn(async move {
            dlq_runner.start().await;
        });
        info!("DLQ processor started for payment retries");
    }

    // Start freshness decay background job
    if state.config.freshness_decay_enabled {
        FreshnessDecayJob::spawn(state.db.clone());
        info!("Freshness decay job started (interval: 6h)");
    }

    // Start instance heartbeat for horizontal scaling (if Redis is available)
    if let Some(ref redis_conn) = state.redis {
        let redis_service = RedisService::new(redis_conn.clone());
        let instance_id = state.config.instance_id.clone();
        tokio::spawn(async move {
            loop {
                if let Err(e) = redis_service.register_instance(&instance_id).await {
                    warn!("Failed to register instance heartbeat: {}", e);
                }
                // Heartbeat every 15 seconds (TTL is 30 seconds)
                tokio::time::sleep(Duration::from_secs(15)).await;
            }
        });
        info!("Instance heartbeat started (id: {})", state.config.instance_id);
    }

    // Start replica lag monitor (if read replica is configured)
    if state.db_read.is_some() {
        let replica_pool = state.db_read.clone().unwrap();
        let metrics = state.metrics.clone();
        tokio::spawn(async move {
            const LAG_CUTOFF_SECS: f64 = 2.0;
            loop {
                let lag = sqlx::query_scalar::<_, Option<f64>>(
                    "SELECT CASE WHEN pg_is_in_recovery() THEN \
                     EXTRACT(EPOCH FROM (now() - pg_last_xact_replay_timestamp())) \
                     ELSE 0 END"
                )
                .fetch_optional(&replica_pool)
                .await
                .ok()
                .flatten()
                .flatten()
                .unwrap_or(f64::MAX);

                let healthy = lag < LAG_CUTOFF_SECS;
                metrics.replica_healthy.store(healthy, Ordering::Relaxed);
                metrics.replica_lag_ms.store((lag * 1000.0) as u64, Ordering::Relaxed);

                if !healthy {
                    warn!(lag_secs = lag, cutoff = LAG_CUTOFF_SECS, "Read replica lag exceeds cutoff, routing to primary");
                }

                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });
        info!("Replica lag monitor started (cutoff: 2s, check interval: 5s)");
    }

    // Build GraphQL schema
    let schema = build_schema(state.clone());
    let is_dev = !state.config.is_production();
    info!("GraphQL schema built (playground: {})", if is_dev { "enabled" } else { "disabled" });

    // Build CORS layer based on environment
    let cors_layer = if is_dev {
        // Development: allow all origins
        CorsLayer::new()
            .allow_origin(Any)
            .allow_methods(Any)
            .allow_headers(Any)
    } else {
        // Production: restrict to configured origins
        let origins = state.config.cors_allowed_origins.clone();
        if origins.len() == 1 && origins[0] == "*" {
            CorsLayer::new()
                .allow_origin(Any)
                .allow_methods(Any)
                .allow_headers(Any)
        } else {
            let allowed: Vec<axum::http::HeaderValue> = origins
                .iter()
                .filter_map(|o| o.parse().ok())
                .collect();
            CorsLayer::new()
                .allow_origin(allowed)
                .allow_methods(Any)
                .allow_headers(Any)
                .allow_credentials(true)
        }
    };

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
        // Health & Metrics (enhanced for load balancers)
        .route("/health", get(health))
        .route("/health/detailed", get(health_detailed))  // Detailed health for monitoring
        .route("/ready", get(readiness_probe))            // K8s readiness probe
        .route("/live", get(liveness_probe))              // K8s liveness probe
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
        // Voice Intro
        .route("/voice-intro", post(upload_voice_intro))
        .route("/voice-intro/url", post(upload_voice_intro_json))
        .route("/voice-intro/track", post(track_voice_play))
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
        // Subscriptions (RevenueCat)
        .route("/subscriptions/sync", post(sync_subscription))
        .route("/webhooks/revenuecat", post(revenuecat_webhook))
        // Web Payments (Razorpay + Stripe) - bypasses 30% App Store fees!
        .route("/api/payments/create-order", post(payment_create_order))
        .route("/api/payments/verify", post(verify_payment))
        .route("/api/payments/products", get(list_products))
        .route("/api/payments/subscriptions", get(get_subscriptions))
        .route("/api/payments/subscriptions/cancel", post(cancel_subscription))
        .route("/api/payments/verify-apple", post(verify_apple_payment))
        .route("/api/payments/dlq/stats", get(dlq_stats))
        .route("/webhook/razorpay", post(razorpay_webhook))
        .route("/webhook/stripe", post(stripe_webhook))
        // Account Management
        .route("/account/delete", post(delete_account))
        // Ads Monetization
        .route("/api/ads/request", get(request_ad))
        .route("/api/ads/impression", post(record_impression))
        .route("/api/ads/rewarded-complete", post(rewarded_complete))
        .route("/api/ads/rewards-balance", get(get_rewards_balance))
        // Ambassador Program
        .route("/api/ambassador/login", post(ambassador_login))
        .route("/api/ambassador/me", get(get_ambassador_profile))
        .route("/api/ambassador/performance", get(get_performance))
        .route("/api/ambassador/daily", get(get_daily_breakdown))
        .route("/api/ambassador/hourly", get(get_hourly_breakdown))
        .route("/api/ambassador/leaderboard", get(get_leaderboard))
        .route("/api/ambassador/subscribers", get(get_subscribers))
        .route("/api/ambassador/subscribers/active", get(get_active_subscribers))
        .route("/api/admin/ambassadors/daily", get(admin_get_daily_signups))
        .route("/api/referral/record", post(record_referral))
        .route("/api/referral/verify", post(verify_referral))
        // Student
        .route("/student/verify", post(verify_student))
        .route("/student/verify-otp", post(verify_student_otp))
        .route("/student/status", get(student_status))
        // University Discovery
        .route("/universities/search", get(search_universities))
        .route("/universities/countries", get(get_university_countries))
        .route("/universities/discover", get(discover_university_profiles))
        .route("/universities/reels", get(get_university_reels))
        .route("/universities/passes", get(get_my_university_passes))
        .route("/universities/passes", post(purchase_university_pass))
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
        .route("/admin/secrets/status", get(secrets_status))
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
        .route("/fl/training-data", get(get_fl_training_data))
        // ML Computation Endpoints
        .route("/ml/rl/rank", post(handlers::ml_rank_candidates))
        .route("/ml/linucb/score", post(handlers::ml_linucb_score))
        // ML System Stats
        .route("/ml/stats", get(get_ml_system_stats))
        .with_state(Arc::new(state.clone()))
        // Graph-powered endpoints (Neo4j + PostgreSQL fallback)
        .nest("/api/graph", graph_routes().with_state(Arc::new(state.clone())))
        // Merge GraphQL routes
        .merge(graphql_routes)
        // Middleware stack (order matters - bottom runs first)
        // CORS layer (must be applied first to handle preflight requests)
        .layer(cors_layer)
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
        .layer(axum_middleware::from_fn_with_state(state.clone(), metrics_middleware))
        // Rate limiting (Redis-based)
        .layer(axum_middleware::from_fn_with_state(state.clone(), rate_limit_middleware))
        // Security headers (HSTS, X-Frame-Options, etc.)
        .layer(axum_middleware::from_fn(security_headers_middleware));

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

    // Shutdown OpenTelemetry tracer (flushes remaining spans)
    telemetry::shutdown_telemetry();

    info!("Server shutdown complete");
}

// Prometheus-compatible metrics endpoint
async fn prometheus_metrics(
    State(state): State<AppState>,
) -> String {
    let uptime = state.start_time.elapsed().as_secs();
    let metrics = &state.metrics;

    // Fetch DLQ stats from database
    let dlq_stats = fetch_dlq_metrics(&state.db).await;

    // Fetch database pool stats
    let db_pool_size = state.db.size();
    let db_pool_idle = state.db.num_idle();
    let db_read_pool_size = state.db_read.as_ref().map(|p| p.size()).unwrap_or(0);
    let db_read_pool_idle = state.db_read.as_ref().map(|p| p.num_idle() as u32).unwrap_or(0);

    // Fetch ML stats
    let ml_stats = {
        let ml = state.ml.read().await;
        ml.ml_stats()
    };
    let ml_avg_scoring_us = ml_stats.get("rl_agent")
        .and_then(|v| v.get("avg_scoring_latency_us"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let ml_epsilon = ml_stats.get("rl_agent")
        .and_then(|v| v.get("epsilon"))
        .and_then(|v| v.as_f64())
        .unwrap_or(0.0);
    let ml_train_counter = ml_stats.get("train_counter")
        .and_then(|v| v.as_u64())
        .unwrap_or(0);

    // Get WebSocket room stats
    let (chat_rooms, chat_subscribers) = {
        let rooms = state.chat_rooms.read().await;
        (rooms.room_count(), rooms.total_subscribers())
    };

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
# TYPE app_uptime_seconds gauge
app_uptime_seconds {}

# HELP app_db_pool_size Database connection pool size
# TYPE app_db_pool_size gauge
app_db_pool_size {}

# HELP app_db_pool_idle Idle database connections
# TYPE app_db_pool_idle gauge
app_db_pool_idle {}

# HELP app_chat_rooms_active Active chat rooms
# TYPE app_chat_rooms_active gauge
app_chat_rooms_active {}

# HELP app_chat_subscribers_total Total chat subscribers
# TYPE app_chat_subscribers_total gauge
app_chat_subscribers_total {}

# HELP app_db_read_pool_size Read replica connection pool size
# TYPE app_db_read_pool_size gauge
app_db_read_pool_size {}

# HELP app_db_read_pool_idle Idle read replica connections
# TYPE app_db_read_pool_idle gauge
app_db_read_pool_idle {}

# HELP app_replica_lag_ms Read replica replication lag in milliseconds
# TYPE app_replica_lag_ms gauge
app_replica_lag_ms {}

# HELP app_replica_healthy Whether the read replica is healthy (1=yes, 0=no)
# TYPE app_replica_healthy gauge
app_replica_healthy {}

# HELP app_reads_from_replica Total reads served from read replica
# TYPE app_reads_from_replica counter
app_reads_from_replica {}

# HELP app_reads_fallback_to_primary Total reads that fell back to primary
# TYPE app_reads_fallback_to_primary counter
app_reads_fallback_to_primary {}

# HELP app_ml_avg_scoring_latency_us ML scoring average latency in microseconds
# TYPE app_ml_avg_scoring_latency_us gauge
app_ml_avg_scoring_latency_us {}

# HELP app_ml_epsilon RL agent exploration rate
# TYPE app_ml_epsilon gauge
app_ml_epsilon {}

# HELP app_ml_train_counter Total ML training batches
# TYPE app_ml_train_counter counter
app_ml_train_counter {}

# HELP app_ml_fallback_total Discover requests that fell back to attractiveness score
# TYPE app_ml_fallback_total counter
app_ml_fallback_total {}

# HELP app_discover_requests_total Total discover endpoint requests
# TYPE app_discover_requests_total counter
app_discover_requests_total {}

# HELP app_vision_unavailable_total Vision service requests that were unavailable
# TYPE app_vision_unavailable_total counter
app_vision_unavailable_total {}

# HELP app_swipe_writes_total Total swipe write operations (like + pass)
# TYPE app_swipe_writes_total counter
app_swipe_writes_total {}

# HELP app_photos_rejected_quality Photos rejected by quality scoring
# TYPE app_photos_rejected_quality counter
app_photos_rejected_quality {}

# HELP app_moderation_actions_total Content moderation actions taken
# TYPE app_moderation_actions_total counter
app_moderation_actions_total {}

# HELP app_trust_safety_flags_total Trust safety flags raised
# TYPE app_trust_safety_flags_total counter
app_trust_safety_flags_total {}

# HELP app_upload_deferrals_total Upload deferrals due to client conditions
# TYPE app_upload_deferrals_total counter
app_upload_deferrals_total {}

# HELP dlq_entries_pending Pending entries in dead letter queue
# TYPE dlq_entries_pending gauge
dlq_entries_pending{{queue="payments"}} {}
dlq_entries_pending{{queue="webhooks"}} {}

# HELP dlq_entries_resolved_total Total resolved DLQ entries
# TYPE dlq_entries_resolved_total counter
dlq_entries_resolved_total{{queue="payments"}} {}
dlq_entries_resolved_total{{queue="webhooks"}} {}

# HELP dlq_entries_abandoned_total Total abandoned DLQ entries
# TYPE dlq_entries_abandoned_total counter
dlq_entries_abandoned_total{{queue="payments"}} {}
dlq_entries_abandoned_total{{queue="webhooks"}} {}
"#,
        metrics.requests_total.load(Ordering::Relaxed),
        metrics.requests_active.load(Ordering::Relaxed),
        metrics.errors_total.load(Ordering::Relaxed),
        metrics.db_queries_total.load(Ordering::Relaxed),
        metrics.cache_hits.load(Ordering::Relaxed),
        metrics.cache_misses.load(Ordering::Relaxed),
        metrics.websocket_connections.load(Ordering::Relaxed),
        uptime,
        db_pool_size,
        db_pool_idle,
        chat_rooms,
        chat_subscribers,
        db_read_pool_size,
        db_read_pool_idle,
        metrics.replica_lag_ms.load(Ordering::Relaxed),
        if metrics.replica_healthy.load(Ordering::Relaxed) { 1 } else { 0 },
        metrics.reads_from_replica.load(Ordering::Relaxed),
        metrics.reads_fallback_to_primary.load(Ordering::Relaxed),
        ml_avg_scoring_us,
        ml_epsilon,
        ml_train_counter,
        metrics.ml_fallback_total.load(Ordering::Relaxed),
        metrics.discover_requests_total.load(Ordering::Relaxed),
        metrics.vision_unavailable_total.load(Ordering::Relaxed),
        metrics.swipe_writes_total.load(Ordering::Relaxed),
        metrics.photos_rejected_quality.load(Ordering::Relaxed),
        metrics.moderation_actions_total.load(Ordering::Relaxed),
        metrics.trust_safety_flags_total.load(Ordering::Relaxed),
        metrics.upload_deferrals_total.load(Ordering::Relaxed),
        dlq_stats.payments_pending,
        dlq_stats.webhooks_pending,
        dlq_stats.payments_resolved,
        dlq_stats.webhooks_resolved,
        dlq_stats.payments_abandoned,
        dlq_stats.webhooks_abandoned,
    )
}

// DLQ metrics structure
struct DlqMetrics {
    payments_pending: i64,
    webhooks_pending: i64,
    payments_resolved: i64,
    webhooks_resolved: i64,
    payments_abandoned: i64,
    webhooks_abandoned: i64,
}

impl Default for DlqMetrics {
    fn default() -> Self {
        Self {
            payments_pending: 0,
            webhooks_pending: 0,
            payments_resolved: 0,
            webhooks_resolved: 0,
            payments_abandoned: 0,
            webhooks_abandoned: 0,
        }
    }
}

// Fetch DLQ metrics from database
async fn fetch_dlq_metrics(db: &sqlx::PgPool) -> DlqMetrics {
    let result = sqlx::query_as::<_, (String, String, i64)>(
        r#"
        SELECT queue_name, status, COUNT(*) as count
        FROM dead_letter_queue
        GROUP BY queue_name, status
        "#
    )
    .fetch_all(db)
    .await;

    match result {
        Ok(rows) => {
            let mut metrics = DlqMetrics::default();
            for (queue_name, status, count) in rows {
                match (queue_name.as_str(), status.as_str()) {
                    ("payments", "pending") => metrics.payments_pending = count,
                    ("payments", "resolved") => metrics.payments_resolved = count,
                    ("payments", "abandoned") => metrics.payments_abandoned = count,
                    ("webhooks", "pending") => metrics.webhooks_pending = count,
                    ("webhooks", "resolved") => metrics.webhooks_resolved = count,
                    ("webhooks", "abandoned") => metrics.webhooks_abandoned = count,
                    _ => {}
                }
            }
            metrics
        }
        Err(_) => DlqMetrics::default(),
    }
}
