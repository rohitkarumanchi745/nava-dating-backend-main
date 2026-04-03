use axum::extract::ws::{Message, WebSocket};
use chrono::Utc;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::{
    auth::decode_access_token,
    handlers::auto_queue_for_labeling,
    state::{AppState, ChatMessage, CallSignal},
};

// ============================================================================
// Chat WebSocket Handler
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct ChatPayload {
    #[serde(rename = "type")]
    message_type: String,
    content: Option<String>,
    message_id: Option<i32>,
}

pub async fn handle_chat(socket: WebSocket, state: AppState, match_id: String, token: String) {
    // Validate token
    let user_id = match decode_access_token(&token, &state.config.secret_key) {
        Ok(id) => id,
        Err(_) => {
            tracing::warn!("Invalid token for chat WebSocket");
            return;
        }
    };

    // Verify user is part of the match
    let match_check = sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM matches WHERE id = $1 AND (user1_id = $2 OR user2_id = $2) AND is_mutual_match = TRUE)",
    )
    .bind(&match_id)
    .bind(user_id)
    .fetch_one(&state.db)
    .await;

    if !match_check.unwrap_or(false) {
        tracing::warn!("User {} not authorized for match {}", user_id, match_id);
        return;
    }

    // Get broadcast channel for this room
    let tx = {
        let mut rooms = state.chat_rooms.write().await;
        rooms.get_or_create(&match_id)
    };
    let mut rx = tx.subscribe();

    let (sender, mut receiver) = socket.split();
    let sender = Arc::new(Mutex::new(sender));

    // Send welcome message
    let welcome = json!({
        "type": "connected",
        "match_id": match_id,
        "user_id": user_id,
        "timestamp": Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
    });
    let _ = sender.lock().await.send(Message::Text(welcome.to_string().into())).await;

    // Spawn keepalive ping every 30 seconds
    let ping_sender = Arc::clone(&sender);
    let ping_task = tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(30));
        loop {
            interval.tick().await;
            if ping_sender.lock().await.send(Message::Ping(vec![].into())).await.is_err() {
                break; // Connection closed
            }
        }
    });

    // Spawn task to forward broadcasts to this client
    let match_id_clone = match_id.clone();
    let fwd_sender = Arc::clone(&sender);
    let forward_task = tokio::spawn(async move {
        while let Ok(msg) = rx.recv().await {
            // Don't echo back to sender
            if msg.sender_id == user_id {
                continue;
            }
            let payload = json!({
                "type": msg.message_type,
                "sender_id": msg.sender_id,
                "content": msg.content,
                "message_id": msg.message_id,
                "timestamp": msg.timestamp,
            });
            if fwd_sender.lock().await.send(Message::Text(payload.to_string().into())).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages
    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Ping(_) | Message::Pong(_) = msg {
            // Ping/Pong handled by framework; ignore in application layer
            continue;
        }
        if let Message::Close(_) = msg {
            break;
        }
        if let Message::Text(text) = msg {
            if let Ok(payload) = serde_json::from_str::<ChatPayload>(&text) {
                match payload.message_type.as_str() {
                    "message" => {
                        if let Some(content) = payload.content {
                            // Enforce 5KB max message size
                            if content.len() > 5000 {
                                tracing::warn!(user_id, len = content.len(), "Message exceeds 5KB limit, skipping");
                                continue;
                            }
                            // Get receiver ID — O(1) from cache, DB fallback
                            let receiver_id = {
                                let rooms = state.chat_rooms.read().await;
                                rooms.get_other_user(&match_id, user_id)
                            };
                            let receiver_id = if let Some(rid) = receiver_id {
                                rid
                            } else {
                                let rid = get_other_user(&state, &match_id, user_id).await;
                                let mut rooms = state.chat_rooms.write().await;
                                rooms.cache_participants(&match_id, user_id, rid);
                                rid
                            };

                            // Store message in database
                            let message_id = sqlx::query_scalar::<_, i32>(
                                r#"
                                INSERT INTO messages (match_id, sender_id, receiver_id, content, message_type, created_at)
                                VALUES ($1, $2, $3, $4, 'text', NOW())
                                RETURNING id
                                "#,
                            )
                            .bind(&match_id)
                            .bind(user_id)
                            .bind(receiver_id)
                            .bind(&content)
                            .fetch_one(&state.db)
                            .await
                            .ok();

                            // Update match statistics (fire-and-forget)
                            {
                                let db = state.db.clone();
                                let mid = match_id.clone();
                                tokio::spawn(async move {
                                    let _ = sqlx::query(
                                        "UPDATE matches SET messages_count = COALESCE(messages_count, 0) + 1, last_message_at = NOW() WHERE id = $1",
                                    )
                                    .bind(&mid)
                                    .execute(&db)
                                    .await;
                                });
                            }

                            // Auto-queue for LLM labeling (toxicity check)
                            if let Some(mid) = message_id {
                                auto_queue_for_labeling(state.db.clone(), state.config.llm_enabled, "message", mid as i64, 3);
                            }

                            // Broadcast to room
                            let chat_msg = ChatMessage {
                                message_type: "message".to_string(),
                                sender_id: user_id,
                                content,
                                message_id,
                                timestamp: Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                            };
                            let _ = tx.send(chat_msg);
                        }
                    }
                    "typing" => {
                        let chat_msg = ChatMessage {
                            message_type: "typing".to_string(),
                            sender_id: user_id,
                            content: String::new(),
                            message_id: None,
                            timestamp: Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                        };
                        let _ = tx.send(chat_msg);
                    }
                    "read" => {
                        if let Some(message_id) = payload.message_id {
                            // Mark message as read (fire-and-forget, don't block WebSocket loop)
                            let db = state.db.clone();
                            let mid = message_id;
                            tokio::spawn(async move {
                                let _ = sqlx::query(
                                    "UPDATE messages SET is_read = TRUE, read_at = NOW() WHERE id = $1 AND receiver_id = $2",
                                )
                                .bind(mid)
                                .bind(user_id)
                                .execute(&db)
                                .await;
                            });

                            let chat_msg = ChatMessage {
                                message_type: "read".to_string(),
                                sender_id: user_id,
                                content: String::new(),
                                message_id: Some(message_id),
                                timestamp: Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string(),
                            };
                            let _ = tx.send(chat_msg);
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    // Cleanup
    ping_task.abort();
    forward_task.abort();
    {
        let mut rooms = state.chat_rooms.write().await;
        rooms.cleanup(&match_id_clone);
    }
}

async fn get_other_user(state: &AppState, match_id: &str, user_id: i32) -> i32 {
    #[derive(sqlx::FromRow)]
    struct MatchUsers {
        user1_id: i32,
        user2_id: i32,
    }

    let result = sqlx::query_as::<_, MatchUsers>(
        "SELECT user1_id, user2_id FROM matches WHERE id = $1",
    )
    .bind(match_id)
    .fetch_optional(&state.db)
    .await;

    match result {
        Ok(Some(m)) => {
            if m.user1_id == user_id {
                m.user2_id
            } else {
                m.user1_id
            }
        }
        _ => 0,
    }
}

// ============================================================================
// Call WebSocket Handler
// ============================================================================

#[derive(Debug, Serialize, Deserialize)]
struct CallPayload {
    #[serde(rename = "type")]
    signal_type: String,
    payload: Option<String>,
}

pub async fn handle_call(socket: WebSocket, state: AppState, call_id: String, token: String) {
    // Validate call token
    let user_id = match crate::auth::decode_call_token(&token, &state.config.secret_key) {
        Ok((id, _match_id, token_call_id)) => {
            if token_call_id != call_id {
                tracing::warn!("Call ID mismatch in token");
                return;
            }
            id
        }
        Err(_) => {
            // Try regular access token for callee who doesn't have call token yet
            match decode_access_token(&token, &state.config.secret_key) {
                Ok(id) => id,
                Err(_) => {
                    tracing::warn!("Invalid token for call WebSocket");
                    return;
                }
            }
        }
    };

    // Get call session and verify user is participant
    let session = {
        let sessions = state.call_sessions.read().await;
        sessions.get(&call_id).cloned()
    };

    let session = match session {
        Some(s) => {
            if s.caller_id != user_id && s.callee_id != user_id {
                tracing::warn!("User {} not authorized for call {}", user_id, call_id);
                return;
            }
            s
        }
        None => {
            tracing::warn!("Call session {} not found", call_id);
            return;
        }
    };

    // Get signal channel
    let tx = match {
        let sessions = state.call_sessions.read().await;
        sessions.get_signal_sender(&call_id)
    } {
        Some(tx) => tx,
        None => {
            tracing::warn!("Signal channel not found for call {}", call_id);
            return;
        }
    };

    let mut rx = tx.subscribe();
    let (mut sender, mut receiver) = socket.split();

    // Send join notification
    let join_signal = CallSignal {
        signal_type: "join".to_string(),
        sender_id: user_id,
        payload: json!({
            "call_id": call_id,
            "call_type": session.call_type,
            "caller_id": session.caller_id,
            "callee_id": session.callee_id,
        }).to_string(),
    };
    let _ = tx.send(join_signal);

    // Forward signals to this client
    let call_id_clone = call_id.clone();
    let forward_task = tokio::spawn(async move {
        while let Ok(signal) = rx.recv().await {
            // Don't echo back to sender
            if signal.sender_id == user_id {
                continue;
            }
            let payload = json!({
                "type": signal.signal_type,
                "sender_id": signal.sender_id,
                "payload": signal.payload,
            });
            if sender.send(Message::Text(payload.to_string().into())).await.is_err() {
                break;
            }
        }
    });

    // Handle incoming signals
    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(text) = msg {
            if let Ok(payload) = serde_json::from_str::<CallPayload>(&text) {
                let signal = CallSignal {
                    signal_type: payload.signal_type,
                    sender_id: user_id,
                    payload: payload.payload.unwrap_or_default(),
                };
                let _ = tx.send(signal);
            }
        } else if let Message::Close(_) = msg {
            break;
        }
    }

    // Send leave notification
    let leave_signal = CallSignal {
        signal_type: "leave".to_string(),
        sender_id: user_id,
        payload: String::new(),
    };
    let _ = tx.send(leave_signal);

    // Cleanup
    forward_task.abort();

    // End call if both parties left
    let should_end = {
        let sessions = state.call_sessions.read().await;
        if let Some(s) = sessions.get(&call_id_clone) {
            s.status == "ended"
        } else {
            true
        }
    };

    if should_end {
        let mut sessions = state.call_sessions.write().await;
        sessions.end(&call_id_clone);
    }
}
