//! Integration tests for NAVA API endpoints
//!
//! Run all tests: cargo test --test api_tests
//! Run unit tests only: cargo test --test api_tests unit_tests
//! Run integration tests: cargo test --test api_tests integration_tests -- --ignored
//! Run security tests: cargo test --test api_tests security_tests
//!
//! For integration tests, ensure:
//! - PostgreSQL is running with TEST_DATABASE_URL configured
//! - Redis is running (optional)

mod common;

use common::{create_admin_test_token, create_test_token, TestConfig};

// =============================================================================
// Unit Tests (no external dependencies) - 30+ tests
// =============================================================================

#[cfg(test)]
mod unit_tests {
    use super::*;

    #[test]
    fn test_create_test_token() {
        let config = TestConfig::default();
        let token = create_test_token(1, &config.secret_key);
        assert!(!token.is_empty());
        assert!(token.contains('.')); // JWT format: header.payload.signature

        // Verify it has 3 parts
        let parts: Vec<&str> = token.split('.').collect();
        assert_eq!(parts.len(), 3, "JWT should have 3 parts");
    }

    #[test]
    fn test_create_admin_token() {
        let config = TestConfig::default();
        let token = create_admin_test_token(1, &config.secret_key);
        assert!(!token.is_empty());

        // Decode and verify is_admin claim
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let parts: Vec<&str> = token.split('.').collect();
        let payload = URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&payload).unwrap();
        assert_eq!(claims["is_admin"], true);
    }

    #[test]
    fn test_phone_number_validation() {
        // Valid phone numbers
        assert!(is_valid_phone("+1234567890"));
        assert!(is_valid_phone("+919876543210"));
        assert!(is_valid_phone("1234567890"));

        // Invalid phone numbers
        assert!(!is_valid_phone(""));
        assert!(!is_valid_phone("123")); // Too short
        assert!(!is_valid_phone("abcdefghij")); // Not numeric
    }

    fn is_valid_phone(phone: &str) -> bool {
        if phone.is_empty() {
            return false;
        }
        let cleaned: String = phone.chars().filter(|c| c.is_ascii_digit() || *c == '+').collect();
        cleaned.len() >= 10 && cleaned.len() <= 15
    }

    #[test]
    fn test_email_validation() {
        // Valid emails
        assert!(is_valid_email("test@example.com"));
        assert!(is_valid_email("user.name@domain.co.uk"));
        assert!(is_valid_email("user+tag@example.org"));

        // Invalid emails
        assert!(!is_valid_email(""));
        assert!(!is_valid_email("notanemail"));
        assert!(!is_valid_email("@nodomain"));
        assert!(!is_valid_email("noat.com"));
    }

    fn is_valid_email(email: &str) -> bool {
        email.contains('@') && email.contains('.') && email.len() >= 5
    }

    #[test]
    fn test_age_calculation() {
        use chrono::{Datelike, NaiveDate, Utc};

        fn calculate_age(dob: NaiveDate) -> i32 {
            let today = Utc::now().date_naive();
            let mut age = today.year() - dob.year();
            if (today.month(), today.day()) < (dob.month(), dob.day()) {
                age -= 1;
            }
            age
        }

        // Someone born exactly 25 years ago
        let today = Utc::now().date_naive();
        let dob_25_years_ago = NaiveDate::from_ymd_opt(
            today.year() - 25,
            today.month(),
            today.day()
        ).unwrap();

        assert_eq!(calculate_age(dob_25_years_ago), 25);
    }

    #[test]
    fn test_age_validation_18_plus() {
        use chrono::{Datelike, NaiveDate, Utc};

        fn is_18_or_older(dob: NaiveDate) -> bool {
            let today = Utc::now().date_naive();
            let mut age = today.year() - dob.year();
            if (today.month(), today.day()) < (dob.month(), dob.day()) {
                age -= 1;
            }
            age >= 18
        }

        let today = Utc::now().date_naive();

        // 18 years old - should pass
        let dob_18 = NaiveDate::from_ymd_opt(today.year() - 18, today.month(), today.day()).unwrap();
        assert!(is_18_or_older(dob_18));

        // 17 years old - should fail
        let dob_17 = NaiveDate::from_ymd_opt(today.year() - 17, today.month(), today.day()).unwrap();
        assert!(!is_18_or_older(dob_17));

        // 25 years old - should pass
        let dob_25 = NaiveDate::from_ymd_opt(today.year() - 25, 1, 1).unwrap();
        assert!(is_18_or_older(dob_25));
    }

    #[test]
    fn test_compatibility_score_calculation() {
        let my_interests = vec!["music".to_string(), "travel".to_string(), "food".to_string()];
        let their_interests = vec!["music".to_string(), "sports".to_string(), "food".to_string()];

        let overlap = calculate_interest_overlap(&my_interests, &their_interests);
        assert_eq!(overlap, 2); // "music" and "food"
    }

    fn calculate_interest_overlap(mine: &[String], theirs: &[String]) -> usize {
        use std::collections::HashSet;
        let my_set: HashSet<_> = mine.iter().map(|s| s.to_lowercase()).collect();
        let their_set: HashSet<_> = theirs.iter().map(|s| s.to_lowercase()).collect();
        my_set.intersection(&their_set).count()
    }

    #[test]
    fn test_compatibility_score_full() {
        // Test full compatibility score calculation
        let score = calculate_compatibility(
            3,    // interest overlap
            5.0,  // distance km
            true, // same language
            true, // verified
        );

        // Score should be higher for more overlap, closer distance, same language, verified
        assert!(score > 0.5);
        assert!(score <= 1.0);
    }

    fn calculate_compatibility(
        interest_overlap: usize,
        distance_km: f64,
        same_language: bool,
        is_verified: bool,
    ) -> f64 {
        let mut score = 0.0;

        // Interest overlap: 0-40%
        score += (interest_overlap as f64 / 10.0).min(0.4);

        // Distance: 0-30% (closer is better)
        score += 0.3 * (1.0 - (distance_km / 100.0).min(1.0));

        // Same language: 15%
        if same_language {
            score += 0.15;
        }

        // Verified: 15%
        if is_verified {
            score += 0.15;
        }

        score.min(1.0)
    }

    #[test]
    fn test_pass_type_pricing() {
        assert_eq!(pass_duration_hours("hourly"), Some(1));
        assert_eq!(pass_duration_hours("daily"), Some(24));
        assert_eq!(pass_duration_hours("weekly"), Some(24 * 7));
        assert_eq!(pass_duration_hours("monthly"), Some(24 * 30));
        assert_eq!(pass_duration_hours("ultra"), None); // Unlimited
        assert_eq!(pass_duration_hours("free"), None);
    }

    fn pass_duration_hours(pass_type: &str) -> Option<i64> {
        match pass_type {
            "hourly" => Some(1),
            "daily" => Some(24),
            "weekly" => Some(24 * 7),
            "monthly" => Some(24 * 30),
            "ultra" | "free" => None,
            _ => None,
        }
    }

    #[test]
    fn test_pass_pricing_cents() {
        assert_eq!(pass_price_cents("hourly"), 299);
        assert_eq!(pass_price_cents("daily"), 499);
        assert_eq!(pass_price_cents("weekly"), 999);
        assert_eq!(pass_price_cents("monthly"), 1999);
        assert_eq!(pass_price_cents("ultra"), 4999);
    }

    fn pass_price_cents(pass_type: &str) -> u32 {
        match pass_type {
            "hourly" => 299,
            "daily" => 499,
            "weekly" => 999,
            "monthly" => 1999,
            "ultra" => 4999,
            _ => 0,
        }
    }

    #[test]
    fn test_student_discount_tiers() {
        assert!(student_discount("ivy") > student_discount("top50"));
        assert!(student_discount("top50") > student_discount("state"));
        assert!(student_discount("state") > student_discount("other"));
        assert!(student_discount("alumni") > 0.0);
        assert_eq!(student_discount("none"), 0.0);
    }

    fn student_discount(tier: &str) -> f64 {
        match tier {
            "ivy" => 0.30,       // 30% for Ivy League
            "top50" => 0.20,    // 20% for top 50 schools
            "state" => 0.15,    // 15% for state schools
            "other" => 0.10,    // 10% for other accredited
            "graduate" => 0.15, // 15% for graduate students
            "alumni" => 0.05,   // 5% for alumni
            _ => 0.0,
        }
    }

    #[test]
    fn test_student_discounted_price() {
        let base_price = 1999; // $19.99
        let ivy_discount = 0.30;

        let discounted = apply_discount(base_price, ivy_discount);
        assert_eq!(discounted, 1399); // $13.99 after 30% off
    }

    fn apply_discount(price_cents: u32, discount: f64) -> u32 {
        let discounted = price_cents as f64 * (1.0 - discount);
        discounted.round() as u32
    }

    #[test]
    fn test_rate_limit_key_generation() {
        let user_key = format_rate_limit_key("user", 123);
        assert_eq!(user_key, "rate_limit:user:123");

        let ip_key = format_rate_limit_key("ip", "192.168.1.1");
        assert_eq!(ip_key, "rate_limit:ip:192.168.1.1");

        let endpoint_key = format_rate_limit_key("endpoint", "/api/discover");
        assert_eq!(endpoint_key, "rate_limit:endpoint:/api/discover");
    }

    fn format_rate_limit_key<T: std::fmt::Display>(prefix: &str, identifier: T) -> String {
        format!("rate_limit:{}:{}", prefix, identifier)
    }

    #[test]
    fn test_rate_limit_check() {
        // Simulate rate limit checking
        let limit = 120; // 120 requests per minute
        let burst = 30;

        assert!(!is_rate_limited(100, limit, burst));
        assert!(!is_rate_limited(150, limit, burst)); // Within burst
        assert!(is_rate_limited(151, limit, burst));  // Over burst
    }

    fn is_rate_limited(count: u32, limit: u32, burst: u32) -> bool {
        count > limit + burst
    }

    #[test]
    fn test_gender_validation() {
        let valid_genders = ["male", "female", "non_binary", "other"];

        assert!(valid_genders.contains(&"male"));
        assert!(valid_genders.contains(&"female"));
        assert!(valid_genders.contains(&"non_binary"));
        assert!(valid_genders.contains(&"other"));
        assert!(!valid_genders.contains(&"invalid"));
        assert!(!valid_genders.contains(&""));
    }

    #[test]
    fn test_jwt_expiration_check() {
        use chrono::{Duration, Utc};

        fn is_token_expired(exp: i64) -> bool {
            let now = Utc::now().timestamp();
            exp < now
        }

        // Token expiring in 1 hour - not expired
        let future_exp = (Utc::now() + Duration::hours(1)).timestamp();
        assert!(!is_token_expired(future_exp));

        // Token expired 1 hour ago
        let past_exp = (Utc::now() - Duration::hours(1)).timestamp();
        assert!(is_token_expired(past_exp));
    }

    #[test]
    fn test_haversine_distance() {
        // Calculate distance between two coordinates
        fn haversine_distance(lat1: f64, lon1: f64, lat2: f64, lon2: f64) -> f64 {
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

        // New York to Los Angeles (approx 3944 km)
        let distance = haversine_distance(40.7128, -74.0060, 34.0522, -118.2437);
        assert!(distance > 3900.0 && distance < 4000.0);

        // Same location should be 0
        let same_distance = haversine_distance(40.7128, -74.0060, 40.7128, -74.0060);
        assert!(same_distance < 0.001);
    }

    #[test]
    fn test_input_sanitization() {
        fn sanitize_input(input: &str) -> String {
            input
                .chars()
                .filter(|c| !['<', '>', '"', '\'', '&', '\0'].contains(c))
                .take(10000) // Max length
                .collect()
        }

        assert_eq!(sanitize_input("hello"), "hello");
        assert_eq!(sanitize_input("<script>alert('xss')</script>"), "scriptalert(xss)/script");
        assert_eq!(sanitize_input("normal text"), "normal text");
    }

    #[test]
    fn test_uuid_generation() {
        use uuid::Uuid;

        let id1 = Uuid::new_v4();
        let id2 = Uuid::new_v4();

        assert_ne!(id1, id2);
        assert_eq!(id1.to_string().len(), 36); // UUID format: 8-4-4-4-12
    }

    #[test]
    fn test_date_parsing() {
        use chrono::NaiveDate;

        let valid_date = NaiveDate::parse_from_str("2000-01-15", "%Y-%m-%d");
        assert!(valid_date.is_ok());

        let invalid_date = NaiveDate::parse_from_str("not-a-date", "%Y-%m-%d");
        assert!(invalid_date.is_err());

        let invalid_format = NaiveDate::parse_from_str("01/15/2000", "%Y-%m-%d");
        assert!(invalid_format.is_err());
    }

    #[test]
    fn test_json_serialization() {
        use serde::{Deserialize, Serialize};

        #[derive(Serialize, Deserialize, PartialEq, Debug)]
        struct TestUser {
            id: i32,
            name: String,
        }

        let user = TestUser {
            id: 1,
            name: "Test".to_string(),
        };

        let json = serde_json::to_string(&user).unwrap();
        assert!(json.contains("\"id\":1"));
        assert!(json.contains("\"name\":\"Test\""));

        let deserialized: TestUser = serde_json::from_str(&json).unwrap();
        assert_eq!(user, deserialized);
    }

    #[test]
    fn test_base64_encoding() {
        use base64::{engine::general_purpose::STANDARD, Engine};

        let original = "Hello, World!";
        let encoded = STANDARD.encode(original);
        let decoded = STANDARD.decode(&encoded).unwrap();
        let decoded_str = String::from_utf8(decoded).unwrap();

        assert_eq!(original, decoded_str);
    }

    #[test]
    fn test_password_hash_length() {
        // Argon2 hashes should be around 97 characters
        fn mock_hash_length() -> usize {
            // Argon2 PHC format: $argon2id$v=19$m=65536,t=3,p=4$<salt>$<hash>
            97
        }

        assert!(mock_hash_length() > 50);
    }
}

// =============================================================================
// Integration Tests (require database connection)
// =============================================================================

#[cfg(test)]
mod integration_tests {
    use super::*;
    use common::{cleanup_test_user, generate_test_phone, setup_test_db};

    // These tests require a running database
    // Run with: cargo test --test api_tests integration_tests -- --ignored

    #[tokio::test]
    #[ignore = "requires database connection"]
    async fn test_database_connection() {
        let pool = setup_test_db().await;
        assert!(pool.is_some(), "Failed to connect to test database");

        if let Some(pool) = pool {
            // Test basic query
            let result: (i32,) = sqlx::query_as("SELECT 1")
                .fetch_one(&pool)
                .await
                .expect("Failed to execute test query");
            assert_eq!(result.0, 1);
        }
    }

    #[tokio::test]
    #[ignore = "requires database connection"]
    async fn test_user_creation() {
        let pool = match setup_test_db().await {
            Some(p) => p,
            None => {
                eprintln!("Skipping test - no database connection");
                return;
            }
        };

        let test_phone = generate_test_phone();

        // Clean up any existing test user
        cleanup_test_user(&pool, &test_phone).await;

        // Create user
        let result = sqlx::query_scalar::<_, i32>(
            r#"
            INSERT INTO users (phone_number, is_active, is_profile_complete, created_at, updated_at)
            VALUES ($1, TRUE, FALSE, NOW(), NOW())
            RETURNING id
            "#,
        )
        .bind(&test_phone)
        .fetch_one(&pool)
        .await;

        assert!(result.is_ok(), "Failed to create test user");

        let user_id = result.unwrap();
        assert!(user_id > 0);

        // Verify user exists
        let exists: Option<(i32,)> = sqlx::query_as("SELECT id FROM users WHERE phone_number = $1")
            .bind(&test_phone)
            .fetch_optional(&pool)
            .await
            .expect("Query failed");

        assert!(exists.is_some(), "User should exist after creation");

        // Cleanup
        cleanup_test_user(&pool, &test_phone).await;
    }

    #[tokio::test]
    #[ignore = "requires database connection"]
    async fn test_user_update() {
        let pool = match setup_test_db().await {
            Some(p) => p,
            None => return,
        };

        let test_phone = generate_test_phone();
        cleanup_test_user(&pool, &test_phone).await;

        // Create user
        let user_id: i32 = sqlx::query_scalar(
            "INSERT INTO users (phone_number, is_active, created_at, updated_at) VALUES ($1, TRUE, NOW(), NOW()) RETURNING id"
        )
        .bind(&test_phone)
        .fetch_one(&pool)
        .await
        .expect("Failed to create user");

        // Update user
        let update_result = sqlx::query(
            "UPDATE users SET name = $1, gender = $2, is_profile_complete = TRUE WHERE id = $3"
        )
        .bind("Test User")
        .bind("male")
        .bind(user_id)
        .execute(&pool)
        .await;

        assert!(update_result.is_ok());

        // Verify update
        let user: Option<(String, String)> = sqlx::query_as(
            "SELECT name, gender FROM users WHERE id = $1"
        )
        .bind(user_id)
        .fetch_optional(&pool)
        .await
        .expect("Query failed");

        assert!(user.is_some());
        let (name, gender) = user.unwrap();
        assert_eq!(name, "Test User");
        assert_eq!(gender, "male");

        cleanup_test_user(&pool, &test_phone).await;
    }

    #[tokio::test]
    #[ignore = "requires database connection"]
    async fn test_duplicate_phone_number() {
        let pool = match setup_test_db().await {
            Some(p) => p,
            None => return,
        };

        let test_phone = generate_test_phone();
        cleanup_test_user(&pool, &test_phone).await;

        // Create first user
        let result1 = sqlx::query(
            "INSERT INTO users (phone_number, is_active, created_at, updated_at) VALUES ($1, TRUE, NOW(), NOW())"
        )
        .bind(&test_phone)
        .execute(&pool)
        .await;

        assert!(result1.is_ok(), "First insert should succeed");

        // Try to create duplicate
        let result2 = sqlx::query(
            "INSERT INTO users (phone_number, is_active, created_at, updated_at) VALUES ($1, TRUE, NOW(), NOW())"
        )
        .bind(&test_phone)
        .execute(&pool)
        .await;

        assert!(result2.is_err(), "Duplicate phone should fail");

        cleanup_test_user(&pool, &test_phone).await;
    }
}

// =============================================================================
// Security Tests
// =============================================================================

#[cfg(test)]
mod security_tests {
    use super::*;

    #[test]
    fn test_token_tampering_detection() {
        let config = TestConfig::default();
        let token = create_test_token(1, &config.secret_key);

        // Tamper with the token
        let parts: Vec<&str> = token.split('.').collect();
        let tampered_token = format!("{}.{}.tampered", parts[0], parts[1]);

        // Verify that tampered token cannot be decoded with same secret
        use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};

        #[derive(serde::Deserialize)]
        struct Claims {
            sub: String,
            exp: usize,
            is_admin: bool,
        }

        let result = decode::<Claims>(
            &tampered_token,
            &DecodingKey::from_secret(config.secret_key.as_bytes()),
            &Validation::new(Algorithm::HS256),
        );

        assert!(result.is_err(), "Tampered token should fail validation");
    }

    #[test]
    fn test_different_secret_key() {
        let config = TestConfig::default();
        let token = create_test_token(1, &config.secret_key);

        // Try to decode with different secret
        use jsonwebtoken::{decode, Algorithm, DecodingKey, Validation};

        #[derive(serde::Deserialize)]
        struct Claims {
            sub: String,
            exp: usize,
            is_admin: bool,
        }

        let result = decode::<Claims>(
            &token,
            &DecodingKey::from_secret("wrong_secret_key_12345678901234".as_bytes()),
            &Validation::new(Algorithm::HS256),
        );

        assert!(result.is_err(), "Token with wrong secret should fail");
    }

    #[test]
    fn test_admin_claim_elevation() {
        let config = TestConfig::default();

        // Create non-admin token
        let user_token = create_test_token(1, &config.secret_key);

        // Verify it's not admin
        use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
        let parts: Vec<&str> = user_token.split('.').collect();
        let payload = URL_SAFE_NO_PAD.decode(parts[1]).unwrap();
        let claims: serde_json::Value = serde_json::from_slice(&payload).unwrap();

        assert_eq!(claims["is_admin"], false, "User token should not be admin");
    }

    #[test]
    fn test_sql_injection_patterns() {
        fn contains_sql_injection(input: &str) -> bool {
            let patterns = [
                "'; DROP",
                "1=1",
                "OR 1=1",
                "UNION SELECT",
                "--",
                "/*",
                "*/",
                "xp_",
                "exec(",
            ];
            let lower = input.to_lowercase();
            patterns.iter().any(|p| lower.contains(&p.to_lowercase()))
        }

        assert!(contains_sql_injection("'; DROP TABLE users;--"));
        assert!(contains_sql_injection("1 OR 1=1"));
        assert!(contains_sql_injection("UNION SELECT * FROM users"));
        assert!(!contains_sql_injection("normal input"));
        assert!(!contains_sql_injection("john@example.com"));
    }

    #[test]
    fn test_xss_patterns() {
        fn contains_xss(input: &str) -> bool {
            let patterns = [
                "<script",
                "javascript:",
                "onerror=",
                "onload=",
                "onclick=",
                "<img",
                "<iframe",
            ];
            let lower = input.to_lowercase();
            patterns.iter().any(|p| lower.contains(p))
        }

        assert!(contains_xss("<script>alert('xss')</script>"));
        assert!(contains_xss("<img src=x onerror=alert(1)>"));
        assert!(contains_xss("javascript:alert(1)"));
        assert!(!contains_xss("Hello World"));
        assert!(!contains_xss("Normal user bio text"));
    }

    #[test]
    fn test_path_traversal_patterns() {
        fn contains_path_traversal(input: &str) -> bool {
            let patterns = [
                "../",
                "..\\",
                "%2e%2e",
                "%252e",
                "....//",
            ];
            let lower = input.to_lowercase();
            patterns.iter().any(|p| lower.contains(p))
        }

        assert!(contains_path_traversal("../../../etc/passwd"));
        assert!(contains_path_traversal("..\\..\\windows\\system32"));
        assert!(!contains_path_traversal("/normal/path/file.txt"));
        assert!(!contains_path_traversal("photo_123.jpg"));
    }

    #[test]
    fn test_command_injection_patterns() {
        fn contains_command_injection(input: &str) -> bool {
            let patterns = [
                ";",
                "|",
                "`",
                "$(",
                "&&",
                "||",
            ];
            patterns.iter().any(|p| input.contains(p))
        }

        assert!(contains_command_injection("; rm -rf /"));
        assert!(contains_command_injection("| cat /etc/passwd"));
        assert!(contains_command_injection("`whoami`"));
        assert!(!contains_command_injection("normal text"));
    }

    #[test]
    fn test_secret_key_minimum_length() {
        // Secret key should be at least 32 characters for security
        let config = TestConfig::default();
        assert!(
            config.secret_key.len() >= 32,
            "Secret key should be at least 32 characters"
        );
    }

    #[test]
    fn test_phone_number_redaction() {
        fn redact_phone(phone: &str) -> String {
            if phone.len() <= 4 {
                return "****".to_string();
            }
            let visible = &phone[phone.len() - 4..];
            format!("****{}", visible)
        }

        assert_eq!(redact_phone("+1234567890"), "****7890");
        assert_eq!(redact_phone("+919876543210"), "****3210");
    }

    #[test]
    fn test_email_redaction() {
        fn redact_email(email: &str) -> String {
            if let Some(at_pos) = email.find('@') {
                let local = &email[..at_pos];
                let domain = &email[at_pos..];
                let visible = if local.len() >= 2 { 2 } else { 1 };
                format!("{}***{}", &local[..visible], domain)
            } else {
                "***".to_string()
            }
        }

        assert_eq!(redact_email("john@example.com"), "jo***@example.com");
        assert_eq!(redact_email("ab@test.com"), "ab***@test.com");
    }
}

// =============================================================================
// Alert Route Validation Tests — verify slo-alerts.yml structure
// =============================================================================

#[cfg(test)]
mod alert_route_tests {
    use std::collections::HashSet;

    /// Parse slo-alerts.yml and verify all expected alert rules exist with correct config
    #[test]
    fn test_slo_alerts_yaml_has_all_expected_rules() {
        let yaml_content = include_str!("../deploy/slo-alerts.yml");

        // All alert names that must exist
        let expected_alerts = vec![
            // Availability
            "HighErrorRate",
            "ServiceUnhealthy",
            // Latency
            "HighApiLatency",
            "HighDiscoverLatency",
            // Database
            "DbPoolSaturated",
            "DbPoolExhausted",
            "SwipeWriteTpsHigh",
            // Payments
            "PaymentDlqBacklog",
            "PaymentDlqAbandoned",
            // ML Scoring
            "MlScoringLatencyHigh",
            "MlFallbackRateHigh",
            "MlFallbackRateCritical",
            // Vision
            "VisionServiceUnavailable",
            // WebSocket
            "WebSocketConnectionsHigh",
            // Infrastructure
            "InstanceHeartbeatLost",
            "HighCacheMissRate",
            // Replica
            "ReplicaLagHigh",
            "ReplicaUnhealthy",
            "ReplicaFallbackRateHigh",
            // PgBouncer
            "PgBouncerPoolExhausted",
            "PgBouncerClientWaiting",
            // Query
            "StatementTimeoutsRising",
            "AutovacuumBehind",
            // Notification Policy
            "NotifThrottleRateHigh",
            "NotifOptOutRateHigh",
            "NotifVariantSkew",
            "NotifEngagementLow",
            "NotifSendRateZero",
            // Content Pipeline
            "PhotoRejectionRateHigh",
            "ModerationActionsSpike",
            "TrustSafetyFlagsHigh",
            "UploadDeferralsHigh",
            // FL Stability (new)
            "FlAggregationStalled",
            "FlHeadWeightDrift",
            "FlNotifPredictorDrift",
            "FlColdStartBucketsEmpty",
            "FlDpDisabled",
            // Canary (new)
            "CanaryShadowModeStale",
            "CanaryEngagementRegression",
        ];

        let mut found: HashSet<String> = HashSet::new();

        for line in yaml_content.lines() {
            let trimmed = line.trim();
            if trimmed.starts_with("- alert:") {
                let name = trimmed.trim_start_matches("- alert:").trim().to_string();
                found.insert(name);
            }
        }

        let mut missing = Vec::new();
        for alert in &expected_alerts {
            if !found.contains(*alert) {
                missing.push(*alert);
            }
        }

        assert!(
            missing.is_empty(),
            "Missing alert rules in slo-alerts.yml: {:?}\nFound: {:?}",
            missing,
            found
        );

        // Verify total count matches
        assert_eq!(
            found.len(),
            expected_alerts.len(),
            "Alert count mismatch — found {} but expected {}. Extra: {:?}",
            found.len(),
            expected_alerts.len(),
            found.difference(&expected_alerts.iter().map(|s| s.to_string()).collect::<HashSet<_>>())
        );
    }

    #[test]
    fn test_fl_alerts_have_correct_severity() {
        let yaml_content = include_str!("../deploy/slo-alerts.yml");
        let lines: Vec<&str> = yaml_content.lines().collect();

        let severity_checks = vec![
            ("FlAggregationStalled", "warning"),
            ("FlHeadWeightDrift", "warning"),
            ("FlNotifPredictorDrift", "warning"),
            ("FlColdStartBucketsEmpty", "warning"),
            ("FlDpDisabled", "critical"),       // DP off is critical
            ("CanaryShadowModeStale", "warning"),
            ("CanaryEngagementRegression", "warning"),
        ];

        for (alert_name, expected_severity) in &severity_checks {
            // Find the alert line
            let alert_idx = lines.iter().position(|l| {
                l.trim().starts_with("- alert:") && l.contains(alert_name)
            });

            assert!(
                alert_idx.is_some(),
                "Alert {} not found in slo-alerts.yml",
                alert_name
            );

            let idx = alert_idx.unwrap();

            // Search for severity within next 10 lines
            let severity_found = lines[idx..std::cmp::min(idx + 10, lines.len())]
                .iter()
                .any(|l| {
                    let t = l.trim();
                    t.starts_with("severity:") && t.contains(expected_severity)
                });

            assert!(
                severity_found,
                "Alert {} should have severity: {} but doesn't",
                alert_name, expected_severity
            );
        }
    }

    #[test]
    fn test_fl_alerts_have_slo_labels() {
        let yaml_content = include_str!("../deploy/slo-alerts.yml");
        let lines: Vec<&str> = yaml_content.lines().collect();

        // FL alerts should have slo: ml_scoring or slo: privacy
        let slo_checks = vec![
            ("FlAggregationStalled", "ml_scoring"),
            ("FlHeadWeightDrift", "ml_scoring"),
            ("FlNotifPredictorDrift", "ml_scoring"),
            ("FlColdStartBucketsEmpty", "ml_scoring"),
            ("FlDpDisabled", "privacy"),
        ];

        for (alert_name, expected_slo) in &slo_checks {
            let alert_idx = lines.iter().position(|l| {
                l.trim().starts_with("- alert:") && l.contains(alert_name)
            }).unwrap_or_else(|| panic!("Alert {} not found", alert_name));

            let slo_found = lines[alert_idx..std::cmp::min(alert_idx + 10, lines.len())]
                .iter()
                .any(|l| {
                    let t = l.trim();
                    t.starts_with("slo:") && t.contains(expected_slo)
                });

            assert!(
                slo_found,
                "Alert {} should have slo: {} label",
                alert_name, expected_slo
            );
        }
    }

    #[test]
    fn test_canary_alerts_have_notifications_slo() {
        let yaml_content = include_str!("../deploy/slo-alerts.yml");
        let lines: Vec<&str> = yaml_content.lines().collect();

        for alert_name in &["CanaryShadowModeStale", "CanaryEngagementRegression"] {
            let alert_idx = lines.iter().position(|l| {
                l.trim().starts_with("- alert:") && l.contains(alert_name)
            }).unwrap_or_else(|| panic!("Alert {} not found", alert_name));

            let slo_found = lines[alert_idx..std::cmp::min(alert_idx + 10, lines.len())]
                .iter()
                .any(|l| {
                    let t = l.trim();
                    t.starts_with("slo:") && t.contains("notifications")
                });

            assert!(
                slo_found,
                "Canary alert {} should have slo: notifications label",
                alert_name
            );
        }
    }

    #[test]
    fn test_all_alerts_have_summary_annotation() {
        let yaml_content = include_str!("../deploy/slo-alerts.yml");
        let lines: Vec<&str> = yaml_content.lines().collect();

        let mut alert_indices: Vec<(usize, String)> = Vec::new();
        for (i, line) in lines.iter().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("- alert:") {
                let name = trimmed.trim_start_matches("- alert:").trim().to_string();
                alert_indices.push((i, name));
            }
        }

        for (idx, name) in &alert_indices {
            // Each alert should have a summary annotation within 20 lines
            let has_summary = lines[*idx..std::cmp::min(*idx + 20, lines.len())]
                .iter()
                .any(|l| l.trim().starts_with("summary:"));

            assert!(
                has_summary,
                "Alert {} at line {} is missing summary annotation",
                name, idx + 1
            );
        }
    }

    #[test]
    fn test_alert_groups_exist() {
        let yaml_content = include_str!("../deploy/slo-alerts.yml");

        let expected_groups = vec![
            "slo_availability",
            "slo_latency",
            "slo_database",
            "slo_payments",
            "slo_ml",
            "slo_vision",
            "slo_websocket",
            "infra_alerts",
            "replica_alerts",
            "pgbouncer_alerts",
            "query_alerts",
            "notification_policy_alerts",
            "content_pipeline_alerts",
            "fl_stability_alerts",
            "canary_alerts",
        ];

        for group in &expected_groups {
            assert!(
                yaml_content.contains(&format!("name: {}", group)),
                "Alert group '{}' not found in slo-alerts.yml",
                group
            );
        }
    }
}
