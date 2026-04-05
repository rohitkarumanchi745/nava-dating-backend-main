use std::collections::{HashMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Instant;

use axum::extract::FromRef;
use lru::LruCache;
use redis::aio::ConnectionManager;
use sqlx::PgPool;
use tokio::sync::{broadcast, Mutex, RwLock};

// ============================================================================
// In-Memory LRU Caches (eliminate repeated DB queries on hot paths)
// ============================================================================

/// Cached user preferences (discover hot path) — 5 min TTL
#[derive(Clone, Debug)]
pub struct CachedPreferences {
    pub min_age: Option<i32>,
    pub max_age: Option<i32>,
    pub max_distance: Option<i32>,
    pub only_verified: Option<bool>,
    pub only_students: Option<bool>,
    pub cached_at: Instant,
}

/// Cached user location (discover hot path) — 5 min TTL
#[derive(Clone, Debug)]
pub struct CachedLocation {
    pub latitude: Option<f64>,
    pub longitude: Option<f64>,
    pub city: Option<String>,
    pub cached_at: Instant,
}

/// Cached university info (static data) — 6 hour TTL
#[derive(Clone, Debug)]
pub struct CachedUniversity {
    pub name: String,
    pub tier: String,
    pub cached_at: Instant,
}

/// Cached premium status — 10 min TTL
#[derive(Clone, Debug)]
pub struct CachedPremiumStatus {
    pub is_premium: bool,
    pub tier: Option<String>,
    pub cached_at: Instant,
}

/// Check if a cache entry is still fresh
pub fn cache_fresh(cached_at: Instant, ttl_secs: u64) -> bool {
    cached_at.elapsed().as_secs() < ttl_secs
}

pub const PREFS_CACHE_TTL: u64 = 300;     // 5 min
pub const LOCATION_CACHE_TTL: u64 = 300;  // 5 min
pub const UNI_CACHE_TTL: u64 = 21600;     // 6 hours
pub const PREMIUM_CACHE_TTL: u64 = 600;   // 10 min

use crate::config::Config;
use crate::models::CallSession;
use crate::redis_service::RedisService;
use crate::vision::VisionAnalyzer;
use crate::services::graph_service::GraphService;
use crate::services::graph::GraphAbstraction;
use crate::services::payments::PaymentService;
use crate::services::ads::AdsService;
use crate::services::trust_safety::TrustSafetyService;
use crate::services::moderation::ModerationPipeline;
use crate::middleware::dual_write::DualWriteManager;
use crate::ml::MlService;
use crate::modules::events::EventBus;

/// Shared application state
#[derive(Clone)]
pub struct AppState {
    pub db: PgPool,
    /// Read replica pool for read-heavy queries (discover, matches, history).
    /// Falls back to primary if not configured.
    pub db_read: Option<PgPool>,
    pub redis: Option<ConnectionManager>,
    pub graph_service: Arc<GraphService>,
    /// Netflix-inspired property graph abstraction (FoF, university, reel collab, fraud)
    pub graph: Arc<GraphAbstraction>,
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
    // Trust & Safety service
    pub trust_safety: Option<Arc<TrustSafetyService>>,
    // Content moderation pipeline
    pub moderation: Option<Arc<ModerationPipeline>>,
    // Event bus for domain events (notifications, analytics)
    pub event_bus: Arc<EventBus>,
    // In-memory LRU caches (hot path optimization)
    pub preferences_cache: Arc<RwLock<LruCache<i32, CachedPreferences>>>,
    pub location_cache: Arc<RwLock<LruCache<i32, CachedLocation>>>,
    pub university_cache: Arc<RwLock<LruCache<i32, CachedUniversity>>>,
    pub premium_cache: Arc<RwLock<LruCache<i32, CachedPremiumStatus>>>,
    /// Blocked user pairs for O(1) lookup — rebuilt every 5 min
    pub blocked_pairs: Arc<RwLock<HashSet<(i32, i32)>>>,
    /// Per-user event hub for app-wide WebSocket fanout (unread counts, etc.)
    pub user_events: Arc<RwLock<UserEventHub>>,
}

// Allow extracting AppState from Arc<AppState>
impl FromRef<Arc<AppState>> for AppState {
    fn from_ref(state: &Arc<AppState>) -> Self {
        state.as_ref().clone()
    }
}

impl AppState {
    /// Get the read-replica pool if available and healthy.
    /// Falls back to primary if replica is not configured or `replica_healthy` is false.
    /// The `replica_healthy` flag is updated by a background task that checks lag every 5s.
    /// This keeps the hot path zero-cost (no extra query per request).
    pub fn read_pool(&self) -> &PgPool {
        if let Some(ref replica) = self.db_read {
            if self.metrics.replica_healthy.load(Ordering::Relaxed) {
                self.metrics.reads_from_replica.fetch_add(1, Ordering::Relaxed);
                return replica;
            }
            self.metrics.reads_fallback_to_primary.fetch_add(1, Ordering::Relaxed);
        }
        &self.db
    }

    /// Get Redis service wrapper (if Redis is connected)
    pub fn redis_service(&self) -> Option<RedisService> {
        self.redis.as_ref().map(|conn| RedisService::new(conn.clone()))
    }

    /// Get GraphService for graph queries
    pub fn graph(&self) -> &GraphService {
        self.graph_service.as_ref()
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

/// Standard Prometheus HTTP request duration histogram bucket boundaries (in seconds).
pub const HISTOGRAM_BUCKETS: &[f64] = &[
    0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0,
];

/// Application metrics for monitoring
pub struct AppMetrics {
    pub requests_total: AtomicU64,
    pub requests_active: AtomicU64,
    pub errors_total: AtomicU64,
    pub db_queries_total: AtomicU64,
    pub cache_hits: AtomicU64,
    pub cache_misses: AtomicU64,
    pub websocket_connections: AtomicU64,
    /// Reads served from the read replica pool
    pub reads_from_replica: AtomicU64,
    /// Reads that fell back to primary (replica unavailable or lag too high)
    pub reads_fallback_to_primary: AtomicU64,
    /// Whether the read replica is healthy (lag < cutoff). Updated by background task.
    pub replica_healthy: std::sync::atomic::AtomicBool,
    /// Last measured replica lag in milliseconds (for /metrics export)
    pub replica_lag_ms: AtomicU64,
    /// Discover requests that fell back to attractiveness score (ML timeout/error)
    pub ml_fallback_total: AtomicU64,
    /// Total discover requests (denominator for ML fallback rate)
    pub discover_requests_total: AtomicU64,
    /// Vision service requests that were unavailable or failed
    pub vision_unavailable_total: AtomicU64,
    /// Total swipe write operations (like + pass) for TPS monitoring
    pub swipe_writes_total: AtomicU64,
    /// Photos rejected by quality scoring
    pub photos_rejected_quality: AtomicU64,
    /// Content moderation actions taken
    pub moderation_actions_total: AtomicU64,
    /// Trust safety flags raised
    pub trust_safety_flags_total: AtomicU64,
    /// Upload deferrals due to client conditions
    pub upload_deferrals_total: AtomicU64,
    // --- Knowledge graph observability ---
    pub graph_edge_writes_ok: AtomicU64,
    pub graph_edge_writes_failed: AtomicU64,
    pub event_log_writes_failed: AtomicU64,
    pub behavior_recompute_rows: AtomicU64,
    pub behavior_recompute_duration_ms: AtomicU64,
    pub session_heartbeat_drops: AtomicU64,
    pub location_track_ingested: AtomicU64,
    pub location_precision_reduced: AtomicU64,

    // --- HTTP request duration histogram ---
    /// Bucket counters for http_request_duration_seconds histogram.
    /// Index i counts observations <= HISTOGRAM_BUCKETS[i].
    /// The last element (index 11) is the +Inf bucket.
    pub duration_buckets: [AtomicU64; 12],
    /// Cumulative sum of observed durations in seconds (stored as f64 bits).
    pub duration_sum_bits: AtomicU64,
    /// Total number of observations.
    pub duration_count: AtomicU64,
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
            reads_from_replica: AtomicU64::new(0),
            reads_fallback_to_primary: AtomicU64::new(0),
            replica_healthy: std::sync::atomic::AtomicBool::new(false),
            replica_lag_ms: AtomicU64::new(0),
            ml_fallback_total: AtomicU64::new(0),
            discover_requests_total: AtomicU64::new(0),
            vision_unavailable_total: AtomicU64::new(0),
            swipe_writes_total: AtomicU64::new(0),
            photos_rejected_quality: AtomicU64::new(0),
            moderation_actions_total: AtomicU64::new(0),
            trust_safety_flags_total: AtomicU64::new(0),
            upload_deferrals_total: AtomicU64::new(0),
            graph_edge_writes_ok: AtomicU64::new(0),
            graph_edge_writes_failed: AtomicU64::new(0),
            event_log_writes_failed: AtomicU64::new(0),
            behavior_recompute_rows: AtomicU64::new(0),
            behavior_recompute_duration_ms: AtomicU64::new(0),
            session_heartbeat_drops: AtomicU64::new(0),
            location_track_ingested: AtomicU64::new(0),
            location_precision_reduced: AtomicU64::new(0),
            duration_buckets: [
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
                AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0), AtomicU64::new(0),
            ],
            duration_sum_bits: AtomicU64::new(0_f64.to_bits()),
            duration_count: AtomicU64::new(0),
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

    pub fn inc_ml_fallback(&self) {
        self.ml_fallback_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_discover_requests(&self) {
        self.discover_requests_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_vision_unavailable(&self) {
        self.vision_unavailable_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_swipe_writes(&self) {
        self.swipe_writes_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_photos_rejected(&self) {
        self.photos_rejected_quality.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_moderation_actions(&self) {
        self.moderation_actions_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_trust_safety_flags(&self) {
        self.trust_safety_flags_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn inc_upload_deferrals(&self) {
        self.upload_deferrals_total.fetch_add(1, Ordering::Relaxed);
    }

    /// Record an observed request duration into the histogram.
    pub fn observe_duration(&self, secs: f64) {
        // Increment every bucket whose boundary >= observed value, plus the +Inf bucket.
        for (i, &bound) in HISTOGRAM_BUCKETS.iter().enumerate() {
            if secs <= bound {
                self.duration_buckets[i].fetch_add(1, Ordering::Relaxed);
            }
        }
        // +Inf bucket (index 11) always incremented
        self.duration_buckets[HISTOGRAM_BUCKETS.len()].fetch_add(1, Ordering::Relaxed);

        // Atomically add to sum using CAS loop on the f64 bits
        loop {
            let old_bits = self.duration_sum_bits.load(Ordering::Relaxed);
            let old_val = f64::from_bits(old_bits);
            let new_val = old_val + secs;
            let new_bits = new_val.to_bits();
            if self
                .duration_sum_bits
                .compare_exchange_weak(old_bits, new_bits, Ordering::Relaxed, Ordering::Relaxed)
                .is_ok()
            {
                break;
            }
        }

        self.duration_count.fetch_add(1, Ordering::Relaxed);
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
    /// Cache match participants: match_id -> (user1_id, user2_id) — avoids DB query per message
    match_participants: HashMap<String, (i32, i32)>,
    /// Buffer size for broadcast channels (configurable via config)
    buffer_size: usize,
}

impl ChatRooms {
    pub fn new() -> Self {
        Self {
            rooms: HashMap::new(),
            match_participants: HashMap::new(),
            buffer_size: 100,
        }
    }

    /// Create with configurable buffer size for high-traffic scenarios
    pub fn with_buffer_size(buffer_size: usize) -> Self {
        Self {
            rooms: HashMap::new(),
            match_participants: HashMap::new(),
            buffer_size,
        }
    }

    /// Cache match participants for O(1) receiver lookup
    pub fn cache_participants(&mut self, match_id: &str, user1: i32, user2: i32) {
        self.match_participants.insert(match_id.to_string(), (user1, user2));
    }

    /// Get the other user in a match — O(1), no DB query
    pub fn get_other_user(&self, match_id: &str, user_id: i32) -> Option<i32> {
        self.match_participants.get(match_id).map(|(u1, u2)| {
            if *u1 == user_id { *u2 } else { *u1 }
        })
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

    /// Remove all rooms with zero subscribers (periodic cleanup)
    pub fn cleanup_stale(&mut self) {
        self.rooms.retain(|_match_id, sender| sender.receiver_count() > 0);
        // Clean participant cache for rooms that no longer exist
        let active_rooms: HashSet<&String> = self.rooms.keys().collect();
        self.match_participants.retain(|mid, _| active_rooms.contains(mid));
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
    /// Reverse index: user_id -> call_id for O(1) lookup
    user_to_call: HashMap<i32, String>,
    /// Buffer size for call signal channels
    buffer_size: usize,
}

impl CallSessions {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            signals: HashMap::new(),
            user_to_call: HashMap::new(),
            buffer_size: 50, // Default
        }
    }

    /// Create with configurable buffer size
    pub fn with_buffer_size(buffer_size: usize) -> Self {
        Self {
            sessions: HashMap::new(),
            signals: HashMap::new(),
            user_to_call: HashMap::new(),
            buffer_size,
        }
    }

    /// Create a new call session
    pub fn create(&mut self, session: CallSession) -> broadcast::Sender<CallSignal> {
        let call_id = session.call_id.clone();
        self.user_to_call.insert(session.caller_id, call_id.clone());
        self.user_to_call.insert(session.callee_id, call_id.clone());
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
        if let Some(session) = self.sessions.remove(call_id) {
            self.user_to_call.remove(&session.caller_id);
            self.user_to_call.remove(&session.callee_id);
        }
        self.signals.remove(call_id);
    }

    /// Check if user is in an active call — O(1) via reverse index
    pub fn user_in_call(&self, user_id: i32) -> Option<String> {
        self.user_to_call.get(&user_id).cloned()
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

// ============================================================================
// User Event Hub — per-user WebSocket fanout (unread counts, app-wide events)
// ============================================================================

/// App-wide user event. Frontend subscribes over /ws/events and receives these
/// as JSON with the `type` field discriminating the payload.
#[derive(Debug, Clone)]
pub struct UserEvent {
    pub json: String,
}

/// Hub mapping user_id -> broadcast channel. Each authenticated WebSocket
/// subscribes to its user's channel. Multiple devices per user all receive
/// the same event via broadcast::channel's multi-consumer semantics.
pub struct UserEventHub {
    channels: HashMap<i32, broadcast::Sender<UserEvent>>,
    buffer_size: usize,
}

impl UserEventHub {
    pub fn new() -> Self {
        Self { channels: HashMap::new(), buffer_size: 32 }
    }

    /// Get or create the broadcast sender for a user.
    pub fn get_or_create(&mut self, user_id: i32) -> broadcast::Sender<UserEvent> {
        if let Some(s) = self.channels.get(&user_id) {
            s.clone()
        } else {
            let (s, _) = broadcast::channel(self.buffer_size);
            self.channels.insert(user_id, s.clone());
            s
        }
    }

    /// Publish a pre-serialized JSON event to a user. If nobody is listening,
    /// the send is a no-op (broadcast channels drop when no subscribers).
    pub fn publish(&self, user_id: i32, json: String) {
        if let Some(s) = self.channels.get(&user_id) {
            let _ = s.send(UserEvent { json });
        }
    }
}

impl Default for UserEventHub {
    fn default() -> Self { Self::new() }
}
