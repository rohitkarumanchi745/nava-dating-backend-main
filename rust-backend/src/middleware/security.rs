// =============================================================================
// NAVA Platform - Security Middleware
// =============================================================================
// Production security headers and request sanitization
// =============================================================================

use axum::{
    body::Body,
    extract::Request,
    http::{header, HeaderValue, StatusCode},
    middleware::Next,
    response::{IntoResponse, Response},
};
use tracing::warn;

// -----------------------------------------------------------------------------
// Security Headers Middleware
// -----------------------------------------------------------------------------

/// Add security headers to all responses
pub async fn security_headers_middleware(
    request: Request<Body>,
    next: Next,
) -> Response {
    let mut response = next.run(request).await;
    let headers = response.headers_mut();

    // Prevent clickjacking
    headers.insert(
        header::X_FRAME_OPTIONS,
        HeaderValue::from_static("DENY"),
    );

    // Prevent MIME type sniffing
    headers.insert(
        header::X_CONTENT_TYPE_OPTIONS,
        HeaderValue::from_static("nosniff"),
    );

    // XSS Protection (legacy but still useful)
    headers.insert(
        "X-XSS-Protection",
        HeaderValue::from_static("1; mode=block"),
    );

    // Referrer Policy
    headers.insert(
        header::REFERRER_POLICY,
        HeaderValue::from_static("strict-origin-when-cross-origin"),
    );

    // Content Security Policy (API-focused)
    headers.insert(
        header::CONTENT_SECURITY_POLICY,
        HeaderValue::from_static("default-src 'none'; frame-ancestors 'none'"),
    );

    // Strict Transport Security (HTTPS only)
    headers.insert(
        header::STRICT_TRANSPORT_SECURITY,
        HeaderValue::from_static("max-age=31536000; includeSubDomains; preload"),
    );

    // Permissions Policy (disable dangerous features)
    headers.insert(
        "Permissions-Policy",
        HeaderValue::from_static("geolocation=(), microphone=(), camera=()"),
    );

    // Cache control for API responses
    if !headers.contains_key(header::CACHE_CONTROL) {
        headers.insert(
            header::CACHE_CONTROL,
            HeaderValue::from_static("no-store, no-cache, must-revalidate, private"),
        );
    }

    response
}

// -----------------------------------------------------------------------------
// Request Size Limit Middleware
// -----------------------------------------------------------------------------

const MAX_BODY_SIZE: u64 = 10 * 1024 * 1024; // 10MB default

/// Reject requests with bodies that are too large
pub async fn body_size_limit_middleware(
    request: Request<Body>,
    next: Next,
) -> Result<Response, Response> {
    if let Some(content_length) = request.headers()
        .get(header::CONTENT_LENGTH)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse::<u64>().ok())
    {
        if content_length > MAX_BODY_SIZE {
            warn!("Request body too large: {} bytes", content_length);
            return Err((
                StatusCode::PAYLOAD_TOO_LARGE,
                "Request body too large",
            ).into_response());
        }
    }

    Ok(next.run(request).await)
}

// -----------------------------------------------------------------------------
// Request Sanitization
// -----------------------------------------------------------------------------

/// Sanitize user input to prevent injection attacks
pub fn sanitize_input(input: &str) -> String {
    input
        .chars()
        .filter(|c| !c.is_control() || *c == '\n' || *c == '\t')
        .take(10000) // Limit length
        .collect()
}

/// Sanitize for SQL-safe string (basic)
pub fn sanitize_for_display(input: &str) -> String {
    input
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&#x27;")
        .chars()
        .take(1000)
        .collect()
}

/// Validate phone number format
pub fn validate_phone(phone: &str) -> bool {
    let clean: String = phone.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect();
    clean.len() >= 10 && clean.len() <= 15 && clean.starts_with('+')
}

/// Validate email format (basic)
pub fn validate_email(email: &str) -> bool {
    let parts: Vec<&str> = email.split('@').collect();
    parts.len() == 2
        && !parts[0].is_empty()
        && parts[1].contains('.')
        && email.len() <= 254
}

/// Validate UUID format
pub fn validate_uuid(uuid: &str) -> bool {
    uuid::Uuid::parse_str(uuid).is_ok()
}

// -----------------------------------------------------------------------------
// IP-based Security
// -----------------------------------------------------------------------------

/// Extract real client IP from request headers
pub fn extract_client_ip(headers: &axum::http::HeaderMap) -> String {
    // Check X-Forwarded-For (load balancer)
    if let Some(xff) = headers.get("x-forwarded-for") {
        if let Ok(xff_str) = xff.to_str() {
            if let Some(ip) = xff_str.split(',').next() {
                return ip.trim().to_string();
            }
        }
    }

    // Check X-Real-IP (nginx)
    if let Some(real_ip) = headers.get("x-real-ip") {
        if let Ok(ip) = real_ip.to_str() {
            return ip.trim().to_string();
        }
    }

    // Check CF-Connecting-IP (Cloudflare)
    if let Some(cf_ip) = headers.get("cf-connecting-ip") {
        if let Ok(ip) = cf_ip.to_str() {
            return ip.trim().to_string();
        }
    }

    "unknown".to_string()
}

/// Check if IP is in a private range (for internal requests)
pub fn is_private_ip(ip: &str) -> bool {
    ip.starts_with("10.")
        || ip.starts_with("172.16.")
        || ip.starts_with("172.17.")
        || ip.starts_with("172.18.")
        || ip.starts_with("172.19.")
        || ip.starts_with("172.20.")
        || ip.starts_with("172.21.")
        || ip.starts_with("172.22.")
        || ip.starts_with("172.23.")
        || ip.starts_with("172.24.")
        || ip.starts_with("172.25.")
        || ip.starts_with("172.26.")
        || ip.starts_with("172.27.")
        || ip.starts_with("172.28.")
        || ip.starts_with("172.29.")
        || ip.starts_with("172.30.")
        || ip.starts_with("172.31.")
        || ip.starts_with("192.168.")
        || ip == "127.0.0.1"
        || ip == "localhost"
}

// -----------------------------------------------------------------------------
// Sensitive Data Redaction
// -----------------------------------------------------------------------------

/// Redact sensitive data from logs
pub fn redact_sensitive(value: &str) -> String {
    if value.len() <= 4 {
        return "***".to_string();
    }
    format!("{}***{}", &value[..2], &value[value.len()-2..])
}

/// Redact phone number for logging
pub fn redact_phone(phone: &str) -> String {
    if phone.len() <= 6 {
        return "***".to_string();
    }
    format!("{}***{}", &phone[..4], &phone[phone.len()-2..])
}

/// Redact email for logging
pub fn redact_email(email: &str) -> String {
    if let Some(at_pos) = email.find('@') {
        let local = &email[..at_pos];
        let domain = &email[at_pos..];
        if local.len() <= 2 {
            return format!("***{}", domain);
        }
        return format!("{}***{}", &local[..2], domain);
    }
    "***".to_string()
}

// -----------------------------------------------------------------------------
// Tests
// -----------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_validate_phone() {
        assert!(validate_phone("+12025551234"));
        assert!(validate_phone("+919876543210"));
        assert!(!validate_phone("12025551234")); // Missing +
        assert!(!validate_phone("+123")); // Too short
    }

    #[test]
    fn test_validate_email() {
        assert!(validate_email("test@example.com"));
        assert!(validate_email("user.name@domain.co.uk"));
        assert!(!validate_email("invalid"));
        assert!(!validate_email("@domain.com"));
        assert!(!validate_email("user@"));
    }

    #[test]
    fn test_sanitize_input() {
        assert_eq!(sanitize_input("hello\nworld"), "hello\nworld");
        assert_eq!(sanitize_input("test\x00bad"), "testbad");
    }

    #[test]
    fn test_redact_phone() {
        assert_eq!(redact_phone("+919876543210"), "+919***10");
    }

    #[test]
    fn test_redact_email() {
        assert_eq!(redact_email("test@example.com"), "te***@example.com");
    }

    #[test]
    fn test_is_private_ip() {
        assert!(is_private_ip("192.168.1.1"));
        assert!(is_private_ip("10.0.0.1"));
        assert!(is_private_ip("127.0.0.1"));
        assert!(!is_private_ip("8.8.8.8"));
    }
}
