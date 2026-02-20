//! Common configuration utilities

use std::env;

/// Get environment variable or panic
pub fn get_env(key: &str) -> String {
    env::var(key).unwrap_or_else(|_| panic!("{} must be set", key))
}

/// Get environment variable with default
pub fn get_env_or(key: &str, default: &str) -> String {
    env::var(key).unwrap_or_else(|_| default.to_string())
}

/// Get environment variable as u16
pub fn get_env_port(key: &str, default: u16) -> u16 {
    env::var(key)
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(default)
}

/// Service URLs for inter-service communication
#[derive(Debug, Clone)]
pub struct ServiceUrls {
    pub auth_service: String,
    pub user_service: String,
    pub match_service: String,
    pub chat_service: String,
    pub payment_service: String,
    pub ambassador_service: String,
    pub notification_service: String,
    pub analytics_service: String,
}

impl ServiceUrls {
    pub fn from_env() -> Self {
        Self {
            auth_service: get_env_or("AUTH_SERVICE_URL", "http://localhost:8001"),
            user_service: get_env_or("USER_SERVICE_URL", "http://localhost:8002"),
            match_service: get_env_or("MATCH_SERVICE_URL", "http://localhost:8003"),
            chat_service: get_env_or("CHAT_SERVICE_URL", "http://localhost:8004"),
            payment_service: get_env_or("PAYMENT_SERVICE_URL", "http://localhost:8005"),
            ambassador_service: get_env_or("AMBASSADOR_SERVICE_URL", "http://localhost:8006"),
            notification_service: get_env_or("NOTIFICATION_SERVICE_URL", "http://localhost:8007"),
            analytics_service: get_env_or("ANALYTICS_SERVICE_URL", "http://localhost:8008"),
        }
    }
}

/// Kafka configuration
#[derive(Debug, Clone)]
pub struct KafkaConfig {
    pub bootstrap_servers: String,
    pub client_id: String,
    pub group_id: Option<String>,
}

impl KafkaConfig {
    pub fn from_env(service_name: &str) -> Self {
        Self {
            bootstrap_servers: get_env_or("KAFKA_BOOTSTRAP_SERVERS", "localhost:9092"),
            client_id: get_env_or("KAFKA_CLIENT_ID", service_name),
            group_id: std::env::var("KAFKA_GROUP_ID").ok(),
        }
    }
}
