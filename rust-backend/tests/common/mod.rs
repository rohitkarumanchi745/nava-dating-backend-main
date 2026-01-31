//! Common test utilities and fixtures
//!
//! This module provides:
//! - Test configuration loading
//! - JWT token generation for tests
//! - Test server setup utilities
//! - Database fixtures and cleanup

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Router;
use sqlx::PgPool;
use tokio::net::TcpListener;

/// Test configuration for integration tests
pub struct TestConfig {
    pub database_url: String,
    pub redis_url: String,
    pub secret_key: String,
    pub bind_addr: String,
}

impl Default for TestConfig {
    fn default() -> Self {
        Self {
            database_url: std::env::var("TEST_DATABASE_URL")
                .unwrap_or_else(|_| "postgres://nava:nava123@localhost:5432/nava_test".to_string()),
            redis_url: std::env::var("TEST_REDIS_URL")
                .unwrap_or_else(|_| "redis://localhost:6379".to_string()),
            secret_key: "test_secret_key_for_jwt_signing_32chars!".to_string(),
            bind_addr: "127.0.0.1:0".to_string(), // Port 0 = auto-assign
        }
    }
}

/// Create a test JWT token for a given user ID
pub fn create_test_token(user_id: i32, secret: &str) -> String {
    use chrono::{Duration, Utc};
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct Claims {
        sub: String,
        exp: usize,
        is_admin: bool,
    }

    let exp = (Utc::now() + Duration::hours(1)).timestamp() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        exp,
        is_admin: false,
    };

    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap()
}

/// Create an admin test JWT token
pub fn create_admin_test_token(user_id: i32, secret: &str) -> String {
    use chrono::{Duration, Utc};
    use jsonwebtoken::{encode, Algorithm, EncodingKey, Header};
    use serde::{Deserialize, Serialize};

    #[derive(Serialize, Deserialize)]
    struct Claims {
        sub: String,
        exp: usize,
        is_admin: bool,
    }

    let exp = (Utc::now() + Duration::hours(1)).timestamp() as usize;
    let claims = Claims {
        sub: user_id.to_string(),
        exp,
        is_admin: true,
    };

    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(secret.as_bytes()),
    )
    .unwrap()
}

/// Test HTTP client with base URL
pub struct TestClient {
    pub client: reqwest::Client,
    pub base_url: String,
}

impl TestClient {
    pub fn new(base_url: String) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url,
        }
    }

    pub async fn get(&self, path: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{}", self.base_url, path))
            .send()
            .await
            .expect("Failed to send request")
    }

    pub async fn get_with_auth(&self, path: &str, token: &str) -> reqwest::Response {
        self.client
            .get(format!("{}{}", self.base_url, path))
            .header("Authorization", format!("Bearer {}", token))
            .send()
            .await
            .expect("Failed to send request")
    }

    pub async fn post_json<T: serde::Serialize>(&self, path: &str, body: &T) -> reqwest::Response {
        self.client
            .post(format!("{}{}", self.base_url, path))
            .json(body)
            .send()
            .await
            .expect("Failed to send request")
    }

    pub async fn post_json_with_auth<T: serde::Serialize>(
        &self,
        path: &str,
        body: &T,
        token: &str,
    ) -> reqwest::Response {
        self.client
            .post(format!("{}{}", self.base_url, path))
            .header("Authorization", format!("Bearer {}", token))
            .json(body)
            .send()
            .await
            .expect("Failed to send request")
    }
}

/// Setup test database connection
pub async fn setup_test_db() -> Option<PgPool> {
    let config = TestConfig::default();

    match PgPool::connect(&config.database_url).await {
        Ok(pool) => {
            // Run migrations
            if let Err(e) = sqlx::migrate!("./migrations").run(&pool).await {
                eprintln!("Failed to run migrations: {}", e);
                return None;
            }
            Some(pool)
        }
        Err(e) => {
            eprintln!("Failed to connect to test database: {}", e);
            None
        }
    }
}

/// Clean up test data for a specific phone number
pub async fn cleanup_test_user(pool: &PgPool, phone_number: &str) {
    let _ = sqlx::query("DELETE FROM users WHERE phone_number = $1")
        .bind(phone_number)
        .execute(pool)
        .await;
}

/// Test phone number for integration tests (use unique prefix to avoid conflicts)
pub const TEST_PHONE_PREFIX: &str = "+1999999";

/// Generate a unique test phone number
pub fn generate_test_phone() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis();
    format!("{}{}", TEST_PHONE_PREFIX, timestamp % 10000)
}
