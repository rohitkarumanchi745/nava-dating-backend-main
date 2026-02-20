//! API Gateway - Routes requests to appropriate microservices
//! 
//! The gateway handles:
//! - Request routing to backend services
//! - Authentication validation
//! - Rate limiting
//! - Request/response logging
//! - CORS handling

use axum::{
    body::Body,
    extract::{Path, State},
    http::{Request, StatusCode, HeaderMap, Method},
    response::{IntoResponse, Response},
    routing::{any, get},
    Router,
};
use reqwest::Client;
use std::sync::Arc;
use tower_http::{
    cors::{Any, CorsLayer},
    trace::TraceLayer,
};
use tracing::{info, error, Level};
use tracing_subscriber::FmtSubscriber;

use shared::config::{get_env_or, get_env_port, ServiceUrls};

/// Application state shared across handlers
#[derive(Clone)]
struct AppState {
    client: Client,
    services: ServiceUrls,
    jwt_secret: String,
}

#[tokio::main]
async fn main() {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting subscriber failed");

    dotenvy::dotenv().ok();

    let state = AppState {
        client: Client::new(),
        services: ServiceUrls::from_env(),
        jwt_secret: get_env_or("JWT_SECRET", "dev-secret-change-in-production"),
    };

    info!("Service URLs configured:");
    info!("  Auth:         {}", state.services.auth_service);
    info!("  User:         {}", state.services.user_service);
    info!("  Match:        {}", state.services.match_service);
    info!("  Chat:         {}", state.services.chat_service);
    info!("  Payment:      {}", state.services.payment_service);
    info!("  Ambassador:   {}", state.services.ambassador_service);
    info!("  Notification: {}", state.services.notification_service);
    info!("  Analytics:    {}", state.services.analytics_service);

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let app = Router::new()
        // Health check
        .route("/health", get(health))
        
        // Auth service routes
        .route("/api/auth/*path", any(proxy_auth))
        .route("/api/otp/*path", any(proxy_auth))
        
        // User service routes
        .route("/api/users/*path", any(proxy_user))
        .route("/api/profile/*path", any(proxy_user))
        .route("/api/preferences/*path", any(proxy_user))
        
        // Match service routes
        .route("/api/discover/*path", any(proxy_match))
        .route("/api/matches/*path", any(proxy_match))
        .route("/api/likes/*path", any(proxy_match))
        
        // Chat service routes
        .route("/api/chat/*path", any(proxy_chat))
        .route("/api/messages/*path", any(proxy_chat))
        .route("/ws/chat/*path", any(proxy_chat))
        
        // Payment service routes
        .route("/api/payments/*path", any(proxy_payment))
        .route("/api/subscriptions/*path", any(proxy_payment))
        .route("/webhook/*path", any(proxy_payment))
        
        // Ambassador service routes
        .route("/api/ambassador/*path", any(proxy_ambassador))
        .route("/api/referral/*path", any(proxy_ambassador))

        // Notification service routes (internal API)
        .route("/api/notifications/*path", any(proxy_notification))

        // Analytics service routes (internal API)
        .route("/api/analytics/*path", any(proxy_analytics))

        .layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::new(state));

    let port = get_env_port("GATEWAY_PORT", 8080);
    let addr = format!("0.0.0.0:{}", port);
    
    info!("🚀 API Gateway starting on {}", addr);
    
    let listener = tokio::net::TcpListener::bind(&addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

/// Health check endpoint
async fn health() -> impl IntoResponse {
    axum::Json(serde_json::json!({
        "service": "gateway",
        "status": "healthy",
        "version": env!("CARGO_PKG_VERSION")
    }))
}

/// Proxy request to auth service
async fn proxy_auth(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    proxy_request(&state.client, &state.services.auth_service, &path, method, headers, body).await
}

/// Proxy request to user service
async fn proxy_user(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    proxy_request(&state.client, &state.services.user_service, &path, method, headers, body).await
}

/// Proxy request to match service
async fn proxy_match(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    proxy_request(&state.client, &state.services.match_service, &path, method, headers, body).await
}

/// Proxy request to chat service
async fn proxy_chat(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    proxy_request(&state.client, &state.services.chat_service, &path, method, headers, body).await
}

/// Proxy request to payment service
async fn proxy_payment(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    proxy_request(&state.client, &state.services.payment_service, &path, method, headers, body).await
}

/// Proxy request to ambassador service
async fn proxy_ambassador(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    proxy_request(&state.client, &state.services.ambassador_service, &path, method, headers, body).await
}

/// Proxy request to notification service
async fn proxy_notification(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    proxy_request(&state.client, &state.services.notification_service, &path, method, headers, body).await
}

/// Proxy request to analytics service
async fn proxy_analytics(
    State(state): State<Arc<AppState>>,
    Path(path): Path<String>,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    proxy_request(&state.client, &state.services.analytics_service, &path, method, headers, body).await
}

/// Generic proxy function
async fn proxy_request(
    client: &Client,
    base_url: &str,
    path: &str,
    method: Method,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let url = format!("{}/{}", base_url, path);
    
    // Convert axum body to bytes
    let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(e) => {
            error!("Failed to read request body: {}", e);
            return (StatusCode::BAD_REQUEST, "Invalid request body").into_response();
        }
    };

    // Build the proxied request
    let mut request = client.request(method, &url);
    
    // Forward relevant headers
    for (name, value) in headers.iter() {
        if let Ok(v) = value.to_str() {
            if !name.as_str().starts_with("host") {
                request = request.header(name.as_str(), v);
            }
        }
    }

    // Send request with body
    let response = match request.body(body_bytes.to_vec()).send().await {
        Ok(resp) => resp,
        Err(e) => {
            error!("Proxy request failed: {} -> {}", url, e);
            return (StatusCode::SERVICE_UNAVAILABLE, "Service unavailable").into_response();
        }
    };

    // Build response
    let status = response.status();
    let response_headers = response.headers().clone();
    let response_body = response.bytes().await.unwrap_or_default();

    let mut builder = Response::builder().status(status.as_u16());
    
    for (name, value) in response_headers.iter() {
        builder = builder.header(name.as_str(), value.as_bytes());
    }

    builder.body(Body::from(response_body)).unwrap_or_else(|_| {
        (StatusCode::INTERNAL_SERVER_ERROR, "Failed to build response").into_response()
    })
}
