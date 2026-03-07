use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use axum::extract::FromRef;
use neo4rs::Graph;
use redis::aio::ConnectionManager;
use sqlx::PgPool;
use tokio::sync::{broadcast, Mutex, RwLock};

use crate::config::Config;
use crate::models::CallSession;
use crate::redis_service::RedisService;
use crate::vision::VisionAnalyzer;
use crate::services::graph_service::GraphService;
use crate::services::payments::PaymentService;
use crate::services::ads::AdsService;
use crate::middleware::dual_write::DualWriteManager;
use crate::ml::MlService;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    pub redis: Option<ConnectionManager>,
    pub neo4j: Option<Arc<Graph>>,
    pub graph_service: Option<Arc<GraphService>>,
    pub dual_write: Arc<DualWriteManager>,
    pub config: Config,
    pub vision: Option<Arc<Mutex<VisionAnalyzer>>>,
    pub chat_rooms: Arc<RwLock<ChatRooms>>,
    pub call_sessions: Arc<RwLock<CallSessions>>,
    pub metrics: Arc<AppMetrics>,
    pub start_time: Instant,
    // Payment and Ads services
    pub payment_service: Option<Arc<PaymentService>>,
    pub ads_service: Option<Arc<AdsService>>,
    // ML service (RL, LinUCB, Federated Learning)
    pub ml: Arc<RwLock<MlService>>,
}

// Allow extracting AppState from Arc<AppState>
impl FromRef<Arc<AppState>> for AppState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        state.as_ref().clone()
    }
}

impl AppState {
    /// Get Redis service wrapper (if Redis is connected)
    pub fn redis_service(&self) -> Option<RedisService> {
        self.redis.as_ref().map(|conn| RedisService::new(conn.clone()))
    }

    /// Get GraphService for dual-database operations
    pub fn graph(&self) -> Option<&GraphService> {
        self.graph_service.as_ref().map(|g| g.as_ref())
    }

    /// Check if Neo4j is available
    pub async fn is_neo4j_available(&self) -> bool {
        self.dual_write.can_write_neo4j().await
    }

    /// Check if PostgreSQL is available
    pub async fn is_postgres_available(&self) -> bool {
        self.dual_write.can_write_postgres().await
    }

    /// Get database health status
    pub async fn database_health(&self) -> crate::middleware::dual_write::DualWriteStatus {
        self.dual_write.status_report().await
    }
}

/// Application metrics for monitoring
pub struct AppMetrics {
    pub requests_total: AtomicU64,
    pub requests_active: AtomicU64,
    pub errors_total: AtomicU64,
    pub db_queries_total: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub websocket_connections: AtomicU64,
}

impl AppMetrics {
    pub fn new() -> Self {
        Self {
            requests_total: AtomicU64::new(0),
            requests_active: AtomicU64::new(0),
            errors_total: AtomicU64::new(0),
            db_queries_total: AtomicU64::new(0),
            cache_hits: AtomicU64::new(0),
            cache_misses: AtomicU64::new(0),
            websocket_connections: AtomicU64::new(0),
        }
    }

    pub fn inc_requests(&self) {
        self.requests_total.fetch_add(1, Ordering::Relaxed);
        self.requests_active.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dec_active_requests(&self) {
        self.requests_active.fetch_sub(1, Ordering::Relaxed);
    }

    pub fn inc_errors(&self) {
        self.errors_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_db_queries(&self) {
        self.db_queries_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_cache_hit(&self) {
        self.cache_hits.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_cache_miss(&self) {
        self.cache_misses.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_ws_connections(&self) {
        self.websocket_connections.fetch_add(1, Ordering::Relaxed);
    }

    pub fn dec_ws_connections(&self) {
        self.websocket_connections.fetch_sub(1, Ordering::Relaxed);
    }
}

impl Default for AppMetrics {
    fn default() -> Self {
        Self::new()
    }
}

/// Chat room management for WebSocket connections (scaled for 10k+ users)
pub struct ChatRooms {
    /// Map of match_id -> broadcast sender for that room
    rooms: HashMap<String, broadcast::Sender<ChatMessage>>,
    /// Buffer size for broadcast channels (configurable via config)
    buffer_size: usize,
}

impl ChatRooms {
    pub fn new() -> Self {
        Self {
            rooms: HashMap::new(),
            buffer_size: 100, // Default
        }
    }

    /// Create with configurable buffer size for high-traffic scenarios
    pub fn with_buffer_size(buffer_size: usize) -> Self {
        Self {
            rooms: HashMap::new(),
            buffer_size,
        }
    }

    /// Get or create a chat room for a match
    pub fn get_or_create(&mut self, match_id: &str) -> broadcast::Sender<ChatMessage> {
        if let Some(sender) = self.rooms.get(match_id) {
            sender.clone()
        } else {
            let (sender, _) = broadcast::channel(self.buffer_size);
            self.rooms.insert(match_id.to_string(), sender.clone());
            sender
        }
    }

    /// Remove a room if no subscribers remain
    pub fn cleanup(&mut self, match_id: &str) {
        if let Some(sender) = self.rooms.get(match_id) {
            if sender.receiver_count() == 0 {
                self.rooms.remove(match_id);
            }
        }
    }

    /// Get count of active rooms (for monitoring)
    pub fn room_count(&self) -> usize {
        self.rooms.len()
    }

    /// Get total subscribers across all rooms (for monitoring)
    pub fn total_subscribers(&self) -> usize {
        self.rooms.values().map(|s| s.receiver_count()).sum()
    }
}

impl Default for ChatRooms {
    fn default() -> Self {
        Self::new()
    }
}

/// Message sent through chat WebSocket
#[derive(Debug, Clone)]
pub struct ChatMessage {
    pub message_type: String, // "message", "typing", "read", "system"
    pub sender_id: i32,
    pub content: String,
    pub message_id: Option<i32>,
    pub timestamp: String,
}

/// Call session management (scaled for concurrent video calls)
pub struct CallSessions {
    /// Map of call_id -> call session
    sessions: HashMap<String, CallSession>,
    /// Map of call_id -> broadcast sender for signaling
    signals: HashMap<String, broadcast::Sender<CallSignal>>,
    /// Buffer size for call signal channels
    buffer_size: usize,
}

impl CallSessions {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            signals: HashMap::new(),
            buffer_size: 50, // Default
        }
    }

    /// Create with configurable buffer size
    pub fn with_buffer_size(buffer_size: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            signals: HashMap::new(),
            buffer_size,
        }
    }

    /// Create a new call session
    pub fn create(&mut self, session: CallSession) -> broadcast::Sender<CallSignal> {
        let call_id = session.call_id.clone();
        self.sessions.insert(call_id.clone(), session);
        let (sender, _) = broadcast::channel(self.buffer_size);
        self.signals.insert(call_id, sender.clone());
        sender
    }

    /// Get an existing call session
    pub fn get(&self, call_id: &str) -> Option<&CallSession> {
        self.sessions.get(call_id)
    }

    /// Get signal sender for a call
    pub fn get_signal_sender(&self, call_id: &str) -> Option<broadcast::Sender<CallSignal>> {
        self.signals.get(call_id).cloned()
    }

    /// End a call session
    pub fn end(&mut self, call_id: &str) {
        self.sessions.remove(call_id);
        self.signals.remove(call_id);
    }

    /// Check if user is in an active call
    pub fn user_in_call(&self, user_id: i32) -> Option<String> {
        for (call_id, session) in &self.sessions {
            if session.status != "ended"
                && (session.caller_id == user_id || session.callee_id == user_id)
            {
                return Some(call_id.clone());
            }
        }
        None
    }
}

impl Default for CallSessions {
    fn default() -> Self {
        Self::new()
    }
}

/// Signal sent through call WebSocket
#[derive(Debug, Clone)]
pub struct CallSignal {
    pub signal_type: String, // "offer", "answer", "ice", "join", "leave", "end"
    pub sender_id: i32,
    pub payload: String, // JSON payload (SDP, ICE candidate, etc.)
}
