//! Chat Service - Handles real-time messaging
//!
//! Endpoints:
//! - GET  /messages/:match_id    - Get messages for a match
//! - POST /messages/:match_id    - Send a message
//! - WS   /ws/:match_id          - WebSocket for real-time chat

use axum::{
    extract::{Path, State, WebSocketUpgrade, ws::{WebSocket, Message}},
    response::IntoResponse,
    routing::{get, post},
    Json, Router,
};
use futures::{StreamExt, SinkExt};
use serde::{Deserialize, Serialize};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;
use std::sync::Arc;
use tokio::sync::broadcast;
use tower_http::trace::TraceLayer;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use shared::config::{get_env, get_env_port};
use shared::{AppError, AppResult};

#[derive(Clone)]
struct AppState {
    db: PgPool,
    tx: broadcast::Sender<ChatEvent>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ChatEvent {
    match_id: i32,
    sender_id: i32,
    content: String,
    timestamp: String,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let subscriber = FmtSubscriber::builder().with_max_level(Level::INFO).finish();
    tracing::subscriber::set_global_default(subscriber)?;
    dotenvy::dotenv().ok();

    let db = PgPoolOptions::new()
        .max_connections(10)
        .connect(&get_env("DATABASE_URL"))
        .await?;

    let (tx, _) = broadcast::channel::<ChatEvent>(100);

    info!("Connected to database");

    let app = Router::new()
        .route("/health", get(health))
        .route("/messages/:match_id", get(get_messages))
        .route("/messages/:match_id", post(send_message))
        .route("/ws/:match_id", get(ws_handler))
        .layer(TraceLayer::new_for_http())
        .with_state(Arc::new(AppState { db, tx }));

    let port = get_env_port("CHAT_SERVICE_PORT", 8004);
    info!("💬 Chat Service starting on 0.0.0.0:{}", port);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn health() -> Json<serde_json::Value> {
    Json(serde_json::json!({"service": "chat-service", "status": "healthy"}))
}

#[derive(Debug, Serialize)]
struct ChatMessage {
    id: i32,
    sender_id: i32,
    content: String,
    created_at: String,
    read_at: Option<String>,
}

async fn get_messages(
    State(state): State<Arc<AppState>>,
    Path(match_id): Path<i32>,
) -> AppResult<Json<Vec<ChatMessage>>> {
    let messages = sqlx::query_as::<_, (i32, i32, String, chrono::DateTime<chrono::Utc>, Option<chrono::DateTime<chrono::Utc>>)>(
        r#"
        SELECT id, sender_id, content, created_at, read_at
        FROM messages
        WHERE match_id = $1
        ORDER BY created_at ASC
        LIMIT 100
        "#
    )
    .bind(match_id)
    .fetch_all(&state.db)
    .await?;

    let result: Vec<ChatMessage> = messages
        .into_iter()
        .map(|(id, sender_id, content, created_at, read_at)| ChatMessage {
            id,
            sender_id,
            content,
            created_at: created_at.to_rfc3339(),
            read_at: read_at.map(|t| t.to_rfc3339()),
        })
        .collect();

    Ok(Json(result))
}

#[derive(Debug, Deserialize)]
struct SendMessageRequest {
    content: String,
}

async fn send_message(
    State(state): State<Arc<AppState>>,
    Path(match_id): Path<i32>,
    Json(req): Json<SendMessageRequest>,
) -> AppResult<Json<ChatMessage>> {
    let user_id = 1; // Placeholder

    let result = sqlx::query_as::<_, (i32, chrono::DateTime<chrono::Utc>)>(
        r#"
        INSERT INTO messages (match_id, sender_id, content, created_at)
        VALUES ($1, $2, $3, NOW())
        RETURNING id, created_at
        "#
    )
    .bind(match_id)
    .bind(user_id)
    .bind(&req.content)
    .fetch_one(&state.db)
    .await?;

    // Broadcast to WebSocket subscribers
    let event = ChatEvent {
        match_id,
        sender_id: user_id,
        content: req.content.clone(),
        timestamp: result.1.to_rfc3339(),
    };
    let _ = state.tx.send(event);

    Ok(Json(ChatMessage {
        id: result.0,
        sender_id: user_id,
        content: req.content,
        created_at: result.1.to_rfc3339(),
        read_at: None,
    }))
}

async fn ws_handler(
    State(state): State<Arc<AppState>>,
    Path(match_id): Path<i32>,
    ws: WebSocketUpgrade,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state, match_id))
}

async fn handle_socket(socket: WebSocket, state: Arc<AppState>, match_id: i32) {
    let (mut sender, mut receiver) = socket.split();
    let mut rx = state.tx.subscribe();

    // Spawn task to forward broadcast messages to this client
    let send_task = tokio::spawn(async move {
        while let Ok(event) = rx.recv().await {
            if event.match_id == match_id {
                let msg = serde_json::to_string(&event).unwrap();
                if sender.send(Message::Text(msg)).await.is_err() {
                    break;
                }
            }
        }
    });

    // Handle incoming messages
    while let Some(msg) = receiver.next().await {
        if let Ok(Message::Text(text)) = msg {
            info!("Received: {}", text);
            // Could process incoming messages here
        }
    }

    send_task.abort();
}
