use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use serde_json::json;
use std::fmt;

#[derive(Debug, Clone)]
pub struct AppError {
    pub status: StatusCode,
    message: String,
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for AppError {}

impl AppError {
    pub fn new(status: StatusCode, message: impl Into<String>) -> Self {
        Self {
            status,
            message: message.into(),
        }
    }

    pub fn bad_request(message: impl Into<String>) -> Self {
        Self::new(StatusCode::BAD_REQUEST, message)
    }

    pub fn unauthorized(message: impl Into<String>) -> Self {
        Self::new(StatusCode::UNAUTHORIZED, message)
    }

    #[allow(dead_code)]
    pub fn forbidden(message: impl Into<String>) -> Self {
        Self::new(StatusCode::FORBIDDEN, message)
    }

    pub fn not_found(message: impl Into<String>) -> Self {
        Self::new(StatusCode::NOT_FOUND, message)
    }

    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(StatusCode::INTERNAL_SERVER_ERROR, message)
    }

    pub fn service_unavailable(message: impl Into<String>) -> Self {
        Self::new(StatusCode::SERVICE_UNAVAILABLE, message)
    }

    pub fn too_many_requests(message: impl Into<String>) -> Self {
        Self::new(StatusCode::TOO_MANY_REQUESTS, message)
    }
}

/// Structured moderation rejection response.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ModerationRejection {
    pub detail: String,
    pub trace_id: String,
    pub decision_id: Option<i64>,
    pub reason_code: String,
    pub can_appeal: bool,
}

impl ModerationRejection {
    pub fn into_response(self) -> axum::response::Response {
        let body = axum::Json(self);
        (StatusCode::UNPROCESSABLE_ENTITY, body).into_response()
    }
}

impl IntoResponse for AppError {
    fn into_response(self) -> Response {
        let body = Json(json!({ "detail": self.message }));
        (self.status, body).into_response()
    }
}

impl From<sqlx::Error> for AppError {
    fn from(err: sqlx::Error) -> Self {
        // Log the actual error for debugging (full details in server logs)
        tracing::error!("Database error: {:?}", err);

        // In production, return generic message to avoid leaking schema details
        // In development, include error details for debugging
        let is_production = std::env::var("ENVIRONMENT")
            .map(|e| e == "production")
            .unwrap_or(false);

        if is_production {
            // Generic message for production - no internal details exposed
            AppError::internal("A database error occurred. Please try again later.")
        } else {
            // Detailed message for development
            AppError::internal(format!("Database error: {}", err))
        }
    }
}

// Type alias for Result with AppError
pub type Result<T> = std::result::Result<T, AppError>;

// Error variants for graph service
#[derive(Debug)]
pub enum GraphError {
    Database(String),
    Internal(String),
    NotFound(String),
    BadRequest(String),
}

impl From<GraphError> for AppError {
    fn from(err: GraphError) -> Self {
        match err {
            GraphError::Database(msg) => AppError::internal(format!("Database error: {}", msg)),
            GraphError::Internal(msg) => AppError::internal(msg),
            GraphError::NotFound(msg) => AppError::not_found(msg),
            GraphError::BadRequest(msg) => AppError::bad_request(msg),
        }
    }
}

// Error type wrappers used by graph_service
impl AppError {
    /// Create a database error
    pub fn Database(msg: String) -> Self {
        AppError::internal(format!("Database error: {}", msg))
    }

    /// Create an internal error (alias)
    pub fn Internal(msg: String) -> Self {
        AppError::internal(msg)
    }

    /// Create a not found error (alias)
    pub fn NotFound(msg: String) -> Self {
        AppError::not_found(msg)
    }

    /// Create a bad request error (alias)
    pub fn BadRequest(msg: String) -> Self {
        AppError::bad_request(msg)
    }
}
