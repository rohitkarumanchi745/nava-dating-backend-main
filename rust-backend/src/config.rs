use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    // Server
    pub bind_addr: String,
    pub database_url: String,
    pub redis_url: String,
    pub environment: String,  // development, staging, production

    // Database Pool
    pub db_max_connections: u32,
    pub db_min_connections: u32,
    pub db_acquire_timeout_secs: u64,
    pub db_idle_timeout_secs: u64,

    // Authentication
    pub secret_key: String,
    pub access_token_expire_minutes: i64,
    pub call_token_expire_minutes: i64,

    // Rate Limiting
    pub rate_limit_requests_per_minute: u32,
    pub rate_limit_burst: u32,

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
}

impl Config {
    pub fn from_env() -> Self {
        let bind_addr = env::var("BIND_ADDR").unwrap_or_else(|_| "0.0.0.0:8080".to_string());
        let database_url = env::var("DATABASE_URL").unwrap_or_default();
        let secret_key =
            env::var("SECRET_KEY").unwrap_or_else(|_| "your-secret-key-change-in-production".to_string());
        let access_token_expire_minutes = env::var("ACCESS_TOKEN_EXPIRE_MINUTES")
            .ok()
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(10080); // 7 days
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
            .unwrap_or(10 * 1024 * 1024); // 10MB
        let max_video_bytes = env::var("MAX_VIDEO_BYTES")
            .ok()
            .and_then(|value| value.parse::<usize>().ok())
            .unwrap_or(50 * 1024 * 1024); // 50MB
        let max_spot_duration_sec = env::var("MAX_SPOT_DURATION_SEC")
            .ok()
            .and_then(|value| value.parse::<i32>().ok())
            .unwrap_or(30);
        let upload_dir = env::var("UPLOAD_DIR").unwrap_or_else(|_| "uploads".to_string());
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

        // Database Pool
        let db_max_connections = env::var("DB_MAX_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(100);
        let db_min_connections = env::var("DB_MIN_CONNECTIONS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);
        let db_acquire_timeout_secs = env::var("DB_ACQUIRE_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(30);
        let db_idle_timeout_secs = env::var("DB_IDLE_TIMEOUT_SECS")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(600);

        // Rate Limiting
        let rate_limit_requests_per_minute = env::var("RATE_LIMIT_RPM")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(60);
        let rate_limit_burst = env::var("RATE_LIMIT_BURST")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(10);

        // Pass Pricing (cents)
        let pass_price_hourly = env::var("PASS_PRICE_HOURLY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(1200);
        let pass_price_daily = env::var("PASS_PRICE_DAILY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(2000);
        let pass_price_weekly = env::var("PASS_PRICE_WEEKLY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(9900);
        let pass_price_monthly = env::var("PASS_PRICE_MONTHLY")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(29900);
        let pass_price_ultra = env::var("PASS_PRICE_ULTRA")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(49900);

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
            .unwrap_or(30);

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

        Self {
            bind_addr,
            database_url,
            redis_url,
            environment,
            db_max_connections,
            db_min_connections,
            db_acquire_timeout_secs,
            db_idle_timeout_secs,
            secret_key,
            access_token_expire_minutes,
            call_token_expire_minutes,
            rate_limit_requests_per_minute,
            rate_limit_burst,
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
        }
    }

    pub fn is_production(&self) -> bool {
        self.environment == "production"
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
