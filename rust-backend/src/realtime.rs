//! Cross-instance real-time fanout over Redis Pub/Sub.
//!
//! WebSocket rooms (chat, calls, app events) live in per-process memory, so two
//! users connected to different pods can't see each other's traffic. This module
//! bridges that gap: every real-time send is also published to a Redis channel,
//! and each pod runs a single subscriber that re-injects messages that
//! originated on *other* pods into its own local broadcast channels.
//!
//! Delivery model (why this is correct and not double-delivered):
//! - The sending pod delivers to its own connected clients directly, in-process,
//!   exactly as before, AND publishes an envelope tagged with its instance id.
//! - Every pod's subscriber receives that envelope. It **skips its own** instance
//!   id (those clients already got it locally) and injects everyone else's.
//!
//! Degrades gracefully: with no Redis, publishing is a no-op and delivery stays
//! single-pod — identical to the previous behaviour.

use futures::StreamExt;
use redis::AsyncCommands;
use serde::{Deserialize, Serialize};
use tracing::{debug, info, warn};

use crate::models::CallSession;
use crate::state::{AppState, CallSignal, ChatMessage};

/// Redis Pub/Sub channel carrying all cross-instance real-time traffic.
const REALTIME_CHANNEL: &str = "nava:realtime";

/// Redis key prefix for shared call sessions, so a callee whose WebSocket lands
/// on a different pod than the caller can still find and join the call.
const CALL_SESSION_PREFIX: &str = "callsession";
/// TTL for a shared call session — comfortably longer than ring + connect time.
const CALL_SESSION_TTL_SECS: u64 = 3600;

/// The three real-time payload shapes that cross pods.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum Payload {
    Chat {
        match_id: String,
        message_type: String,
        sender_id: i32,
        content: String,
        message_id: Option<i32>,
        timestamp: String,
    },
    Call {
        call_id: String,
        signal_type: String,
        sender_id: i32,
        payload: String,
    },
    UserEvent {
        user_id: i32,
        json: String,
    },
}

/// Wire envelope: the payload plus the originating instance id (used to skip
/// our own echoes when they come back over the subscription).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Envelope {
    instance: String,
    payload: Payload,
}

// ---------------------------------------------------------------------------
// Publish side
// ---------------------------------------------------------------------------

async fn publish(state: &AppState, payload: Payload) {
    let Some(mut conn) = state.redis.clone() else {
        return; // no Redis → single-pod delivery, nothing to fan out
    };
    let envelope = Envelope {
        instance: state.config.instance_id.clone(),
        payload,
    };
    let json = match serde_json::to_string(&envelope) {
        Ok(j) => j,
        Err(e) => {
            warn!("realtime: envelope serialize failed: {e}");
            return;
        }
    };
    let result: Result<(), redis::RedisError> = conn.publish(REALTIME_CHANNEL, json).await;
    if let Err(e) = result {
        debug!("realtime: publish failed: {e}");
    }
}

/// Fan a chat message out to other pods.
pub async fn publish_chat(state: &AppState, match_id: &str, msg: &ChatMessage) {
    publish(
        state,
        Payload::Chat {
            match_id: match_id.to_string(),
            message_type: msg.message_type.clone(),
            sender_id: msg.sender_id,
            content: msg.content.clone(),
            message_id: msg.message_id,
            timestamp: msg.timestamp.clone(),
        },
    )
    .await;
}

/// Fan a call signal out to other pods.
pub async fn publish_call(state: &AppState, call_id: &str, signal: &CallSignal) {
    publish(
        state,
        Payload::Call {
            call_id: call_id.to_string(),
            signal_type: signal.signal_type.clone(),
            sender_id: signal.sender_id,
            payload: signal.payload.clone(),
        },
    )
    .await;
}

/// Fan an app-wide user event out to other pods.
pub async fn publish_user_event(state: &AppState, user_id: i32, json: &str) {
    publish(
        state,
        Payload::UserEvent {
            user_id,
            json: json.to_string(),
        },
    )
    .await;
}

// ---------------------------------------------------------------------------
// Shared call sessions (Redis-backed, so calls survive cross-pod WS routing)
// ---------------------------------------------------------------------------

/// Persist a call session so a participant on any pod can find it.
pub async fn store_call_session(state: &AppState, session: &CallSession) {
    let Some(mut conn) = state.redis.clone() else {
        return;
    };
    let key = format!("{}:{}", CALL_SESSION_PREFIX, session.call_id);
    match serde_json::to_string(session) {
        Ok(json) => {
            let result: Result<(), redis::RedisError> =
                conn.set_ex(key, json, CALL_SESSION_TTL_SECS).await;
            if let Err(e) = result {
                debug!("realtime: store_call_session failed: {e}");
            }
        }
        Err(e) => warn!("realtime: call session serialize failed: {e}"),
    }
}

/// Look up a call session created on another pod.
pub async fn load_call_session(state: &AppState, call_id: &str) -> Option<CallSession> {
    let mut conn = state.redis.clone()?;
    let key = format!("{}:{}", CALL_SESSION_PREFIX, call_id);
    let json: Option<String> = conn.get(key).await.ok()?;
    serde_json::from_str(&json?).ok()
}

// ---------------------------------------------------------------------------
// Subscribe side
// ---------------------------------------------------------------------------

/// Spawn the single per-pod subscriber that injects other pods' traffic into our
/// local broadcast channels. No-op (with a log) when Redis isn't configured.
pub fn spawn_subscriber(state: AppState) {
    if state.redis.is_none() {
        info!("realtime: Redis not configured; cross-instance fanout disabled");
        return;
    }
    let url = state.config.redis_url.clone();
    let our_instance = state.config.instance_id.clone();
    tokio::spawn(async move {
        loop {
            match run_subscriber(&state, &url, &our_instance).await {
                Ok(()) => warn!("realtime: subscriber stream ended; reconnecting in 2s"),
                Err(e) => warn!("realtime: subscriber error: {e}; reconnecting in 2s"),
            }
            tokio::time::sleep(std::time::Duration::from_secs(2)).await;
        }
    });
    info!("realtime: cross-instance Pub/Sub subscriber started");
}

async fn run_subscriber(
    state: &AppState,
    url: &str,
    our_instance: &str,
) -> Result<(), redis::RedisError> {
    let client = redis::Client::open(url)?;
    let mut pubsub = client.get_async_connection().await?.into_pubsub();
    pubsub.subscribe(REALTIME_CHANNEL).await?;
    let mut stream = pubsub.on_message();

    while let Some(msg) = stream.next().await {
        let raw: String = match msg.get_payload() {
            Ok(s) => s,
            Err(_) => continue,
        };
        let envelope: Envelope = match serde_json::from_str(&raw) {
            Ok(e) => e,
            Err(e) => {
                debug!("realtime: dropping malformed envelope: {e}");
                continue;
            }
        };
        // Skip our own messages — those clients were already served in-process.
        if envelope.instance == our_instance {
            continue;
        }
        deliver(state, envelope.payload).await;
    }
    Ok(())
}

/// Inject a remote payload into this pod's local broadcast channels. Each arm
/// only delivers if a relevant local room/subscriber exists, so we never
/// allocate rooms for users who aren't connected here.
async fn deliver(state: &AppState, payload: Payload) {
    match payload {
        Payload::Chat {
            match_id,
            message_type,
            sender_id,
            content,
            message_id,
            timestamp,
        } => {
            let sender = { state.chat_rooms.read().await.get_existing(&match_id) };
            if let Some(tx) = sender {
                let _ = tx.send(ChatMessage {
                    message_type,
                    sender_id,
                    content,
                    message_id,
                    timestamp,
                });
            }
        }
        Payload::Call {
            call_id,
            signal_type,
            sender_id,
            payload,
        } => {
            let sender = { state.call_sessions.read().await.get_signal_sender(&call_id) };
            if let Some(tx) = sender {
                let _ = tx.send(CallSignal {
                    signal_type,
                    sender_id,
                    payload,
                });
            }
        }
        Payload::UserEvent { user_id, json } => {
            state.user_events.read().await.publish(user_id, json);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chat_envelope_roundtrips() {
        let env = Envelope {
            instance: "nava-abc".to_string(),
            payload: Payload::Chat {
                match_id: "m1".into(),
                message_type: "message".into(),
                sender_id: 7,
                content: "hi".into(),
                message_id: Some(42),
                timestamp: "2026-01-01T00:00:00".into(),
            },
        };
        let json = serde_json::to_string(&env).unwrap();
        let back: Envelope = serde_json::from_str(&json).unwrap();
        assert_eq!(back.instance, "nava-abc");
        match back.payload {
            Payload::Chat { match_id, sender_id, message_id, .. } => {
                assert_eq!(match_id, "m1");
                assert_eq!(sender_id, 7);
                assert_eq!(message_id, Some(42));
            }
            other => panic!("wrong variant: {other:?}"),
        }
    }

    #[test]
    fn variants_are_tagged_by_kind() {
        // The `kind` tag is the cross-pod contract; guard against accidental drift.
        let call = Envelope {
            instance: "i".into(),
            payload: Payload::Call {
                call_id: "c".into(),
                signal_type: "offer".into(),
                sender_id: 1,
                payload: "sdp".into(),
            },
        };
        assert!(serde_json::to_string(&call).unwrap().contains("\"kind\":\"call\""));

        let user_event = Envelope {
            instance: "i".into(),
            payload: Payload::UserEvent { user_id: 3, json: "{}".into() },
        };
        assert!(serde_json::to_string(&user_event).unwrap().contains("\"kind\":\"user_event\""));
    }
}
