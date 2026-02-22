# NAVA Platform - Test Suite

Comprehensive testing suite for the NAVA dating platform.

## Test Types

| Type | Purpose | Location | How to Run |
|------|---------|----------|------------|
| **Unit Tests** | Test individual functions | `rust-backend/tests/` | `cargo test` |
| **Integration Tests** | Test with database | `rust-backend/tests/` | `cargo test -- --ignored` |
| **Security Tests** | Vulnerability checks | `rust-backend/tests/` | `cargo test security_tests` |
| **E2E Tests** | Complete user flows | `tests/e2e/` | `cargo test --test user_flows -- --ignored` |
| **Contract Tests** | API schema validation | `tests/contract/` | `cargo test --test api_contract_tests` |
| **Load Tests** | Performance under load | `tests/load/` | `k6 run tests/load/k6-load-test.js` |
| **Smoke Tests** | Quick health checks | `tests/smoke/` | `./tests/smoke/smoke_tests.sh` |
| **Fuzz Tests** | Random input testing | `tests/fuzz/` | `cargo +nightly fuzz run <target>` |
| **Chaos Tests** | Resilience testing | `tests/chaos/` | `./tests/chaos/chaos_tests.sh` |

## Quick Start

### Run All Unit Tests
```bash
cargo test
```

### Run Integration Tests (requires database)
```bash
export TEST_DATABASE_URL="postgres://user:pass@localhost/nava_test"
cargo test -- --ignored
```

### Run Smoke Tests
```bash
# Start the server first
./tests/smoke/smoke_tests.sh http://localhost:8080
```

### Run Load Tests
```bash
# Install k6 first: brew install k6
k6 run tests/load/k6-load-test.js

# With custom options
k6 run --vus 50 --duration 2m tests/load/k6-load-test.js

# With environment variables
k6 run -e BASE_URL=http://api.nava.app -e TEST_TOKEN=xxx tests/load/k6-load-test.js
```

### Run Fuzz Tests
```bash
# Install cargo-fuzz
cargo install cargo-fuzz

# Run a specific fuzz target
cd rust-backend
cargo +nightly fuzz run fuzz_json_parser
cargo +nightly fuzz run fuzz_phone_validator

# List all fuzz targets
cargo +nightly fuzz list
```

### Run Chaos Tests
```bash
# Requires Docker and docker-compose services running
./tests/chaos/chaos_tests.sh http://localhost:8080

# Run with destructive tests (DB/Kafka failures)
RUN_DESTRUCTIVE=true ./tests/chaos/chaos_tests.sh
```

## Test Coverage by Area

### Authentication
- [x] JWT token creation/validation
- [x] Token tampering detection
- [x] OTP send/verify flow
- [x] Admin privilege escalation prevention

### User Management
- [x] Profile creation
- [x] Profile updates
- [x] Phone number validation
- [x] Email validation
- [x] Age calculation (18+ enforcement)

### Matching
- [x] Compatibility score calculation
- [x] Distance calculation (Haversine)
- [x] Interest overlap
- [x] Swipe operations

### Security
- [x] SQL injection patterns
- [x] XSS detection
- [x] Path traversal prevention
- [x] Command injection detection
- [x] Input sanitization
- [x] Rate limiting

### Payments
- [x] Pass pricing
- [x] Student discount tiers
- [x] Discount application

## Load Test Thresholds

| Metric | Target |
|--------|--------|
| p95 response time | < 500ms |
| p99 response time | < 1000ms |
| Error rate | < 10% |
| Auth latency (p95) | < 300ms |
| Discover latency (p95) | < 800ms |

## Adding New Tests

### Unit Test Template
```rust
#[test]
fn test_feature_name() {
    // Arrange
    let input = "test input";

    // Act
    let result = function_under_test(input);

    // Assert
    assert_eq!(result, expected_value);
}
```

### Integration Test Template
```rust
#[tokio::test]
#[ignore = "requires database connection"]
async fn test_database_operation() {
    let pool = setup_test_db().await.expect("DB required");

    // Test code here

    cleanup_test_data(&pool).await;
}
```

### Contract Test Template
```rust
#[test]
fn test_api_response_matches_contract() {
    let response = json!({
        "field1": "value1",
        "field2": 123
    });

    let result = validate_contract(&response, &my_contract());
    assert!(result.is_ok());
}
```

## CI/CD Integration

### GitHub Actions Example
```yaml
test:
  runs-on: ubuntu-latest
  steps:
    - uses: actions/checkout@v3

    - name: Run unit tests
      run: cargo test

    - name: Run smoke tests
      run: ./tests/smoke/smoke_tests.sh http://localhost:8080

    - name: Run load tests
      uses: grafana/k6-action@v0.3.1
      with:
        filename: tests/load/k6-load-test.js
        flags: --vus 10 --duration 30s
```

## Test Data

Test data is automatically cleaned up after each test. For manual testing:

```sql
-- Create test user
INSERT INTO users (phone_number, is_active, created_at, updated_at)
VALUES ('+19999999999', TRUE, NOW(), NOW());

-- Cleanup
DELETE FROM users WHERE phone_number LIKE '+1999%';
```

## Troubleshooting

### Tests failing with "connection refused"
- Ensure PostgreSQL is running
- Check `TEST_DATABASE_URL` environment variable

### Load tests timing out
- Increase timeout in k6 options
- Check if server is handling requests

### Smoke tests failing
- Verify server is running at specified URL
- Check if required endpoints are implemented
