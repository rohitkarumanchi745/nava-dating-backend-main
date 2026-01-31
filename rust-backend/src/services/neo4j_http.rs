// =============================================================================
// Neo4j HTTP Client - Workaround for Bolt Protocol Compatibility
// =============================================================================
// Uses Neo4j's HTTP API instead of Bolt for better compatibility with Neo4j 5.x
// =============================================================================

use reqwest::Client;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::sync::Arc;
use tracing::{debug, info, warn};

use crate::error::AppError;

pub type Result<T> = std::result::Result<T, AppError>;

/// Neo4j HTTP Client for executing Cypher queries
#[derive(Clone)]
pub struct Neo4jHttpClient {
    client: Client,
    base_url: String,
    auth: String, // Base64 encoded "user:password"
}

#[derive(Debug, Serialize)]
struct CypherRequest {
    statements: Vec<CypherStatement>,
}

#[derive(Debug, Serialize)]
struct CypherStatement {
    statement: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    parameters: Option<Value>,
}

#[derive(Debug, Deserialize)]
pub struct CypherResponse {
    pub results: Vec<CypherResult>,
    pub errors: Vec<CypherError>,
}

#[derive(Debug, Deserialize)]
pub struct CypherResult {
    pub columns: Vec<String>,
    pub data: Vec<CypherRow>,
}

#[derive(Debug, Deserialize)]
pub struct CypherRow {
    pub row: Vec<Value>,
    #[serde(default)]
    pub meta: Vec<Value>,
}

#[derive(Debug, Deserialize)]
pub struct CypherError {
    pub code: String,
    pub message: String,
}

impl Neo4jHttpClient {
    /// Create a new Neo4j HTTP client
    pub fn new(host: &str, port: u16, user: &str, password: &str) -> Self {
        use base64::{Engine as _, engine::general_purpose::STANDARD};
        let auth = STANDARD.encode(format!("{}:{}", user, password));

        Self {
            client: Client::new(),
            base_url: format!("http://{}:{}", host, port),
            auth,
        }
    }

    /// Create from environment variables
    pub fn from_env() -> Option<Self> {
        let uri = match std::env::var("NEO4J_URI") {
            Ok(u) => u,
            Err(_) => {
                warn!("NEO4J_URI not set");
                return None;
            }
        };
        let user = std::env::var("NEO4J_USER").unwrap_or_else(|_| "neo4j".to_string());
        let password = match std::env::var("NEO4J_PASSWORD") {
            Ok(p) => p,
            Err(_) => {
                warn!("NEO4J_PASSWORD not set");
                return None;
            }
        };

        // Parse URI to get host and port
        // Expected format: bolt://host:port or neo4j://host:port
        let uri_clean = uri.replace("bolt://", "").replace("neo4j://", "");
        let parts: Vec<&str> = uri_clean.split(':').collect();
        let host = parts.get(0).unwrap_or(&"127.0.0.1").to_string();

        info!("Creating Neo4j HTTP client for host: {} (from URI: {})", host, uri);

        // HTTP API is typically on port 7474
        Some(Self::new(&host, 7474, &user, &password))
    }

    /// Execute a Cypher query
    pub async fn execute(&self, query: &str, params: Option<Value>) -> Result<CypherResponse> {
        let url = format!("{}/db/neo4j/tx/commit", self.base_url);

        let request = CypherRequest {
            statements: vec![CypherStatement {
                statement: query.to_string(),
                parameters: params,
            }],
        };

        debug!("Executing Neo4j query via HTTP: {}", query);

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Basic {}", self.auth))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Neo4j HTTP request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(AppError::Internal(format!(
                "Neo4j HTTP error {}: {}", status, body
            )));
        }

        let result: CypherResponse = response
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to parse Neo4j response: {}", e)))?;

        if !result.errors.is_empty() {
            let error = &result.errors[0];
            return Err(AppError::Internal(format!(
                "Neo4j error {}: {}", error.code, error.message
            )));
        }

        Ok(result)
    }

    /// Execute multiple statements in a single transaction
    pub async fn execute_batch(&self, queries: Vec<(&str, Option<Value>)>) -> Result<CypherResponse> {
        let url = format!("{}/db/neo4j/tx/commit", self.base_url);

        let statements: Vec<CypherStatement> = queries
            .into_iter()
            .map(|(q, p)| CypherStatement {
                statement: q.to_string(),
                parameters: p,
            })
            .collect();

        let request = CypherRequest { statements };

        let response = self.client
            .post(&url)
            .header("Authorization", format!("Basic {}", self.auth))
            .header("Content-Type", "application/json")
            .json(&request)
            .send()
            .await
            .map_err(|e| AppError::Internal(format!("Neo4j HTTP request failed: {}", e)))?;

        let result: CypherResponse = response
            .json()
            .await
            .map_err(|e| AppError::Internal(format!("Failed to parse Neo4j response: {}", e)))?;

        if !result.errors.is_empty() {
            let error = &result.errors[0];
            warn!("Neo4j batch error: {} - {}", error.code, error.message);
        }

        Ok(result)
    }

    /// Health check
    pub async fn health_check(&self) -> bool {
        match self.execute("RETURN 1 as health", None).await {
            Ok(response) => response.errors.is_empty(),
            Err(e) => {
                warn!("Neo4j health check failed: {}", e);
                false
            }
        }
    }
}

/// Wrapper for Arc<Neo4jHttpClient>
#[derive(Clone)]
pub struct Neo4jHttp(pub Arc<Neo4jHttpClient>);

impl Neo4jHttp {
    pub fn new(client: Neo4jHttpClient) -> Self {
        Self(Arc::new(client))
    }

    pub fn from_env() -> Option<Self> {
        Neo4jHttpClient::from_env().map(|c| Self::new(c))
    }

    pub async fn execute(&self, query: &str, params: Option<Value>) -> Result<CypherResponse> {
        self.0.execute(query, params).await
    }

    pub async fn health_check(&self) -> bool {
        self.0.health_check().await
    }
}
