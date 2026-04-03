use std::env;

/// Read a secret from a file path (K8s mounted secret) or fall back to env var.
/// Supports secret rotation without restart: mount new secret file, app reads on next call.
fn secret_from_file_or_env(file_env: &str, value_env: &str, default: &str) -> String {
    // First check for a file path (K8s secret mount pattern: SECRET_KEY_FILE=/run/secrets/jwt_key)
    if let Ok(path) = env::var(file_env) {
        if let Ok(contents) = std::fs::read_to_string(&path) {
            return contents.trim().to_string();
        }
    }
    // Fall back to env var
    env::var(value_env).unwrap_or_else(|_| default.to_string())
}

#[derive(Clone, Debug)]
pub struct Config {
    // Server
    pub bind_addr: String,
    pub database_url: String,
    pub redis_url: String,
    pub environment: String,  // development, staging, production
    pub instance_id: String,  // Unique instance ID for horizontal scaling

    // Database Pool (scaled for 10k+ users)
    pub db_max_connections: u32,
    pub db_min_connections: u32,
    pub db_acquire_timeout_secs: u64,
    pub db_idle_timeout_secs: u64,
    pub db_max_lifetime_secs: u64,  // Connection max lifetime for PgBouncer compatibility

    // Read Replica Support
    pub db_read_replica_url: Option<String>,
    pub db_read_replica_enabled: bool,
    pub db_read_max_connections: u32,
    pub db_read_min_connections: u32,

    // Statement-level timeout to protect primary (milliseconds)
    pub db_statement_timeout_ms: u64,

    // Authentication
    pub secret_key: String,
    pub access_token_expire_minutes: i64,
    pub call_token_expire_minutes: i64,

    // Rate Limiting (scaled for high traffic)
    pub rate_limit_requests_per_minute: u32,
    pub rate_limit_burst: u32,
    pub rate_limit_premium_multiplier: f32,  // Premium users get higher limits

    // WebSocket Scaling
    pub ws_chat_buffer_size: usize,
    pub ws_call_buffer_size: usize,
    pub ws_max_connections_per_user: u32,

    // Worker Pool
    pub worker_threads: Option<usize>,  // None = auto-detect CPU cores
    pub blocking_threads: usize,

    // Vision
    pub vision_enabled: bool,
    pub vision_model_dir: String,
    pub vision_nsfw_model: String,
    pub vision_fer_model: String,
    pub vision_nima_model: String,
    pub vision_arcface_model: String,
    pub vision_liveness_model: String,
    pub selfie_match_threshold: f32,
    pub selfie_liveness_threshold: f32,

    // Upload limits
    pub max_photo_bytes: usize,
    pub max_video_bytes: usize,
    pub max_spot_duration_sec: i32,
    pub upload_dir: String,
    /// Public base URL of this server (e.g. https://api.nava.app).
    /// Used to construct absolute media URLs for push notification rich attachments.
    /// If empty, rich image attachments are skipped.
    pub public_url: String,

    // Discovery
    pub discover_limit: i32,
    pub default_max_distance_km: i32,

    // Premium features
    pub free_spots_limit: i32,
    pub spot_expiry_days: i32,

    // Pass Pricing (in cents)
    pub pass_price_hourly: i64,
    pub pass_price_daily: i64,
    pub pass_price_weekly: i64,
    pub pass_price_monthly: i64,
    pub pass_price_ultra: i64,

    // Student Discounts (as decimal: 0.50 = 50%)
    pub student_discount_ivy: f64,
    pub student_discount_top50: f64,
    pub student_discount_state: f64,
    pub student_discount_other: f64,
    pub student_discount_graduate: f64,
    pub student_discount_alumni: f64,

    // LLM Labeling
    pub llm_enabled: bool,
    pub llm_api_url: String,
    pub llm_model_name: String,
    pub llm_batch_size: i32,
    pub llm_max_retries: i32,

    // Federated Learning
    pub fl_enabled: bool,
    pub fl_min_clients: i32,
    pub fl_client_fraction: f64,
    pub fl_local_epochs: i32,
    pub fl_learning_rate: f64,
    pub fl_dp_enabled: bool,
    pub fl_noise_multiplier: f64,
    pub fl_clip_norm: f64,

    // Graceful Shutdown
    pub shutdown_timeout_secs: u64,

    // Request Timeouts
    pub request_timeout_secs: u64,

    // SMTP Email
    pub smtp_host: String,
    pub smtp_username: String,
    pub smtp_password: String,
    pub smtp_from: String,

    // RevenueCat (In-App Purchases)
    pub revenuecat_webhook_secret: Option<String>,

    // Payment Gateways
    // Razorpay (India)
    pub razorpay_key_id: String,
    pub razorpay_key_secret: String,
    pub razorpay_webhook_secret: String,

    // Stripe (Global - USA, EU, etc.)
    pub stripe_secret_key: String,
    pub stripe_publishable_key: String,
    pub stripe_webhook_secret: String,

    // Payment Settings
    pub payment_default_currency: String,
    pub payment_test_mode: bool,

    // Ads Configuration
    pub ads_enabled: bool,
    pub admob_app_id: String,
    pub admob_banner_unit_id: String,
    pub admob_interstitial_unit_id: String,
    pub admob_rewarded_unit_id: String,
    pub facebook_ads_app_id: String,
    pub unity_ads_game_id: String,
    pub ads_free_user_interstitial_interval: i32,
    pub ads_rewarded_cooldown_minutes: i32,

    // CORS Configuration
    pub cors_allowed_origins: Vec<String>,

    // Health Check & Load Balancer
    pub health_check_path: String,
    pub ready_check_enabled: bool,

    // Connection Pooling (for PgBouncer)
    pub pgbouncer_mode: bool,  // Enables PgBouncer-compatible settings

    // Trust & Safety
    pub trust_safety_enabled: bool,
    pub trust_safety_auto_ban_threshold: f64,

    // Content Moderation
    pub moderation_enabled: bool,
    pub moderation_toxicity_threshold: f64,
    pub moderation_nsfw_threshold: f64,

    // Content Freshness
    pub freshness_decay_enabled: bool,

    // Reel RL / Reward formula
    /// Multiplier applied to reel reward when viewer and creator share the same city.
    /// Range 1.0 (off) – 1.20; tunable via REEL_SAME_CITY_MULTIPLIER env var.
    pub reel_same_city_multiplier: f64,
    /// Tag stored alongside every logged reward so shadow-bucket analysis can
    /// filter by formula version. Bump via REEL_REWARD_VERSION env var.
    pub reel_reward_version: String,

    // Shadow bucket / A-B rollout
    /// Fraction of users assigned to the v2 reward cohort (0.0–1.0).
    /// Assignment is deterministic: user_id % 100 < (fraction * 100).
    /// 0.10 = 10% shadow cohort. Set to 1.0 to promote v2 to everyone.
    pub reel_shadow_cohort_fraction: f64,
    /// Minimum days a shadow bucket must run before promotion is considered.
    pub reel_shadow_min_days: u32,
    /// Minimum watch-time uplift (fraction, e.g. 0.05 = 5%) required to promote.
    pub reel_shadow_promote_watch_uplift: f64,
    /// Maximum skip-rate increase (fraction) allowed before auto-rollback.
    pub reel_shadow_rollback_skip_spike: f64,
}

impl Config {
    pub fn from_env() -> Self {
        let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
        let database_url = env::var("DATABASE_URL").unwrap_or_default();
        let secret_key = secret_from_file_or_env(
            "SECRET_KEY_FILE", "SECRET_KEY", "your-secret-key-change-in-production",
        );
        let access_token_expire_minutes = env::var("ACCESS_TOKEN_EXPIRE_MINUTES")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(60); // 1 hour
        let call_token_expire_minutes = env::var("CALL_TOKEN_EXPIRE_MINUTES")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(15);
        let vision_enabled = env::var("VISION_ENABLED")
            .ok()
            .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(true);
        let vision_model_dir = env::var("VISION_MODEL_DIR").unwrap_or_else(|_| "models".to_string());
        let vision_nsfw_model = env::var("VISION_NSFW_MODEL").unwrap_or_else(|_| "nsfw.onnx".to_string());
        let vision_fer_model = env::var("VISION_FER_MODEL").unwrap_or_else(|_| "ferplus.onnx".to_string());
        let vision_nima_model = env::var("VISION_NIMA_MODEL").unwrap_or_else(|_| "nima.onnx".to_string());
        let vision_arcface_model =
            env::var("VISION_ARCFACE_MODEL").unwrap_or_else(|_| "arcface.onnx".to_string());
        let vision_liveness_model =
            env::var("VISION_LIVENESS_MODEL").unwrap_or_else(|_| "minifasnet.onnx".to_string());
        let selfie_match_threshold = env::var("SELFIE_MATCH_THRESHOLD")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(0.45);
        let selfie_liveness_threshold = env::var("SELFIE_LIVENESS_THRESHOLD")
            .ok()
            .and_then(|value| value.parse::<f32>().ok())
            .unwrap_or(0.5);
        let max_photo_bytes = env::var("MAX_PHOTO_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(75 * 1024 * 1024); // 75MB — covers ProRAW (~25MB) and HEIC bursts
        let max_video_bytes = env::var("MAX_VIDEO_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(2 * 1024 * 1024 * 1024); // 2GB — accept any phone video, server normalizes
        let max_spot_duration_sec = env::var("MAX_SPOT_DURATION_SEC")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(30);
        let public_url = env::var("PUBLIC_URL").unwrap_or_default();
        let upload_dir = env::var("UPLOAD_DIR").unwrap_or_else(|_| {
            // Default to an absolute path next to the binary so files survive restarts
            let mut p = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
            p.push("uploads");
            p.to_string_lossy().to_string()
        });
        let discover_limit = env::var("DISCOVER_LIMIT")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(20);
        let default_max_distance_km = env::var("DEFAULT_MAX_DISTANCE_KM")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(50);
        let free_spots_limit = env::var("FREE_SPOTS_LIMIT")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(2);
        let spot_expiry_days = env::var("SPOT_EXPIRY_DAYS")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(15);

        // Redis
        let redis_url = env::var("REDIS_URL").unwrap_or_else(|_| "redis://127.0.0.1:6379".to_string());
        let environment = env::var("ENVIRONMENT").unwrap_or_else(|_| "development".to_string());

        // Instance ID for horizontal scaling (auto-generate if not set)
        let instance_id = env::var("INSTANCE_ID").unwrap_or_else(|_| {
            format!("nava-{}", uuid::Uuid::new_v4().to_string().split('-').next().unwrap_or("0000"))
        });

        // Determine if production for auto-scaling defaults
        let is_prod = environment == "production";

        // Database Pool - SCALED FOR 10K+ USERS
        // Production: 300 max connections (with PgBouncer can handle 10k+ concurrent)
        // Development: 100 connections
        let db_max_connections = env::var("DB_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(if is_prod { 300 } else { 100 });
        let db_min_connections = env::var("DB_MIN_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(if is_prod { 50 } else { 10 });
        let db_acquire_timeout_secs = env::var("DB_ACQUIRE_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(if is_prod { 10 } else { 30 });  // Faster timeout in prod
        let db_idle_timeout_secs = env::var("DB_IDLE_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300);  // 5 minutes
        let db_max_lifetime_secs = env::var("DB_MAX_LIFETIME_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1800);  // 30 minutes - important for PgBouncer

        // Read Replica Support
        let db_read_replica_url = env::var("DB_READ_REPLICA_URL").ok();
        let db_read_replica_enabled = env::var("DB_READ_REPLICA_ENABLED")
            .ok()
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(false);
        let db_read_max_connections = env::var("DB_READ_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(db_max_connections / 2);  // Default: half of primary
        let db_read_min_connections = env::var("DB_READ_MIN_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(db_min_connections / 2);
        // Statement timeout protects primary from long-running queries (default 30s)
        let db_statement_timeout_ms = env::var("DB_STATEMENT_TIMEOUT_MS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(if is_prod { 15000 } else { 30000 });

        // Rate Limiting - SCALED FOR HIGH TRAFFIC
        // Production: 120 req/min + 30 burst = 150 total per user
        let rate_limit_requests_per_minute = env::var("RATE_LIMIT_RPM")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(if is_prod { 120 } else { 60 });
        let rate_limit_burst = env::var("RATE_LIMIT_BURST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(if is_prod { 30 } else { 10 });
        let rate_limit_premium_multiplier = env::var("RATE_LIMIT_PREMIUM_MULTIPLIER")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2.0_f32);  // Premium users get 2x rate limit

        // WebSocket Scaling
        let ws_chat_buffer_size = env::var("WS_CHAT_BUFFER_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(if is_prod { 500 } else { 100 });
        let ws_call_buffer_size = env::var("WS_CALL_BUFFER_SIZE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(if is_prod { 200 } else { 50 });
        let ws_max_connections_per_user = env::var("WS_MAX_CONNECTIONS_PER_USER")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);  // Max 5 devices per user

        // Worker Pool Configuration
        let worker_threads = env::var("WORKER_THREADS")
            .ok()
            .and_then(|v| v.parse().ok());  // None = auto-detect
        let blocking_threads = env::var("BLOCKING_THREADS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(512);  // For blocking I/O operations

        // Pass Pricing (cents) - Competitive pricing for young professionals
        let pass_price_hourly = env::var("PASS_PRICE_HOURLY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(299);      // $2.99 - Quick boost/spotlight
        let pass_price_daily = env::var("PASS_PRICE_DAILY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(499);      // $4.99 - Day pass
        let pass_price_weekly = env::var("PASS_PRICE_WEEKLY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(999);      // $9.99/week
        let pass_price_monthly = env::var("PASS_PRICE_MONTHLY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1999);     // $19.99/month (most popular)
        let pass_price_ultra = env::var("PASS_PRICE_ULTRA")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(4999);     // $49.99 - 3 months (best value)

        // Student Discounts (decimal: 0.30 = 30%)
        let student_discount_ivy = env::var("STUDENT_DISCOUNT_IVY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.30);
        let student_discount_top50 = env::var("STUDENT_DISCOUNT_TOP50")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.20);
        let student_discount_state = env::var("STUDENT_DISCOUNT_STATE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.15);
        let student_discount_other = env::var("STUDENT_DISCOUNT_OTHER")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.10);
        let student_discount_graduate = env::var("STUDENT_DISCOUNT_GRADUATE")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.15);
        let student_discount_alumni = env::var("STUDENT_DISCOUNT_ALUMNI")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.05);

        // Timeouts
        let shutdown_timeout_secs = env::var("SHUTDOWN_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        let request_timeout_secs = env::var("REQUEST_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(300); // 5 minutes — video uploads need time

        // LLM Labeling
        let llm_enabled = env::var("LLM_ENABLED")
            .ok()
            .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(true);
        let llm_api_url = env::var("LLM_API_URL").unwrap_or_else(|_| "http://localhost:8000".to_string());
        let llm_model_name = env::var("LLM_MODEL_NAME").unwrap_or_else(|_| "llama3".to_string());
        let llm_batch_size = env::var("LLM_BATCH_SIZE")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(10);
        let llm_max_retries = env::var("LLM_MAX_RETRIES")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(3);

        // Federated Learning
        let fl_enabled = env::var("FL_ENABLED")
            .ok()
            .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(true);
        let fl_min_clients = env::var("FL_MIN_CLIENTS")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(10);
        let fl_client_fraction = env::var("FL_CLIENT_FRACTION")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.1);
        let fl_local_epochs = env::var("FL_LOCAL_EPOCHS")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(1);
        let fl_learning_rate = env::var("FL_LEARNING_RATE")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(0.01);
        let fl_dp_enabled = env::var("FL_DP_ENABLED")
            .ok()
            .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(true);
        let fl_noise_multiplier = env::var("FL_NOISE_MULTIPLIER")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(1.0);
        let fl_clip_norm = env::var("FL_CLIP_NORM")
            .ok()
            .and_then(|value| value.parse::<f64>().ok())
            .unwrap_or(1.0);

        // SMTP Email
        let smtp_host = env::var("SMTP_HOST").unwrap_or_else(|_| "smtp.gmail.com".to_string());
        let smtp_username = env::var("SMTP_USERNAME").unwrap_or_default();
        let smtp_password = env::var("SMTP_PASSWORD").unwrap_or_default();
        let smtp_from = env::var("SMTP_FROM").unwrap_or_else(|_| "NAVA <noreply@nava.app>".to_string());

        // RevenueCat
        let revenuecat_webhook_secret = env::var("REVENUECAT_WEBHOOK_SECRET").ok();

        // Payment Gateways - Razorpay (India) — supports file-based secrets for rotation
        let razorpay_key_id = env::var("RAZORPAY_KEY_ID").unwrap_or_default();
        let razorpay_key_secret = secret_from_file_or_env(
            "RAZORPAY_KEY_SECRET_FILE", "RAZORPAY_KEY_SECRET", "",
        );
        let razorpay_webhook_secret = secret_from_file_or_env(
            "RAZORPAY_WEBHOOK_SECRET_FILE", "RAZORPAY_WEBHOOK_SECRET", "",
        );

        // Payment Gateways - Stripe (Global) — supports file-based secrets for rotation
        let stripe_secret_key = secret_from_file_or_env(
            "STRIPE_SECRET_KEY_FILE", "STRIPE_SECRET_KEY", "",
        );
        let stripe_publishable_key = env::var("STRIPE_PUBLISHABLE_KEY").unwrap_or_default();
        let stripe_webhook_secret = secret_from_file_or_env(
            "STRIPE_WEBHOOK_SECRET_FILE", "STRIPE_WEBHOOK_SECRET", "",
        );

        // Payment Settings
        let payment_default_currency = env::var("PAYMENT_DEFAULT_CURRENCY")
            .unwrap_or_else(|_| "USD".to_string());
        let payment_test_mode = env::var("PAYMENT_TEST_MODE")
            .ok()
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(!is_prod);  // Default to test mode in dev

        // Ads Configuration
        let ads_enabled = env::var("ADS_ENABLED")
            .ok()
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(true);
        let admob_app_id = env::var("ADMOB_APP_ID").unwrap_or_default();
        let admob_banner_unit_id = env::var("ADMOB_BANNER_UNIT_ID").unwrap_or_default();
        let admob_interstitial_unit_id = env::var("ADMOB_INTERSTITIAL_UNIT_ID").unwrap_or_default();
        let admob_rewarded_unit_id = env::var("ADMOB_REWARDED_UNIT_ID").unwrap_or_default();
        let facebook_ads_app_id = env::var("FACEBOOK_ADS_APP_ID").unwrap_or_default();
        let unity_ads_game_id = env::var("UNITY_ADS_GAME_ID").unwrap_or_default();
        let ads_free_user_interstitial_interval = env::var("ADS_FREE_USER_INTERSTITIAL_INTERVAL")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(5);  // Show interstitial every 5 actions for free users
        let ads_rewarded_cooldown_minutes = env::var("ADS_REWARDED_COOLDOWN_MINUTES")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);  // 30 min cooldown between rewarded ads

        // CORS Configuration
        let cors_allowed_origins = env::var("CORS_ALLOWED_ORIGINS")
            .unwrap_or_else(|_| "https://nava.dating,https://app.nava.dating".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        // Health Check & Load Balancer
        let health_check_path = env::var("HEALTH_CHECK_PATH")
            .unwrap_or_else(|_| "/health".to_string());
        let ready_check_enabled = env::var("READY_CHECK_ENABLED")
            .ok()
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(true);

        // PgBouncer Compatibility Mode
        let pgbouncer_mode = env::var("PGBOUNCER_MODE")
            .ok()
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(is_prod);  // Auto-enable in production

        // Trust & Safety
        let trust_safety_enabled = env::var("TRUST_SAFETY_ENABLED")
            .ok()
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(true);
        let trust_safety_auto_ban_threshold = env::var("TRUST_SAFETY_AUTO_BAN_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.85);

        // Content Moderation
        let moderation_enabled = env::var("MODERATION_ENABLED")
            .ok()
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(true);
        let moderation_toxicity_threshold = env::var("MODERATION_TOXICITY_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.7);
        let moderation_nsfw_threshold = env::var("MODERATION_NSFW_THRESHOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0.7);

        // Content Freshness
        let freshness_decay_enabled = env::var("FRESHNESS_DECAY_ENABLED")
            .ok()
            .map(|v| matches!(v.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(true);

        // Reel RL / Reward formula
        let reel_same_city_multiplier = env::var("REEL_SAME_CITY_MULTIPLIER")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(1.10)
            .clamp(1.0, 1.20); // safety guard — never more than 1.20x
        let reel_reward_version = env::var("REEL_REWARD_VERSION")
            .unwrap_or_else(|_| "v2".to_string());

        // Shadow bucket
        let reel_shadow_cohort_fraction = env::var("REEL_SHADOW_COHORT_FRACTION")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.10)          // 10% default shadow cohort
            .clamp(0.0, 1.0);
        let reel_shadow_min_days = env::var("REEL_SHADOW_MIN_DAYS")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(7);            // must run 7 days before promotion
        let reel_shadow_promote_watch_uplift = env::var("REEL_SHADOW_PROMOTE_WATCH_UPLIFT")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.05);         // 5% watch-time uplift needed
        let reel_shadow_rollback_skip_spike = env::var("REEL_SHADOW_ROLLBACK_SKIP_SPIKE")
            .ok()
            .and_then(|v| v.parse::<f64>().ok())
            .unwrap_or(0.15);         // rollback if skip rate up > 15%

        Self {
            bind_addr,
            database_url,
            redis_url,
            environment,
            instance_id,
            db_max_connections,
            db_min_connections,
            db_acquire_timeout_secs,
            db_idle_timeout_secs,
            db_max_lifetime_secs,
            db_read_replica_url,
            db_read_replica_enabled,
            db_read_max_connections,
            db_read_min_connections,
            db_statement_timeout_ms,
            secret_key,
            access_token_expire_minutes,
            call_token_expire_minutes,
            rate_limit_requests_per_minute,
            rate_limit_burst,
            rate_limit_premium_multiplier,
            ws_chat_buffer_size,
            ws_call_buffer_size,
            ws_max_connections_per_user,
            worker_threads,
            blocking_threads,
            vision_enabled,
            vision_model_dir,
            vision_nsfw_model,
            vision_fer_model,
            vision_nima_model,
            vision_arcface_model,
            vision_liveness_model,
            selfie_match_threshold,
            selfie_liveness_threshold,
            max_photo_bytes,
            max_video_bytes,
            max_spot_duration_sec,
            upload_dir,
            public_url,
            discover_limit,
            default_max_distance_km,
            free_spots_limit,
            spot_expiry_days,
            pass_price_hourly,
            pass_price_daily,
            pass_price_weekly,
            pass_price_monthly,
            pass_price_ultra,
            student_discount_ivy,
            student_discount_top50,
            student_discount_state,
            student_discount_other,
            student_discount_graduate,
            student_discount_alumni,
            llm_enabled,
            llm_api_url,
            llm_model_name,
            llm_batch_size,
            llm_max_retries,
            fl_enabled,
            fl_min_clients,
            fl_client_fraction,
            fl_local_epochs,
            fl_learning_rate,
            fl_dp_enabled,
            fl_noise_multiplier,
            fl_clip_norm,
            shutdown_timeout_secs,
            request_timeout_secs,
            smtp_host,
            smtp_username,
            smtp_password,
            smtp_from,
            revenuecat_webhook_secret,
            razorpay_key_id,
            razorpay_key_secret,
            razorpay_webhook_secret,
            stripe_secret_key,
            stripe_publishable_key,
            stripe_webhook_secret,
            payment_default_currency,
            payment_test_mode,
            ads_enabled,
            admob_app_id,
            admob_banner_unit_id,
            admob_interstitial_unit_id,
            admob_rewarded_unit_id,
            facebook_ads_app_id,
            unity_ads_game_id,
            ads_free_user_interstitial_interval,
            ads_rewarded_cooldown_minutes,
            cors_allowed_origins,
            health_check_path,
            ready_check_enabled,
            pgbouncer_mode,
            trust_safety_enabled,
            trust_safety_auto_ban_threshold,
            moderation_enabled,
            moderation_toxicity_threshold,
            moderation_nsfw_threshold,
            freshness_decay_enabled,
            reel_same_city_multiplier,
            reel_reward_version,
            reel_shadow_cohort_fraction,
            reel_shadow_min_days,
            reel_shadow_promote_watch_uplift,
            reel_shadow_rollback_skip_spike,
        }
    }

    pub fn is_dev_mode(&self) -> bool {
        self.environment != "production"
    }

    pub fn is_production(&self) -> bool {
        self.environment == "production"
    }

    /// Reload secrets from file/env at runtime (for zero-downtime rotation).
    /// Call via admin endpoint or SIGHUP handler.
    pub fn reload_secrets(&mut self) -> Vec<String> {
        let mut rotated = Vec::new();

        let new_secret = secret_from_file_or_env(
            "SECRET_KEY_FILE", "SECRET_KEY", "your-secret-key-change-in-production",
        );
        if new_secret != self.secret_key {
            self.secret_key = new_secret;
            rotated.push("secret_key".to_string());
        }

        let new_rz = secret_from_file_or_env("RAZORPAY_KEY_SECRET_FILE", "RAZORPAY_KEY_SECRET", "");
        if new_rz != self.razorpay_key_secret {
            self.razorpay_key_secret = new_rz;
            rotated.push("razorpay_key_secret".to_string());
        }

        let new_rz_wh = secret_from_file_or_env("RAZORPAY_WEBHOOK_SECRET_FILE", "RAZORPAY_WEBHOOK_SECRET", "");
        if new_rz_wh != self.razorpay_webhook_secret {
            self.razorpay_webhook_secret = new_rz_wh;
            rotated.push("razorpay_webhook_secret".to_string());
        }

        let new_stripe = secret_from_file_or_env("STRIPE_SECRET_KEY_FILE", "STRIPE_SECRET_KEY", "");
        if new_stripe != self.stripe_secret_key {
            self.stripe_secret_key = new_stripe;
            rotated.push("stripe_secret_key".to_string());
        }

        let new_stripe_wh = secret_from_file_or_env("STRIPE_WEBHOOK_SECRET_FILE", "STRIPE_WEBHOOK_SECRET", "");
        if new_stripe_wh != self.stripe_webhook_secret {
            self.stripe_webhook_secret = new_stripe_wh;
            rotated.push("stripe_webhook_secret".to_string());
        }

        rotated
    }

    pub fn validate(&self) -> Result<(), String> {
        if self.database_url.is_empty() {
            return Err("DATABASE_URL is required".to_string());
        }
        if self.is_production() && self.secret_key == "your-secret-key-change-in-production" {
            return Err("SECRET_KEY must be changed in production".to_string());
        }
        if self.is_production() && self.secret_key.len() < 32 {
            return Err("SECRET_KEY must be at least 32 characters in production".to_string());
        }
        Ok(())
    }
}
