//! Fuzz Testing Targets for NAVA Platform
//!
//! Uses cargo-fuzz to send random/invalid data to find crashes and vulnerabilities.
//!
//! Setup:
//!   cargo install cargo-fuzz
//!
//! Run:
//!   cd rust-backend
//!   cargo +nightly fuzz run fuzz_json_parser
//!   cargo +nightly fuzz run fuzz_phone_validator
//!   cargo +nightly fuzz run fuzz_jwt_decoder
//!
//! List targets:
//!   cargo +nightly fuzz list

// =============================================================================
// Note: These are fuzz target templates. To use them:
// 1. Create a fuzz/ directory in rust-backend
// 2. Run: cargo fuzz init
// 3. Copy these targets to fuzz/fuzz_targets/
// =============================================================================

/// Fuzz target: JSON Parser
/// Tests JSON parsing with random/malformed input
pub mod fuzz_json_parser {
    use serde_json::Value;

    pub fn fuzz_target(data: &[u8]) {
        // Try to parse random bytes as JSON
        let _ = serde_json::from_slice::<Value>(data);

        // Try parsing as specific types
        if let Ok(s) = std::str::from_utf8(data) {
            let _ = serde_json::from_str::<Value>(s);
        }
    }
}

/// Fuzz target: Phone Number Validator
/// Tests phone validation with random input
pub mod fuzz_phone_validator {
    pub fn validate_phone(phone: &str) -> bool {
        if phone.is_empty() || phone.len() > 20 {
            return false;
        }

        let cleaned: String = phone
            .chars()
            .filter(|c| c.is_ascii_digit() || *c == '+')
            .collect();

        if cleaned.is_empty() {
            return false;
        }

        // Must start with + or digit
        let first = cleaned.chars().next().unwrap();
        if first != '+' && !first.is_ascii_digit() {
            return false;
        }

        // Length check (including country code)
        let digits: String = cleaned.chars().filter(|c| c.is_ascii_digit()).collect();
        digits.len() >= 10 && digits.len() <= 15
    }

    pub fn fuzz_target(data: &[u8]) {
        if let Ok(s) = std::str::from_utf8(data) {
            let _ = validate_phone(s);
        }
    }
}

/// Fuzz target: Email Validator
pub mod fuzz_email_validator {
    pub fn validate_email(email: &str) -> bool {
        if email.is_empty() || email.len() > 254 {
            return false;
        }

        // Basic checks
        let at_pos = match email.find('@') {
            Some(pos) => pos,
            None => return false,
        };

        let local = &email[..at_pos];
        let domain = &email[at_pos + 1..];

        // Local part checks
        if local.is_empty() || local.len() > 64 {
            return false;
        }

        // Domain checks
        if domain.is_empty() || !domain.contains('.') {
            return false;
        }

        // No consecutive dots
        if email.contains("..") {
            return false;
        }

        true
    }

    pub fn fuzz_target(data: &[u8]) {
        if let Ok(s) = std::str::from_utf8(data) {
            let _ = validate_email(s);
        }
    }
}

/// Fuzz target: JWT Decoder
/// Tests JWT parsing without signature verification
pub mod fuzz_jwt_decoder {
    use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};

    #[derive(Debug)]
    pub struct JwtParts {
        pub header: String,
        pub payload: String,
        pub signature: String,
    }

    pub fn decode_jwt_parts(token: &str) -> Option<JwtParts> {
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return None;
        }

        let header = URL_SAFE_NO_PAD.decode(parts[0]).ok()?;
        let payload = URL_SAFE_NO_PAD.decode(parts[1]).ok()?;

        Some(JwtParts {
            header: String::from_utf8_lossy(&header).to_string(),
            payload: String::from_utf8_lossy(&payload).to_string(),
            signature: parts[2].to_string(),
        })
    }

    pub fn fuzz_target(data: &[u8]) {
        if let Ok(s) = std::str::from_utf8(data) {
            let _ = decode_jwt_parts(s);
        }
    }
}

/// Fuzz target: URL Parser
pub mod fuzz_url_parser {
    pub fn parse_path(path: &str) -> Vec<&str> {
        path.split('/')
            .filter(|s| !s.is_empty())
            .collect()
    }

    pub fn extract_user_id(path: &str) -> Option<i32> {
        let parts = parse_path(path);
        for (i, part) in parts.iter().enumerate() {
            if *part == "users" || *part == "user" {
                if let Some(id_str) = parts.get(i + 1) {
                    return id_str.parse().ok();
                }
            }
        }
        None
    }

    pub fn fuzz_target(data: &[u8]) {
        if let Ok(s) = std::str::from_utf8(data) {
            let _ = parse_path(s);
            let _ = extract_user_id(s);
        }
    }
}

/// Fuzz target: Date Parser
pub mod fuzz_date_parser {
    pub fn parse_date(input: &str) -> Option<(i32, u32, u32)> {
        // Try YYYY-MM-DD format
        let parts: Vec<&str> = input.split('-').collect();
        if parts.len() != 3 {
            return None;
        }

        let year: i32 = parts[0].parse().ok()?;
        let month: u32 = parts[1].parse().ok()?;
        let day: u32 = parts[2].parse().ok()?;

        // Basic validation
        if month < 1 || month > 12 {
            return None;
        }
        if day < 1 || day > 31 {
            return None;
        }

        Some((year, month, day))
    }

    pub fn fuzz_target(data: &[u8]) {
        if let Ok(s) = std::str::from_utf8(data) {
            let _ = parse_date(s);
        }
    }
}

/// Fuzz target: Input Sanitizer
pub mod fuzz_input_sanitizer {
    pub fn sanitize(input: &str) -> String {
        input
            .chars()
            .filter(|c| {
                // Remove potentially dangerous characters
                !matches!(c, '<' | '>' | '"' | '\'' | '&' | '\0' | '\r' | '\n')
            })
            .take(10000) // Max length
            .collect()
    }

    pub fn escape_html(input: &str) -> String {
        input
            .replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
            .replace('\'', "&#x27;")
    }

    pub fn fuzz_target(data: &[u8]) {
        if let Ok(s) = std::str::from_utf8(data) {
            let sanitized = sanitize(s);
            let escaped = escape_html(s);

            // Verify no dangerous chars remain in sanitized
            assert!(!sanitized.contains('<'));
            assert!(!sanitized.contains('>'));

            // Verify escaped doesn't contain raw < or >
            assert!(!escaped.contains('<') || escaped.contains("&lt;"));
            assert!(!escaped.contains('>') || escaped.contains("&gt;"));
        }
    }
}

/// Fuzz target: Haversine Distance Calculator
pub mod fuzz_haversine {
    pub fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
        const R: f64 = 6371.0; // Earth's radius in km

        let lat1_rad = lat1.to_radians();
        let lat2_rad = lat2.to_radians();
        let delta_lat = (lat2 - lat1).to_radians();
        let delta_lon = (lon2 - lon1).to_radians();

        let a = (delta_lat / 2.0).sin().powi(2)
            + lat1_rad.cos() * lat2_rad.cos() * (delta_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().asin();

        R * c
    }

    pub fn fuzz_target(data: &[u8]) {
        if data.len() < 32 {
            return;
        }

        // Extract 4 f64 values from random bytes
        let lat1 = f64::from_le_bytes(data[0..8].try_into().unwrap());
        let lon1 = f64::from_le_bytes(data[8..16].try_into().unwrap());
        let lat2 = f64::from_le_bytes(data[16..24].try_into().unwrap());
        let lon2 = f64::from_le_bytes(data[24..32].try_into().unwrap());

        // Should not panic on any input
        let result = haversine_distance(lat1, lon1, lat2, lon2);

        // Result should be non-negative or NaN for invalid coords
        assert!(result >= 0.0 || result.is_nan());
    }
}

/// Fuzz target: Query String Parser
pub mod fuzz_query_parser {
    use std::collections::HashMap;

    pub fn parse_query_string(query: &str) -> HashMap<String, String> {
        let mut params = HashMap::new();

        for pair in query.split('&') {
            if let Some(eq_pos) = pair.find('=') {
                let key = &pair[..eq_pos];
                let value = &pair[eq_pos + 1..];
                if !key.is_empty() {
                    params.insert(
                        url_decode(key),
                        url_decode(value),
                    );
                }
            }
        }

        params
    }

    fn url_decode(input: &str) -> String {
        let mut result = String::new();
        let mut chars = input.chars().peekable();

        while let Some(c) = chars.next() {
            if c == '%' {
                let hex: String = chars.by_ref().take(2).collect();
                if hex.len() == 2 {
                    if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                        result.push(byte as char);
                        continue;
                    }
                }
                result.push('%');
                result.push_str(&hex);
            } else if c == '+' {
                result.push(' ');
            } else {
                result.push(c);
            }
        }

        result
    }

    pub fn fuzz_target(data: &[u8]) {
        if let Ok(s) = std::str::from_utf8(data) {
            let _ = parse_query_string(s);
        }
    }
}

// =============================================================================
// Test the fuzz targets with known inputs
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fuzz_phone_validator_basic() {
        assert!(fuzz_phone_validator::validate_phone("+1234567890"));
        assert!(fuzz_phone_validator::validate_phone("+919876543210"));
        assert!(!fuzz_phone_validator::validate_phone(""));
        assert!(!fuzz_phone_validator::validate_phone("abc"));
        assert!(!fuzz_phone_validator::validate_phone("123")); // too short
    }

    #[test]
    fn test_fuzz_email_validator_basic() {
        assert!(fuzz_email_validator::validate_email("test@example.com"));
        assert!(!fuzz_email_validator::validate_email(""));
        assert!(!fuzz_email_validator::validate_email("noatsign"));
        assert!(!fuzz_email_validator::validate_email("no@domain"));
        assert!(!fuzz_email_validator::validate_email("double..dot@test.com"));
    }

    #[test]
    fn test_fuzz_jwt_decoder_basic() {
        // Valid JWT structure (content doesn't matter for parsing)
        let jwt = "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxIn0.signature";
        let parts = fuzz_jwt_decoder::decode_jwt_parts(jwt);
        assert!(parts.is_some());

        // Invalid - not 3 parts
        assert!(fuzz_jwt_decoder::decode_jwt_parts("invalid").is_none());
        assert!(fuzz_jwt_decoder::decode_jwt_parts("a.b").is_none());
    }

    #[test]
    fn test_fuzz_date_parser_basic() {
        assert_eq!(fuzz_date_parser::parse_date("2024-01-15"), Some((2024, 1, 15)));
        assert_eq!(fuzz_date_parser::parse_date("invalid"), None);
        assert_eq!(fuzz_date_parser::parse_date("2024-13-01"), None); // invalid month
    }

    #[test]
    fn test_fuzz_input_sanitizer_basic() {
        let input = "<script>alert('xss')</script>";
        let sanitized = fuzz_input_sanitizer::sanitize(input);
        assert!(!sanitized.contains('<'));
        assert!(!sanitized.contains('>'));
    }

    #[test]
    fn test_fuzz_haversine_basic() {
        // NYC to LA
        let distance = fuzz_haversine::haversine_distance(40.7128, -74.0060, 34.0522, -118.2437);
        assert!(distance > 3900.0 && distance < 4000.0);

        // Same point
        let same = fuzz_haversine::haversine_distance(0.0, 0.0, 0.0, 0.0);
        assert!(same < 0.001);
    }

    #[test]
    fn test_fuzz_query_parser_basic() {
        let params = fuzz_query_parser::parse_query_string("name=john&age=25");
        assert_eq!(params.get("name"), Some(&"john".to_string()));
        assert_eq!(params.get("age"), Some(&"25".to_string()));

        // URL encoded
        let params2 = fuzz_query_parser::parse_query_string("name=john%20doe");
        assert_eq!(params2.get("name"), Some(&"john doe".to_string()));
    }

    #[test]
    fn test_fuzz_targets_dont_panic_on_random_input() {
        // Test with various random-like inputs
        let test_inputs: &[&[u8]] = &[
            b"",
            b"\x00\x00\x00",
            b"aaaaaaaaaaaaaaaa",
            b"\xff\xff\xff\xff",
            b"<script>",
            b"' OR 1=1 --",
            b"../../../etc/passwd",
            b"\n\r\t",
            b"emoji: \xf0\x9f\x98\x80",
        ];

        for input in test_inputs {
            // None of these should panic
            fuzz_json_parser::fuzz_target(input);
            fuzz_phone_validator::fuzz_target(input);
            fuzz_email_validator::fuzz_target(input);
            fuzz_jwt_decoder::fuzz_target(input);
            fuzz_url_parser::fuzz_target(input);
            fuzz_date_parser::fuzz_target(input);
            fuzz_query_parser::fuzz_target(input);

            if input.len() >= 32 {
                fuzz_haversine::fuzz_target(input);
            }
        }
    }
}
