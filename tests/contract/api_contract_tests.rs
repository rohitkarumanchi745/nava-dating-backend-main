//! Contract Tests for NAVA API
//!
//! Validates API responses match expected schemas/contracts.
//! Run: cargo test --test api_contract_tests
//!
//! These tests ensure API responses maintain backward compatibility.

use serde_json::{json, Value};

// =============================================================================
// Contract Definitions
// =============================================================================

/// User profile contract
fn user_profile_contract() -> Value {
    json!({
        "type": "object",
        "required": ["id", "phone_number", "is_active", "created_at"],
        "properties": {
            "id": { "type": "integer" },
            "phone_number": { "type": "string" },
            "name": { "type": ["string", "null"] },
            "gender": { "type": ["string", "null"], "enum": ["male", "female", "non_binary", "other", null] },
            "date_of_birth": { "type": ["string", "null"], "format": "date" },
            "bio": { "type": ["string", "null"] },
            "interests": { "type": "array", "items": { "type": "string" } },
            "photos": { "type": "array", "items": { "type": "string", "format": "uri" } },
            "is_active": { "type": "boolean" },
            "is_verified": { "type": "boolean" },
            "is_premium": { "type": "boolean" },
            "created_at": { "type": "string", "format": "date-time" },
            "updated_at": { "type": "string", "format": "date-time" }
        }
    })
}

/// Auth response contract
fn auth_response_contract() -> Value {
    json!({
        "type": "object",
        "required": ["access_token", "token_type"],
        "properties": {
            "access_token": { "type": "string", "minLength": 10 },
            "refresh_token": { "type": ["string", "null"] },
            "token_type": { "type": "string", "enum": ["Bearer"] },
            "expires_in": { "type": "integer", "minimum": 0 },
            "user_id": { "type": "integer" },
            "is_new_user": { "type": "boolean" }
        }
    })
}

/// Match response contract
fn match_response_contract() -> Value {
    json!({
        "type": "object",
        "required": ["matches"],
        "properties": {
            "matches": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["match_id", "user", "matched_at"],
                    "properties": {
                        "match_id": { "type": "integer" },
                        "user": { "$ref": "#/definitions/user_preview" },
                        "matched_at": { "type": "string", "format": "date-time" },
                        "last_message": { "type": ["object", "null"] },
                        "unread_count": { "type": "integer", "minimum": 0 }
                    }
                }
            },
            "total": { "type": "integer", "minimum": 0 }
        }
    })
}

/// Discover response contract
fn discover_response_contract() -> Value {
    json!({
        "type": "object",
        "required": ["profiles"],
        "properties": {
            "profiles": {
                "type": "array",
                "items": {
                    "type": "object",
                    "required": ["id", "name", "age", "photos"],
                    "properties": {
                        "id": { "type": "integer" },
                        "name": { "type": "string" },
                        "age": { "type": "integer", "minimum": 18, "maximum": 100 },
                        "bio": { "type": ["string", "null"] },
                        "photos": { "type": "array", "items": { "type": "string" } },
                        "distance_km": { "type": ["number", "null"], "minimum": 0 },
                        "compatibility_score": { "type": ["number", "null"], "minimum": 0, "maximum": 1 },
                        "interests": { "type": "array", "items": { "type": "string" } },
                        "is_verified": { "type": "boolean" },
                        "is_premium": { "type": "boolean" }
                    }
                }
            },
            "has_more": { "type": "boolean" }
        }
    })
}

/// Swipe response contract
fn swipe_response_contract() -> Value {
    json!({
        "type": "object",
        "required": ["success"],
        "properties": {
            "success": { "type": "boolean" },
            "is_match": { "type": "boolean" },
            "match_id": { "type": ["integer", "null"] },
            "remaining_swipes": { "type": ["integer", "null"], "minimum": 0 }
        }
    })
}

/// Message contract
fn message_contract() -> Value {
    json!({
        "type": "object",
        "required": ["id", "sender_id", "content", "created_at"],
        "properties": {
            "id": { "type": "integer" },
            "sender_id": { "type": "integer" },
            "content": { "type": "string" },
            "content_type": { "type": "string", "enum": ["text", "image", "audio", "video", "gif"] },
            "created_at": { "type": "string", "format": "date-time" },
            "read_at": { "type": ["string", "null"], "format": "date-time" },
            "is_deleted": { "type": "boolean" }
        }
    })
}

/// Error response contract
fn error_response_contract() -> Value {
    json!({
        "type": "object",
        "required": ["error"],
        "properties": {
            "error": { "type": "string" },
            "error_code": { "type": ["string", "null"] },
            "details": { "type": ["object", "null"] },
            "trace_id": { "type": ["string", "null"] }
        }
    })
}

/// Subscription status contract
fn subscription_status_contract() -> Value {
    json!({
        "type": "object",
        "required": ["is_premium"],
        "properties": {
            "is_premium": { "type": "boolean" },
            "plan_type": { "type": ["string", "null"], "enum": ["monthly", "quarterly", "yearly", null] },
            "expires_at": { "type": ["string", "null"], "format": "date-time" },
            "features": {
                "type": "object",
                "properties": {
                    "unlimited_swipes": { "type": "boolean" },
                    "see_who_likes": { "type": "boolean" },
                    "super_likes": { "type": "integer", "minimum": 0 },
                    "rewind": { "type": "boolean" },
                    "passport": { "type": "boolean" }
                }
            }
        }
    })
}

// =============================================================================
// Contract Validation Logic
// =============================================================================

/// Validates a JSON value against a contract schema
fn validate_contract(value: &Value, contract: &Value) -> Result<(), Vec<String>> {
    let mut errors = Vec::new();

    validate_value(value, contract, "", &mut errors);

    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}

fn validate_value(value: &Value, schema: &Value, path: &str, errors: &mut Vec<String>) {
    let type_field = schema.get("type");

    // Check type
    if let Some(expected_type) = type_field {
        match expected_type {
            Value::String(t) => {
                if !check_type(value, t) {
                    errors.push(format!("{}: expected type '{}', got {:?}", path, t, value_type(value)));
                }
            }
            Value::Array(types) => {
                let type_match = types.iter().any(|t| {
                    if let Value::String(ts) = t {
                        check_type(value, ts)
                    } else {
                        false
                    }
                });
                if !type_match {
                    errors.push(format!("{}: expected one of {:?}, got {:?}", path, types, value_type(value)));
                }
            }
            _ => {}
        }
    }

    // Check required fields for objects
    if let (Value::Object(obj), Some(Value::Array(required))) = (value, schema.get("required")) {
        for req in required {
            if let Value::String(field) = req {
                if !obj.contains_key(field) {
                    errors.push(format!("{}: missing required field '{}'", path, field));
                }
            }
        }
    }

    // Validate properties
    if let (Value::Object(obj), Some(Value::Object(props))) = (value, schema.get("properties")) {
        for (key, val) in obj {
            if let Some(prop_schema) = props.get(key) {
                let new_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", path, key)
                };
                validate_value(val, prop_schema, &new_path, errors);
            }
        }
    }

    // Validate array items
    if let (Value::Array(arr), Some(items_schema)) = (value, schema.get("items")) {
        for (i, item) in arr.iter().enumerate() {
            let new_path = format!("{}[{}]", path, i);
            validate_value(item, items_schema, &new_path, errors);
        }
    }

    // Check enum
    if let Some(Value::Array(enum_values)) = schema.get("enum") {
        if !enum_values.contains(value) {
            errors.push(format!("{}: value {:?} not in enum {:?}", path, value, enum_values));
        }
    }

    // Check minimum
    if let (Value::Number(n), Some(Value::Number(min))) = (value, schema.get("minimum")) {
        if let (Some(nf), Some(minf)) = (n.as_f64(), min.as_f64()) {
            if nf < minf {
                errors.push(format!("{}: value {} is less than minimum {}", path, nf, minf));
            }
        }
    }

    // Check maximum
    if let (Value::Number(n), Some(Value::Number(max))) = (value, schema.get("maximum")) {
        if let (Some(nf), Some(maxf)) = (n.as_f64(), max.as_f64()) {
            if nf > maxf {
                errors.push(format!("{}: value {} is greater than maximum {}", path, nf, maxf));
            }
        }
    }

    // Check minLength
    if let (Value::String(s), Some(Value::Number(min_len))) = (value, schema.get("minLength")) {
        if let Some(min) = min_len.as_u64() {
            if (s.len() as u64) < min {
                errors.push(format!("{}: string length {} is less than minLength {}", path, s.len(), min));
            }
        }
    }
}

fn check_type(value: &Value, expected: &str) -> bool {
    match expected {
        "string" => value.is_string(),
        "integer" => value.is_i64() || value.is_u64(),
        "number" => value.is_number(),
        "boolean" => value.is_boolean(),
        "array" => value.is_array(),
        "object" => value.is_object(),
        "null" => value.is_null(),
        _ => true,
    }
}

fn value_type(value: &Value) -> &'static str {
    match value {
        Value::Null => "null",
        Value::Bool(_) => "boolean",
        Value::Number(_) => "number",
        Value::String(_) => "string",
        Value::Array(_) => "array",
        Value::Object(_) => "object",
    }
}

// =============================================================================
// Contract Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_valid_auth_response() {
        let response = json!({
            "access_token": "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxIiwiZXhwIjoxNjk5OTk5OTk5fQ.signature",
            "refresh_token": "refresh_token_here",
            "token_type": "Bearer",
            "expires_in": 3600,
            "user_id": 1,
            "is_new_user": false
        });

        let result = validate_contract(&response, &auth_response_contract());
        assert!(result.is_ok(), "Auth response should match contract: {:?}", result);
    }

    #[test]
    fn test_invalid_auth_response_missing_required() {
        let response = json!({
            "access_token": "some_token"
            // Missing token_type
        });

        let result = validate_contract(&response, &auth_response_contract());
        assert!(result.is_err(), "Should fail for missing required field");

        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.contains("token_type")));
    }

    #[test]
    fn test_valid_user_profile() {
        let profile = json!({
            "id": 1,
            "phone_number": "+1234567890",
            "name": "John Doe",
            "gender": "male",
            "date_of_birth": "1995-06-15",
            "bio": "Hello world",
            "interests": ["music", "travel"],
            "photos": ["https://example.com/photo1.jpg"],
            "is_active": true,
            "is_verified": false,
            "is_premium": false,
            "created_at": "2024-01-01T00:00:00Z",
            "updated_at": "2024-01-02T00:00:00Z"
        });

        let result = validate_contract(&profile, &user_profile_contract());
        assert!(result.is_ok(), "User profile should match contract: {:?}", result);
    }

    #[test]
    fn test_valid_discover_response() {
        let response = json!({
            "profiles": [
                {
                    "id": 1,
                    "name": "Jane Doe",
                    "age": 25,
                    "bio": "Hello!",
                    "photos": ["https://example.com/photo.jpg"],
                    "distance_km": 5.5,
                    "compatibility_score": 0.85,
                    "interests": ["music", "food"],
                    "is_verified": true,
                    "is_premium": false
                }
            ],
            "has_more": true
        });

        let result = validate_contract(&response, &discover_response_contract());
        assert!(result.is_ok(), "Discover response should match contract: {:?}", result);
    }

    #[test]
    fn test_invalid_discover_age_below_minimum() {
        let response = json!({
            "profiles": [
                {
                    "id": 1,
                    "name": "Test",
                    "age": 15, // Below minimum 18
                    "photos": []
                }
            ]
        });

        let result = validate_contract(&response, &discover_response_contract());
        assert!(result.is_err(), "Should fail for age below minimum");

        let errors = result.unwrap_err();
        assert!(errors.iter().any(|e| e.contains("minimum")));
    }

    #[test]
    fn test_valid_swipe_response() {
        let response = json!({
            "success": true,
            "is_match": true,
            "match_id": 42,
            "remaining_swipes": 10
        });

        let result = validate_contract(&response, &swipe_response_contract());
        assert!(result.is_ok(), "Swipe response should match contract: {:?}", result);
    }

    #[test]
    fn test_valid_match_response() {
        let response = json!({
            "matches": [
                {
                    "match_id": 1,
                    "user": {
                        "id": 2,
                        "name": "Match User",
                        "photos": []
                    },
                    "matched_at": "2024-01-01T12:00:00Z",
                    "last_message": null,
                    "unread_count": 0
                }
            ],
            "total": 1
        });

        let result = validate_contract(&response, &match_response_contract());
        assert!(result.is_ok(), "Match response should match contract: {:?}", result);
    }

    #[test]
    fn test_valid_message() {
        let message = json!({
            "id": 1,
            "sender_id": 100,
            "content": "Hello!",
            "content_type": "text",
            "created_at": "2024-01-01T12:00:00Z",
            "read_at": null,
            "is_deleted": false
        });

        let result = validate_contract(&message, &message_contract());
        assert!(result.is_ok(), "Message should match contract: {:?}", result);
    }

    #[test]
    fn test_invalid_message_wrong_content_type() {
        let message = json!({
            "id": 1,
            "sender_id": 100,
            "content": "Hello!",
            "content_type": "invalid_type", // Not in enum
            "created_at": "2024-01-01T12:00:00Z"
        });

        let result = validate_contract(&message, &message_contract());
        assert!(result.is_err(), "Should fail for invalid content_type");
    }

    #[test]
    fn test_valid_error_response() {
        let error = json!({
            "error": "Unauthorized",
            "error_code": "AUTH_FAILED",
            "details": { "reason": "Token expired" },
            "trace_id": "abc123"
        });

        let result = validate_contract(&error, &error_response_contract());
        assert!(result.is_ok(), "Error response should match contract: {:?}", result);
    }

    #[test]
    fn test_valid_subscription_status() {
        let status = json!({
            "is_premium": true,
            "plan_type": "monthly",
            "expires_at": "2024-12-31T23:59:59Z",
            "features": {
                "unlimited_swipes": true,
                "see_who_likes": true,
                "super_likes": 5,
                "rewind": true,
                "passport": false
            }
        });

        let result = validate_contract(&status, &subscription_status_contract());
        assert!(result.is_ok(), "Subscription status should match contract: {:?}", result);
    }

    #[test]
    fn test_nullable_fields() {
        let profile = json!({
            "id": 1,
            "phone_number": "+1234567890",
            "name": null, // Nullable field
            "gender": null,
            "bio": null,
            "interests": [],
            "photos": [],
            "is_active": true,
            "created_at": "2024-01-01T00:00:00Z"
        });

        let result = validate_contract(&profile, &user_profile_contract());
        assert!(result.is_ok(), "Nullable fields should be accepted: {:?}", result);
    }

    #[test]
    fn test_empty_array() {
        let response = json!({
            "profiles": [],
            "has_more": false
        });

        let result = validate_contract(&response, &discover_response_contract());
        assert!(result.is_ok(), "Empty array should be valid: {:?}", result);
    }

    #[test]
    fn test_backward_compatibility_extra_fields() {
        // API should be able to add new fields without breaking contracts
        let response = json!({
            "access_token": "token_here_12345",
            "token_type": "Bearer",
            "new_field_v2": "This is a new field added in v2", // Extra field
            "another_new_field": { "nested": true }
        });

        let result = validate_contract(&response, &auth_response_contract());
        assert!(result.is_ok(), "Extra fields should be ignored for backward compatibility");
    }
}
