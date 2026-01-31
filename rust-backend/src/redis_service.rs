//! Redis Service - Production-ready caching, rate limiting, and session management
//!
//! Features:
//! - Connection pooling with automatic reconnection
//! - Rate limiting (sliding window)
//! - Session/token caching
//! - OTP storage with TTL
//! - User profile caching
//! - Distributed locking
//! - Horizontal scaling support (instance-aware keys)
//! - Pub/Sub for cross-instance communication

use redis::aio::ConnectionManager;
use redis::{AsyncCommands, RedisError};
use serde::{de::DeserializeOwned, Serialize};
use tracing::{debug, warn};

/// Cache key prefixes for organization
pub mod keys {
    pub const RATE_LIMIT: &str = "rl";
    pub const SESSION: &str = "session";
    pub const OTP: &str = "otp";
    pub const USER_CACHE: &str = "user";
    pub const PROFILE_CACHE: &str = "profile";
    pub const TOKEN_BLACKLIST: &str = "blacklist";
    pub const LOCK: &str = "lock";
    pub const ONLINE_USERS: &str = "online";
    pub const MATCH_CACHE: &str = "match";
    pub const DISCOVERY_CACHE: &str = "discover";
    // Horizontal scaling keys
    pub const INSTANCE_HEARTBEAT: &str = "instance";
    pub const WS_USER_LOCATION: &str = "ws_loc";  // Maps user_id -> instance_id for WebSocket routing
    pub const PUBSUB_CHANNEL: &str = "nava_events";
}

/// TTL constants (in seconds)
pub mod ttl {
    pub const OTP: u64 = 300;              // 5 minutes
    pub const SESSION: u64 = 604800;       // 7 days
    pub const USER_CACHE: u64 = 3600;      // 1 hour
    pub const PROFILE_CACHE: u64 = 1800;   // 30 minutes
    pub const RATE_LIMIT_WINDOW: u64 = 60; // 1 minute
    pub const LOCK: u64 = 30;              // 30 seconds
    pub const ONLINE_STATUS: u64 = 300;    // 5 minutes
    pub const DISCOVERY_CACHE: u64 = 300;  // 5 minutes
    pub const MATCH_CACHE: u64 = 600;      // 10 minutes
}

/// Redis service wrapper with helper methods
#[derive(Clone)]
pub struct RedisService {
    conn: ConnectionManager,
}

impl RedisService {
    pub fn new(conn: ConnectionManager) -> Self {
        Self { conn }
    }

    /// Get the underlying connection manager
    pub fn connection(&self) -> ConnectionManager {
        self.conn.clone()
    }

    // ============================================================
    // RATE LIMITING
    // ============================================================

    /// Check and increment rate limit using sliding window
    /// Returns (allowed, remaining, reset_in_seconds)
    pub async fn check_rate_limit(
        &self,
        identifier: &str,
        max_requests: u32,
        window_secs: u64,
    ) -> Result<(bool, u32, u64), RedisError> {
        let key = format!("{}:{}", keys::RATE_LIMIT, identifier);
        let mut conn = self.conn.clone();
        let now = chrono::Utc::now().timestamp() as u64;
        let window_start = now - window_secs;

        // Use individual commands for reliability
        // Remove old entries outside the window
        let _: () = conn.zrembyscore(&key, 0, window_start as isize).await?;

        // Add current request with unique member
        let member = format!("{}:{}", now, rand::random::<u32>());
        let _: () = conn.zadd(&key, member, now as f64).await?;

        // Count requests in window
        let count: u32 = conn.zcard(&key).await?;

        // Set expiry on the key
        let _: () = conn.expire(&key, window_secs as i64).await?;

        let allowed = count <= max_requests;
        let remaining = if allowed { max_requests - count } else { 0 };
        let reset_in = window_secs;

        debug!(
            "Rate limit check for {}: count={}, allowed={}, remaining={}",
            identifier, count, allowed, remaining
        );

        Ok((allowed, remaining, reset_in))
    }

    /// Simple rate limit check (fixed window)
    pub async fn is_rate_limited(
        &self,
        identifier: &str,
        max_requests: u32,
    ) -> Result<bool, RedisError> {
        let (allowed, _, _) = self
            .check_rate_limit(identifier, max_requests, ttl::RATE_LIMIT_WINDOW)
            .await?;
        Ok(!allowed)
    }

    // ============================================================
    // SESSION MANAGEMENT
    // ============================================================

    /// Store a user session
    pub async fn set_session(
        &self,
        user_id: i64,
        token_hash: &str,
        device_info: Option<&str>,
    ) -> Result<(), RedisError> {
        let key = format!("{}:{}:{}", keys::SESSION, user_id, token_hash);
        let mut conn = self.conn.clone();

        let value = serde_json::json!({
            "user_id": user_id,
            "created_at": chrono::Utc::now().to_rfc3339(),
            "device": device_info.unwrap_or("unknown"),
        });

        conn.set_ex::<_, _, ()>(&key, value.to_string(), ttl::SESSION).await?;

        // Also add to user's session set (for listing all sessions)
        let sessions_key = format!("{}:{}:all", keys::SESSION, user_id);
        conn.sadd::<_, _, ()>(&sessions_key, token_hash).await?;
        conn.expire::<_, ()>(&sessions_key, ttl::SESSION as i64).await?;

        debug!("Session created for user {}", user_id);
        Ok(())
    }

    /// Validate a session exists
    pub async fn validate_session(&self, user_id: i64, token_hash: &str) -> Result<bool, RedisError> {
        let key = format!("{}:{}:{}", keys::SESSION, user_id, token_hash);
        let mut conn = self.conn.clone();
        let exists: bool = conn.exists(&key).await?;
        Ok(exists)
    }

    /// Invalidate a specific session (logout)
    pub async fn invalidate_session(&self, user_id: i64, token_hash: &str) -> Result<(), RedisError> {
        let key = format!("{}:{}:{}", keys::SESSION, user_id, token_hash);
        let sessions_key = format!("{}:{}:all", keys::SESSION, user_id);
        let mut conn = self.conn.clone();

        conn.del::<_, ()>(&key).await?;
        conn.srem::<_, _, ()>(&sessions_key, token_hash).await?;

        debug!("Session invalidated for user {}", user_id);
        Ok(())
    }

    /// Invalidate all sessions for a user (logout everywhere)
    pub async fn invalidate_all_sessions(&self, user_id: i64) -> Result<u32, RedisError> {
        let sessions_key = format!("{}:{}:all", keys::SESSION, user_id);
        let mut conn = self.conn.clone();

        // Get all session tokens
        let tokens: Vec<String> = conn.smembers(&sessions_key).await?;
        let count = tokens.len() as u32;

        // Delete each session
        for token in &tokens {
            let key = format!("{}:{}:{}", keys::SESSION, user_id, token);
            conn.del::<_, ()>(&key).await?;
        }

        // Delete the sessions set
        conn.del::<_, ()>(&sessions_key).await?;

        debug!("Invalidated {} sessions for user {}", count, user_id);
        Ok(count)
    }

    // ============================================================
    // OTP MANAGEMENT
    // ============================================================

    /// Store OTP for phone verification
    pub async fn set_otp(&self, phone: &str, otp: &str) -> Result<(), RedisError> {
        let key = format!("{}:{}", keys::OTP, phone);
        let mut conn = self.conn.clone();

        // Store OTP with attempt counter
        let value = serde_json::json!({
            "code": otp,
            "attempts": 0,
            "created_at": chrono::Utc::now().to_rfc3339(),
        });

        conn.set_ex::<_, _, ()>(&key, value.to_string(), ttl::OTP).await?;
        debug!("OTP set for phone {}", phone);
        Ok(())
    }

    /// Verify OTP (with attempt limiting)
    pub async fn verify_otp(&self, phone: &str, otp: &str) -> Result<bool, RedisError> {
        let key = format!("{}:{}", keys::OTP, phone);
        let mut conn = self.conn.clone();

        let data: Option<String> = conn.get(&key).await?;

        match data {
            Some(json_str) => {
                let mut data: serde_json::Value =
                    serde_json::from_str(&json_str).unwrap_or_default();

                let stored_otp = data["code"].as_str().unwrap_or("");
                let attempts = data["attempts"].as_u64().unwrap_or(0);

                // Max 5 attempts
                if attempts >= 5 {
                    warn!("OTP max attempts exceeded for {}", phone);
                    conn.del::<_, ()>(&key).await?;
                    return Ok(false);
                }

                if stored_otp == otp {
                    // Valid OTP - delete it
                    conn.del::<_, ()>(&key).await?;
                    debug!("OTP verified for {}", phone);
                    return Ok(true);
                }

                // Wrong OTP - increment attempts
                data["attempts"] = serde_json::json!(attempts + 1);
                let ttl_remaining: i64 = conn.ttl(&key).await?;
                if ttl_remaining > 0 {
                    conn.set_ex::<_, _, ()>(&key, data.to_string(), ttl_remaining as u64)
                        .await?;
                }

                debug!("Invalid OTP attempt {} for {}", attempts + 1, phone);
                Ok(false)
            }
            None => {
                debug!("No OTP found for {}", phone);
                Ok(false)
            }
        }
    }

    /// Get remaining OTP attempts
    pub async fn get_otp_attempts(&self, phone: &str) -> Result<u32, RedisError> {
        let key = format!("{}:{}", keys::OTP, phone);
        let mut conn = self.conn.clone();

        let data: Option<String> = conn.get(&key).await?;
        match data {
            Some(json_str) => {
                let data: serde_json::Value =
                    serde_json::from_str(&json_str).unwrap_or_default();
                let attempts = data["attempts"].as_u64().unwrap_or(0) as u32;
                Ok(5 - attempts.min(5))
            }
            None => Ok(0),
        }
    }

    // ============================================================
    // CACHING
    // ============================================================

    /// Generic cache set with TTL
    pub async fn cache_set<T: Serialize>(
        &self,
        prefix: &str,
        key: &str,
        value: &T,
        ttl_secs: u64,
    ) -> Result<(), RedisError> {
        let full_key = format!("{}:{}", prefix, key);
        let mut conn = self.conn.clone();

        let json = serde_json::to_string(value).map_err(|e| {
            RedisError::from((redis::ErrorKind::TypeError, "Serialization error", e.to_string()))
        })?;

        conn.set_ex::<_, _, ()>(&full_key, json, ttl_secs).await?;
        debug!("Cached {} (TTL: {}s)", full_key, ttl_secs);
        Ok(())
    }

    /// Generic cache get
    pub async fn cache_get<T: DeserializeOwned>(
        &self,
        prefix: &str,
        key: &str,
    ) -> Result<Option<T>, RedisError> {
        let full_key = format!("{}:{}", prefix, key);
        let mut conn = self.conn.clone();

        let data: Option<String> = conn.get(&full_key).await?;

        match data {
            Some(json) => {
                let value: T = serde_json::from_str(&json).map_err(|e| {
                    RedisError::from((
                        redis::ErrorKind::TypeError,
                        "Deserialization error",
                        e.to_string(),
                    ))
                })?;
                debug!("Cache hit: {}", full_key);
                Ok(Some(value))
            }
            None => {
                debug!("Cache miss: {}", full_key);
                Ok(None)
            }
        }
    }

    /// Delete cache entry
    pub async fn cache_del(&self, prefix: &str, key: &str) -> Result<(), RedisError> {
        let full_key = format!("{}:{}", prefix, key);
        let mut conn = self.conn.clone();
        conn.del::<_, ()>(&full_key).await?;
        debug!("Cache deleted: {}", full_key);
        Ok(())
    }

    /// Cache user profile
    pub async fn cache_user_profile<T: Serialize>(
        &self,
        user_id: i64,
        profile: &T,
    ) -> Result<(), RedisError> {
        self.cache_set(keys::PROFILE_CACHE, &user_id.to_string(), profile, ttl::PROFILE_CACHE)
            .await
    }

    /// Get cached user profile
    pub async fn get_cached_profile<T: DeserializeOwned>(
        &self,
        user_id: i64,
    ) -> Result<Option<T>, RedisError> {
        self.cache_get(keys::PROFILE_CACHE, &user_id.to_string()).await
    }

    /// Invalidate user profile cache
    pub async fn invalidate_profile_cache(&self, user_id: i64) -> Result<(), RedisError> {
        self.cache_del(keys::PROFILE_CACHE, &user_id.to_string()).await
    }

    // ============================================================
    // ONLINE STATUS
    // ============================================================

    /// Set user as online
    pub async fn set_online(&self, user_id: i64) -> Result<(), RedisError> {
        let mut conn = self.conn.clone();
        let now = chrono::Utc::now().timestamp();

        // Add to online users sorted set with timestamp as score
        conn.zadd::<_, _, _, ()>(keys::ONLINE_USERS, now, user_id).await?;

        // Set individual key for quick lookup
        let key = format!("{}:{}", keys::ONLINE_USERS, user_id);
        conn.set_ex::<_, _, ()>(&key, now.to_string(), ttl::ONLINE_STATUS).await?;

        Ok(())
    }

    /// Check if user is online
    pub async fn is_online(&self, user_id: i64) -> Result<bool, RedisError> {
        let key = format!("{}:{}", keys::ONLINE_USERS, user_id);
        let mut conn = self.conn.clone();
        let exists: bool = conn.exists(&key).await?;
        Ok(exists)
    }

    /// Get online users (with pagination)
    pub async fn get_online_users(&self, offset: isize, limit: isize) -> Result<Vec<i64>, RedisError> {
        let mut conn = self.conn.clone();
        let cutoff = chrono::Utc::now().timestamp() - ttl::ONLINE_STATUS as i64;

        // Get users who were active within the TTL window
        let users: Vec<i64> = conn
            .zrangebyscore_limit(keys::ONLINE_USERS, cutoff, "+inf", offset, limit)
            .await?;

        Ok(users)
    }

    /// Clean up stale online status entries
    pub async fn cleanup_online_status(&self) -> Result<u32, RedisError> {
        let mut conn = self.conn.clone();
        let cutoff = chrono::Utc::now().timestamp() - ttl::ONLINE_STATUS as i64;

        let removed: u32 = conn
            .zrembyscore(keys::ONLINE_USERS, "-inf", cutoff)
            .await?;

        if removed > 0 {
            debug!("Cleaned up {} stale online status entries", removed);
        }
        Ok(removed)
    }

    // ============================================================
    // TOKEN BLACKLIST
    // ============================================================

    /// Blacklist a token (for logout/revocation)
    pub async fn blacklist_token(&self, token_hash: &str, ttl_secs: u64) -> Result<(), RedisError> {
        let key = format!("{}:{}", keys::TOKEN_BLACKLIST, token_hash);
        let mut conn = self.conn.clone();
        conn.set_ex::<_, _, ()>(&key, "1", ttl_secs).await?;
        debug!("Token blacklisted");
        Ok(())
    }

    /// Check if token is blacklisted
    pub async fn is_token_blacklisted(&self, token_hash: &str) -> Result<bool, RedisError> {
        let key = format!("{}:{}", keys::TOKEN_BLACKLIST, token_hash);
        let mut conn = self.conn.clone();
        let exists: bool = conn.exists(&key).await?;
        Ok(exists)
    }

    // ============================================================
    // DISTRIBUTED LOCKING
    // ============================================================

    /// Acquire a distributed lock
    pub async fn acquire_lock(&self, resource: &str, ttl_secs: u64) -> Result<Option<String>, RedisError> {
        let key = format!("{}:{}", keys::LOCK, resource);
        let lock_id = uuid::Uuid::new_v4().to_string();
        let mut conn = self.conn.clone();

        // SET NX with expiry (atomic)
        let result: Option<String> = redis::cmd("SET")
            .arg(&key)
            .arg(&lock_id)
            .arg("NX")
            .arg("EX")
            .arg(ttl_secs)
            .query_async(&mut conn)
            .await?;

        match result {
            Some(_) => {
                debug!("Lock acquired on {}", resource);
                Ok(Some(lock_id))
            }
            None => {
                debug!("Failed to acquire lock on {}", resource);
                Ok(None)
            }
        }
    }

    /// Release a distributed lock
    pub async fn release_lock(&self, resource: &str, lock_id: &str) -> Result<bool, RedisError> {
        let key = format!("{}:{}", keys::LOCK, resource);
        let mut conn = self.conn.clone();

        // Only delete if we own the lock (Lua script for atomicity)
        let script = r#"
            if redis.call("get", KEYS[1]) == ARGV[1] then
                return redis.call("del", KEYS[1])
            else
                return 0
            end
        "#;

        let result: i32 = redis::Script::new(script)
            .key(&key)
            .arg(lock_id)
            .invoke_async(&mut conn)
            .await?;

        let released = result == 1;
        if released {
            debug!("Lock released on {}", resource);
        }
        Ok(released)
    }

    // ============================================================
    // HEALTH CHECK
    // ============================================================

    /// Ping Redis to check connection
    pub async fn ping(&self) -> Result<bool, RedisError> {
        let mut conn = self.conn.clone();
        let result: String = redis::cmd("PING").query_async(&mut conn).await?;
        Ok(result == "PONG")
    }

    /// Get Redis info for monitoring
    pub async fn info(&self) -> Result<String, RedisError> {
        let mut conn = self.conn.clone();
        let info: String = redis::cmd("INFO").query_async(&mut conn).await?;
        Ok(info)
    }

    // ============================================================
    // HORIZONTAL SCALING SUPPORT
    // ============================================================

    /// Register this instance as alive (call periodically)
    pub async fn register_instance(&self, instance_id: &str) -> Result<(), RedisError> {
        let key = format!("{}:{}", keys::INSTANCE_HEARTBEAT, instance_id);
        let mut conn = self.conn.clone();
        // Set instance heartbeat with 30 second TTL
        let _: () = conn.set_ex(&key, "alive", 30).await?;
        debug!("Instance {} heartbeat registered", instance_id);
        Ok(())
    }

    /// Get all active instances
    pub async fn get_active_instances(&self) -> Result<Vec<String>, RedisError> {
        let mut conn = self.conn.clone();
        let pattern = format!("{}:*", keys::INSTANCE_HEARTBEAT);
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await?;

        // Extract instance IDs from keys
        let instances: Vec<String> = keys
            .iter()
            .filter_map(|k| k.strip_prefix(&format!("{}:", keys::INSTANCE_HEARTBEAT)))
            .map(|s| s.to_string())
            .collect();

        Ok(instances)
    }

    /// Register WebSocket user location (which instance handles this user's WS connection)
    pub async fn register_ws_user(&self, user_id: i32, instance_id: &str) -> Result<(), RedisError> {
        let key = format!("{}:{}", keys::WS_USER_LOCATION, user_id);
        let mut conn = self.conn.clone();
        // Set with 5 minute TTL (refresh on WS activity)
        let _: () = conn.set_ex(&key, instance_id, 300).await?;
        Ok(())
    }

    /// Get which instance handles a user's WebSocket connection
    pub async fn get_ws_user_instance(&self, user_id: i32) -> Result<Option<String>, RedisError> {
        let key = format!("{}:{}", keys::WS_USER_LOCATION, user_id);
        let mut conn = self.conn.clone();
        let instance: Option<String> = conn.get(&key).await?;
        Ok(instance)
    }

    /// Remove user's WebSocket location (on disconnect)
    pub async fn unregister_ws_user(&self, user_id: i32) -> Result<(), RedisError> {
        let key = format!("{}:{}", keys::WS_USER_LOCATION, user_id);
        let mut conn = self.conn.clone();
        let _: () = conn.del(&key).await?;
        Ok(())
    }

    /// Publish event to all instances (for cross-instance messaging)
    pub async fn publish_event(&self, event_type: &str, payload: &str) -> Result<(), RedisError> {
        let mut conn = self.conn.clone();
        let message = format!("{}:{}", event_type, payload);
        let _: () = conn.publish(keys::PUBSUB_CHANNEL, message).await?;
        Ok(())
    }

    // ============================================================
    // CLUSTER STATS
    // ============================================================

    /// Get cluster-wide online user count
    pub async fn get_cluster_online_count(&self) -> Result<u64, RedisError> {
        let mut conn = self.conn.clone();
        let count: u64 = conn.zcard(keys::ONLINE_USERS).await.unwrap_or(0);
        Ok(count)
    }

    /// Get cluster-wide WebSocket connection count (across all instances)
    pub async fn get_cluster_ws_count(&self) -> Result<u64, RedisError> {
        let mut conn = self.conn.clone();
        let pattern = format!("{}:*", keys::WS_USER_LOCATION);
        let keys: Vec<String> = redis::cmd("KEYS")
            .arg(&pattern)
            .query_async(&mut conn)
            .await
            .unwrap_or_default();
        Ok(keys.len() as u64)
    }
}

/// Rate limiting middleware helper
pub struct RateLimiter {
    redis: RedisService,
    requests_per_minute: u32,
    burst: u32,
}

impl RateLimiter {
    pub fn new(redis: RedisService, requests_per_minute: u32, burst: u32) -> Self {
        Self {
            redis,
            requests_per_minute,
            burst,
        }
    }

    /// Check if request should be allowed
    /// identifier: IP address or user ID
    pub async fn check(&self, identifier: &str) -> Result<RateLimitResult, RedisError> {
        let (allowed, remaining, reset_in) = self
            .redis
            .check_rate_limit(identifier, self.requests_per_minute + self.burst, 60)
            .await?;

        Ok(RateLimitResult {
            allowed,
            remaining,
            reset_in,
            limit: self.requests_per_minute,
        })
    }
}

#[derive(Debug, Clone)]
pub struct RateLimitResult {
    pub allowed: bool,
    pub remaining: u32,
    pub reset_in: u64,
    pub limit: u32,
}

impl RateLimitResult {
    /// Get headers to include in HTTP response
    pub fn headers(&self) -> Vec<(&'static str, String)> {
        vec![
            ("X-RateLimit-Limit", self.limit.to_string()),
            ("X-RateLimit-Remaining", self.remaining.to_string()),
            ("X-RateLimit-Reset", self.reset_in.to_string()),
        ]
    }
}
