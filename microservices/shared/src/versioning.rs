//! API versioning strategy
//!
//! Supports multiple API versioning strategies:
//! - URL path versioning (/v1/users, /v2/users)
//! - Header versioning (X-API-Version: 1)
//! - Accept header versioning (Accept: application/vnd.nava.v1+json)

use axum::{
    extract::Request,
    http::{header, HeaderMap, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// API version representation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ApiVersion {
    pub major: u32,
    pub minor: u32,
}

impl ApiVersion {
    pub const V1: ApiVersion = ApiVersion { major: 1, minor: 0 };
    pub const V2: ApiVersion = ApiVersion { major: 2, minor: 0 };

    pub fn new(major: u32, minor: u32) -> Self {
        Self { major, minor }
    }

    /// Parse version from string like "1", "1.0", "v1", "v1.0"
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('v').trim_start_matches('V');

        if let Some((major, minor)) = s.split_once('.') {
            Some(ApiVersion {
                major: major.parse().ok()?,
                minor: minor.parse().ok()?,
            })
        } else {
            Some(ApiVersion {
                major: s.parse().ok()?,
                minor: 0,
            })
        }
    }

    /// Check if this version is compatible with another
    pub fn is_compatible_with(&self, other: &ApiVersion) -> bool {
        self.major == other.major
    }
}

impl std::fmt::Display for ApiVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}.{}", self.major, self.minor)
    }
}

/// Versioning strategy configuration
#[derive(Debug, Clone)]
pub struct VersioningConfig {
    /// Default version if not specified
    pub default_version: ApiVersion,
    /// Supported versions
    pub supported_versions: Vec<ApiVersion>,
    /// Strategy to use
    pub strategy: VersioningStrategy,
    /// Header name for header-based versioning
    pub version_header: String,
}

impl Default for VersioningConfig {
    fn default() -> Self {
        Self {
            default_version: ApiVersion::V1,
            supported_versions: vec![ApiVersion::V1, ApiVersion::V2],
            strategy: VersioningStrategy::Header,
            version_header: "X-API-Version".to_string(),
        }
    }
}

/// Versioning strategy
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersioningStrategy {
    /// Version in URL path: /v1/users
    Path,
    /// Version in custom header: X-API-Version: 1
    Header,
    /// Version in Accept header: Accept: application/vnd.nava.v1+json
    Accept,
}

/// Version extractor for different strategies
pub struct VersionExtractor {
    config: VersioningConfig,
}

impl VersionExtractor {
    pub fn new(config: VersioningConfig) -> Self {
        Self { config }
    }

    /// Extract version from request based on configured strategy
    pub fn extract(&self, path: &str, headers: &HeaderMap) -> ApiVersion {
        match self.config.strategy {
            VersioningStrategy::Path => self.extract_from_path(path),
            VersioningStrategy::Header => self.extract_from_header(headers),
            VersioningStrategy::Accept => self.extract_from_accept(headers),
        }
        .unwrap_or(self.config.default_version)
    }

    fn extract_from_path(&self, path: &str) -> Option<ApiVersion> {
        // Match /v1/, /v2/, etc.
        let segments: Vec<&str> = path.split('/').collect();
        for segment in segments {
            if segment.starts_with('v') || segment.starts_with('V') {
                if let Some(version) = ApiVersion::parse(segment) {
                    return Some(version);
                }
            }
        }
        None
    }

    fn extract_from_header(&self, headers: &HeaderMap) -> Option<ApiVersion> {
        headers
            .get(&self.config.version_header)
            .and_then(|v| v.to_str().ok())
            .and_then(ApiVersion::parse)
    }

    fn extract_from_accept(&self, headers: &HeaderMap) -> Option<ApiVersion> {
        // Parse: application/vnd.nava.v1+json
        headers
            .get(header::ACCEPT)
            .and_then(|v| v.to_str().ok())
            .and_then(|accept| {
                if let Some(start) = accept.find("vnd.nava.v") {
                    let rest = &accept[start + 10..]; // after "vnd.nava.v"
                    let end = rest.find('+').unwrap_or(rest.len());
                    ApiVersion::parse(&rest[..end])
                } else {
                    None
                }
            })
    }

    /// Check if a version is supported
    pub fn is_supported(&self, version: &ApiVersion) -> bool {
        self.config
            .supported_versions
            .iter()
            .any(|v| v.major == version.major)
    }
}

/// Version-aware router
pub struct VersionedRouter<S = ()> {
    config: VersioningConfig,
    routes: HashMap<ApiVersion, axum::Router<S>>,
}

impl<S: Clone + Send + Sync + 'static> VersionedRouter<S> {
    pub fn new(config: VersioningConfig) -> Self {
        Self {
            config,
            routes: HashMap::new(),
        }
    }

    /// Add routes for a specific version
    pub fn version(mut self, version: ApiVersion, router: axum::Router<S>) -> Self {
        self.routes.insert(version, router);
        self
    }

    /// Build the final router
    pub fn build(self) -> axum::Router<S> {
        let mut router = axum::Router::new();

        for (version, version_router) in self.routes {
            // Nest under /v{major}
            let prefix = format!("/v{}", version.major);
            router = router.nest(&prefix, version_router);
        }

        router
    }
}

/// Middleware to extract and validate API version
pub async fn version_middleware(
    config: std::sync::Arc<VersioningConfig>,
    mut request: Request,
    next: Next,
) -> Response {
    let extractor = VersionExtractor::new((*config).clone());

    let path = request.uri().path();
    let headers = request.headers();

    let version = extractor.extract(path, headers);

    if !extractor.is_supported(&version) {
        return VersionError::UnsupportedVersion {
            requested: version,
            supported: config.supported_versions.clone(),
        }
        .into_response();
    }

    // Store version in request extensions
    request.extensions_mut().insert(version);

    next.run(request).await
}

/// Version-related errors
#[derive(Debug, thiserror::Error)]
pub enum VersionError {
    #[error("Unsupported API version {requested}. Supported versions: {supported:?}")]
    UnsupportedVersion {
        requested: ApiVersion,
        supported: Vec<ApiVersion>,
    },

    #[error("Invalid version format")]
    InvalidFormat,
}

impl IntoResponse for VersionError {
    fn into_response(self) -> Response {
        let (status, message) = match &self {
            VersionError::UnsupportedVersion { .. } => (StatusCode::NOT_FOUND, self.to_string()),
            VersionError::InvalidFormat => (StatusCode::BAD_REQUEST, self.to_string()),
        };

        let body = serde_json::json!({
            "error": "version_error",
            "message": message,
        });

        (status, axum::Json(body)).into_response()
    }
}

/// Deprecation notice for sunset versions
#[derive(Debug, Clone)]
pub struct DeprecationNotice {
    pub version: ApiVersion,
    pub sunset_date: chrono::NaiveDate,
    pub migration_guide_url: Option<String>,
}

impl DeprecationNotice {
    pub fn headers(&self) -> HeaderMap {
        let mut headers = HeaderMap::new();

        if let Ok(sunset) = axum::http::HeaderValue::from_str(&self.sunset_date.to_string()) {
            headers.insert("Sunset", sunset);
        }

        headers.insert(
            "Deprecation",
            axum::http::HeaderValue::from_static("true"),
        );

        if let Some(url) = &self.migration_guide_url {
            if let Ok(link) = axum::http::HeaderValue::from_str(&format!("<{}>; rel=\"successor-version\"", url)) {
                headers.insert("Link", link);
            }
        }

        headers
    }
}

/// Helper to add deprecation headers to responses
pub fn add_deprecation_headers(response: &mut Response, notice: &DeprecationNotice) {
    let headers = notice.headers();
    for (key, value) in headers.iter() {
        response.headers_mut().insert(key.clone(), value.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing() {
        assert_eq!(ApiVersion::parse("1"), Some(ApiVersion::new(1, 0)));
        assert_eq!(ApiVersion::parse("v1"), Some(ApiVersion::new(1, 0)));
        assert_eq!(ApiVersion::parse("v2.1"), Some(ApiVersion::new(2, 1)));
        assert_eq!(ApiVersion::parse("V1.0"), Some(ApiVersion::new(1, 0)));
    }

    #[test]
    fn test_version_compatibility() {
        let v1 = ApiVersion::new(1, 0);
        let v1_1 = ApiVersion::new(1, 1);
        let v2 = ApiVersion::new(2, 0);

        assert!(v1.is_compatible_with(&v1_1));
        assert!(!v1.is_compatible_with(&v2));
    }

    #[test]
    fn test_path_extraction() {
        let config = VersioningConfig {
            strategy: VersioningStrategy::Path,
            ..Default::default()
        };
        let extractor = VersionExtractor::new(config);

        let headers = HeaderMap::new();
        assert_eq!(
            extractor.extract("/v1/users", &headers),
            ApiVersion::new(1, 0)
        );
        assert_eq!(
            extractor.extract("/v2/users/123", &headers),
            ApiVersion::new(2, 0)
        );
    }
}
