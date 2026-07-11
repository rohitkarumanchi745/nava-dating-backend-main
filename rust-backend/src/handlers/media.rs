//! Media serving proxy for S3-backed storage.
//!
//! In local mode `/uploads` is served by tower-http's ServeDir straight from
//! disk (ranges, ETags for free). In S3 mode this handler streams objects from
//! the store (MinIO/R2/S3) through the API, so clients keep resolving the same
//! relative `/uploads/...` paths the DB has always stored — no iOS changes,
//! no public object-store exposure.

use axum::{
    body::Body,
    extract::{Path as AxumPath, State},
    http::{header, HeaderMap, StatusCode},
    response::{IntoResponse, Response},
};
use tracing::warn;

use crate::state::AppState;
use crate::storage::guess_content_type;

/// GET /uploads/{*key} — stream an object from the S3-compatible store.
/// The Range header is passed through so AVPlayer can seek within videos.
pub async fn serve_upload(
    State(state): State<AppState>,
    AxumPath(key): AxumPath<String>,
    headers: HeaderMap,
) -> Response {
    // Keys are served verbatim from the store; reject traversal shapes.
    if key.contains("..") || key.starts_with('/') || key.is_empty() {
        return StatusCode::NOT_FOUND.into_response();
    }

    let range = headers
        .get(header::RANGE)
        .and_then(|value| value.to_str().ok());

    match state.storage.get_object(&key, range).await {
        Ok(Some(obj)) => {
            let status = if obj.partial {
                StatusCode::PARTIAL_CONTENT
            } else {
                StatusCode::OK
            };
            let content_type = obj
                .content_type
                .clone()
                .unwrap_or_else(|| guess_content_type(&key).to_string());

            let mut builder = Response::builder()
                .status(status)
                .header(header::CONTENT_TYPE, content_type)
                .header(header::ACCEPT_RANGES, "bytes")
                // Media files are immutable (uuid'd filenames) — cache hard.
                .header(header::CACHE_CONTROL, "public, max-age=86400");
            if let Some(content_range) = &obj.content_range {
                builder = builder.header(header::CONTENT_RANGE, content_range);
            }
            if let Some(len) = obj.content_length {
                builder = builder.header(header::CONTENT_LENGTH, len);
            }

            builder
                .body(Body::from_stream(obj.response.bytes_stream()))
                .unwrap_or_else(|_| StatusCode::INTERNAL_SERVER_ERROR.into_response())
        }
        Ok(None) => StatusCode::NOT_FOUND.into_response(),
        Err(e) => {
            warn!("serve_upload {key}: {e}");
            StatusCode::BAD_GATEWAY.into_response()
        }
    }
}
