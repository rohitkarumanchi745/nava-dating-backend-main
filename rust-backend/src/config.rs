use std::env;

#[derive(Clone, Debug)]
pub struct Config {
    pub bind_addr: String,
    pub database_url: String,
    pub secret_key: String,
    pub access_token_expire_minutes: i64,
    pub vision_enabled: bool,
    pub vision_model_dir: String,
    pub vision_nsfw_model: String,
    pub vision_fer_model: String,
    pub vision_nima_model: String,
    pub vision_arcface_model: String,
    pub vision_liveness_model: String,
    pub selfie_match_threshold: f32,
    pub selfie_liveness_threshold: f32,
    pub max_photo_bytes: usize,
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
            .unwrap_or(30);
        let vision_enabled = env::var("VISION_ENABLED")
            .ok()
            .map(|value| matches!(value.as_str(), "1" | "true" | "yes" | "on"))
            .unwrap_or(true);
        let vision_model_dir = env::var("VISION_MODEL_DIR").unwrap_or_else(|_| "rust-backend/models".to_string());
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
            .unwrap_or(10 * 1024 * 1024);
        Self {
            bind_addr,
            database_url,
            secret_key,
            access_token_expire_minutes,
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
        }
    }
}
