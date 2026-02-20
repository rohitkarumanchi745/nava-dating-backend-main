//! Integration testing utilities with testcontainers
//!
//! Provides helpers for spinning up PostgreSQL, Redis, and Kafka
//! containers for integration tests.

use std::collections::HashMap;
use std::sync::Arc;

/// Test container configuration
#[derive(Debug, Clone)]
pub struct TestContainerConfig {
    /// PostgreSQL image
    pub postgres_image: String,
    /// Redis image
    pub redis_image: String,
    /// Kafka image
    pub kafka_image: String,
    /// Zookeeper image (for Kafka)
    pub zookeeper_image: String,
}

impl Default for TestContainerConfig {
    fn default() -> Self {
        Self {
            postgres_image: "postgres:15-alpine".to_string(),
            redis_image: "redis:7-alpine".to_string(),
            kafka_image: "confluentinc/cp-kafka:7.5.0".to_string(),
            zookeeper_image: "confluentinc/cp-zookeeper:7.5.0".to_string(),
        }
    }
}

/// Test database configuration
#[derive(Debug, Clone)]
pub struct TestDatabaseConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub database: String,
}

impl TestDatabaseConfig {
    pub fn connection_string(&self) -> String {
        format!(
            "postgres://{}:{}@{}:{}/{}",
            self.user, self.password, self.host, self.port, self.database
        )
    }
}

/// Test Redis configuration
#[derive(Debug, Clone)]
pub struct TestRedisConfig {
    pub host: String,
    pub port: u16,
}

impl TestRedisConfig {
    pub fn connection_string(&self) -> String {
        format!("redis://{}:{}", self.host, self.port)
    }
}

/// Test Kafka configuration
#[derive(Debug, Clone)]
pub struct TestKafkaConfig {
    pub bootstrap_servers: String,
}

/// Integration test harness
///
/// Example usage:
/// ```ignore
/// #[tokio::test]
/// async fn test_user_creation() {
///     let harness = TestHarness::new().await;
///
///     // Run migrations
///     harness.migrate().await;
///
///     // Get database pool
///     let pool = harness.db_pool().await;
///
///     // Run your test
///     let result = create_user(&pool, "test@example.com").await;
///     assert!(result.is_ok());
/// }
/// ```
pub struct TestHarness {
    pub db_config: TestDatabaseConfig,
    pub redis_config: Option<TestRedisConfig>,
    pub kafka_config: Option<TestKafkaConfig>,
    migrations_path: String,
}

impl TestHarness {
    /// Create a new test harness with default configuration
    ///
    /// In a real implementation, this would spin up testcontainers.
    /// For now, it uses environment variables or defaults for local testing.
    pub async fn new() -> Self {
        let db_config = TestDatabaseConfig {
            host: std::env::var("TEST_DB_HOST").unwrap_or_else(|_| "localhost".to_string()),
            port: std::env::var("TEST_DB_PORT")
                .ok()
                .and_then(|p| p.parse().ok())
                .unwrap_or(5432),
            user: std::env::var("TEST_DB_USER").unwrap_or_else(|_| "postgres".to_string()),
            password: std::env::var("TEST_DB_PASSWORD").unwrap_or_else(|_| "postgres".to_string()),
            database: std::env::var("TEST_DB_NAME").unwrap_or_else(|_| "nava_test".to_string()),
        };

        let redis_config = std::env::var("TEST_REDIS_HOST").ok().map(|host| {
            TestRedisConfig {
                host,
                port: std::env::var("TEST_REDIS_PORT")
                    .ok()
                    .and_then(|p| p.parse().ok())
                    .unwrap_or(6379),
            }
        });

        let kafka_config = std::env::var("TEST_KAFKA_BOOTSTRAP").ok().map(|servers| {
            TestKafkaConfig {
                bootstrap_servers: servers,
            }
        });

        Self {
            db_config,
            redis_config,
            kafka_config,
            migrations_path: "migrations".to_string(),
        }
    }

    /// Set custom migrations path
    pub fn with_migrations_path(mut self, path: &str) -> Self {
        self.migrations_path = path.to_string();
        self
    }

    /// Get database connection pool
    pub async fn db_pool(&self) -> Result<sqlx::PgPool, sqlx::Error> {
        sqlx::PgPool::connect(&self.db_config.connection_string()).await
    }

    /// Run database migrations
    pub async fn migrate(&self) -> Result<(), sqlx::migrate::MigrateError> {
        let pool = self.db_pool().await.expect("Failed to connect to test database");
        sqlx::migrate!().run(&pool).await
    }

    /// Clean database (truncate all tables)
    pub async fn clean_db(&self) -> Result<(), sqlx::Error> {
        let pool = self.db_pool().await?;

        // Get all tables
        let tables: Vec<(String,)> = sqlx::query_as(
            r#"
            SELECT tablename
            FROM pg_tables
            WHERE schemaname = 'public'
            AND tablename != '_sqlx_migrations'
            "#,
        )
        .fetch_all(&pool)
        .await?;

        // Truncate all tables
        for (table,) in tables {
            sqlx::query(&format!("TRUNCATE TABLE {} CASCADE", table))
                .execute(&pool)
                .await?;
        }

        Ok(())
    }

    /// Get Redis connection
    pub async fn redis_client(&self) -> Option<redis::Client> {
        self.redis_config.as_ref().map(|config| {
            redis::Client::open(config.connection_string()).expect("Failed to connect to Redis")
        })
    }
}

/// Test fixtures for common test data
pub mod fixtures {
    use serde_json::json;

    /// Create a test user fixture
    pub fn user_fixture(email: &str, name: &str) -> serde_json::Value {
        json!({
            "email": email,
            "name": name,
            "phone": "+1234567890",
            "date_of_birth": "1990-01-01",
            "gender": "male",
            "location": {
                "city": "San Francisco",
                "state": "CA",
                "country": "US"
            }
        })
    }

    /// Create a test profile fixture
    pub fn profile_fixture(user_id: i32) -> serde_json::Value {
        json!({
            "user_id": user_id,
            "bio": "Test bio",
            "occupation": "Software Engineer",
            "education": "Bachelor's",
            "height_cm": 175,
            "interests": ["music", "travel", "cooking"]
        })
    }

    /// Create a test subscription fixture
    pub fn subscription_fixture(user_id: i32, plan: &str) -> serde_json::Value {
        json!({
            "user_id": user_id,
            "plan": plan,
            "status": "active",
            "starts_at": chrono::Utc::now().to_rfc3339(),
            "expires_at": (chrono::Utc::now() + chrono::Duration::days(30)).to_rfc3339()
        })
    }
}

/// HTTP test client for integration tests
pub struct TestClient {
    client: reqwest::Client,
    base_url: String,
    auth_token: Option<String>,
}

impl TestClient {
    pub fn new(base_url: &str) -> Self {
        Self {
            client: reqwest::Client::new(),
            base_url: base_url.to_string(),
            auth_token: None,
        }
    }

    /// Set authentication token
    pub fn with_auth(mut self, token: &str) -> Self {
        self.auth_token = Some(token.to_string());
        self
    }

    /// Make a GET request
    pub async fn get(&self, path: &str) -> reqwest::Result<reqwest::Response> {
        let mut req = self.client.get(format!("{}{}", self.base_url, path));

        if let Some(token) = &self.auth_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        req.send().await
    }

    /// Make a POST request
    pub async fn post<T: serde::Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> reqwest::Result<reqwest::Response> {
        let mut req = self
            .client
            .post(format!("{}{}", self.base_url, path))
            .json(body);

        if let Some(token) = &self.auth_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        req.send().await
    }

    /// Make a PUT request
    pub async fn put<T: serde::Serialize>(
        &self,
        path: &str,
        body: &T,
    ) -> reqwest::Result<reqwest::Response> {
        let mut req = self
            .client
            .put(format!("{}{}", self.base_url, path))
            .json(body);

        if let Some(token) = &self.auth_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        req.send().await
    }

    /// Make a DELETE request
    pub async fn delete(&self, path: &str) -> reqwest::Result<reqwest::Response> {
        let mut req = self.client.delete(format!("{}{}", self.base_url, path));

        if let Some(token) = &self.auth_token {
            req = req.header("Authorization", format!("Bearer {}", token));
        }

        req.send().await
    }
}

/// Assertion helpers for tests
pub mod assertions {
    use axum::http::StatusCode;

    /// Assert response has expected status code
    pub fn assert_status(response: &reqwest::Response, expected: StatusCode) {
        assert_eq!(
            response.status().as_u16(),
            expected.as_u16(),
            "Expected status {}, got {}",
            expected,
            response.status()
        );
    }

    /// Assert response is successful (2xx)
    pub fn assert_success(response: &reqwest::Response) {
        assert!(
            response.status().is_success(),
            "Expected success status, got {}",
            response.status()
        );
    }

    /// Assert response is client error (4xx)
    pub fn assert_client_error(response: &reqwest::Response) {
        assert!(
            response.status().is_client_error(),
            "Expected client error, got {}",
            response.status()
        );
    }

    /// Assert response is server error (5xx)
    pub fn assert_server_error(response: &reqwest::Response) {
        assert!(
            response.status().is_server_error(),
            "Expected server error, got {}",
            response.status()
        );
    }
}

/// Docker compose helper for local testing
pub struct DockerComposeHelper {
    compose_file: String,
    project_name: String,
}

impl DockerComposeHelper {
    pub fn new(compose_file: &str, project_name: &str) -> Self {
        Self {
            compose_file: compose_file.to_string(),
            project_name: project_name.to_string(),
        }
    }

    /// Start services
    pub async fn up(&self) -> std::io::Result<()> {
        let output = tokio::process::Command::new("docker-compose")
            .args([
                "-f",
                &self.compose_file,
                "-p",
                &self.project_name,
                "up",
                "-d",
            ])
            .output()
            .await?;

        if !output.status.success() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        Ok(())
    }

    /// Stop and remove services
    pub async fn down(&self) -> std::io::Result<()> {
        let output = tokio::process::Command::new("docker-compose")
            .args([
                "-f",
                &self.compose_file,
                "-p",
                &self.project_name,
                "down",
                "-v",
            ])
            .output()
            .await?;

        if !output.status.success() {
            return Err(std::io::Error::new(
                std::io::ErrorKind::Other,
                String::from_utf8_lossy(&output.stderr).to_string(),
            ));
        }

        Ok(())
    }

    /// Wait for service to be healthy
    pub async fn wait_for_healthy(&self, service: &str, timeout_secs: u64) -> std::io::Result<()> {
        let start = std::time::Instant::now();
        let timeout = std::time::Duration::from_secs(timeout_secs);

        while start.elapsed() < timeout {
            let output = tokio::process::Command::new("docker-compose")
                .args([
                    "-f",
                    &self.compose_file,
                    "-p",
                    &self.project_name,
                    "ps",
                    service,
                ])
                .output()
                .await?;

            let stdout = String::from_utf8_lossy(&output.stdout);
            if stdout.contains("healthy") || stdout.contains("Up") {
                return Ok(());
            }

            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        }

        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("Service {} did not become healthy in {} seconds", service, timeout_secs),
        ))
    }
}
