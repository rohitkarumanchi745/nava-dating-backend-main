#!/bin/bash
# =============================================================================
# NAVA Platform - Smoke Tests
# =============================================================================
# Quick health checks for critical endpoints
# Run: ./tests/smoke/smoke_tests.sh [BASE_URL]
# Example: ./tests/smoke/smoke_tests.sh http://localhost:8080
# =============================================================================

set -e

BASE_URL="${1:-http://localhost:8080}"
PASSED=0
FAILED=0
TOTAL=0

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "=============================================="
echo "NAVA Platform - Smoke Tests"
echo "Base URL: $BASE_URL"
echo "=============================================="
echo ""

# Helper function to run a smoke test
smoke_test() {
    local name="$1"
    local method="$2"
    local endpoint="$3"
    local expected_status="$4"
    local data="$5"

    TOTAL=$((TOTAL + 1))

    if [ "$method" == "GET" ]; then
        response=$(curl -s -w "\n%{http_code}" "$BASE_URL$endpoint" 2>/dev/null || echo "000")
    else
        response=$(curl -s -w "\n%{http_code}" -X "$method" \
            -H "Content-Type: application/json" \
            -d "$data" \
            "$BASE_URL$endpoint" 2>/dev/null || echo "000")
    fi

    status_code=$(echo "$response" | tail -n1)
    body=$(echo "$response" | sed '$d')

    if [ "$status_code" == "$expected_status" ]; then
        echo -e "${GREEN}✓${NC} $name (HTTP $status_code)"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}✗${NC} $name"
        echo "  Expected: HTTP $expected_status"
        echo "  Got: HTTP $status_code"
        echo "  Response: $body"
        FAILED=$((FAILED + 1))
    fi
}

# Helper for auth tests
smoke_test_auth() {
    local name="$1"
    local method="$2"
    local endpoint="$3"
    local expected_status="$4"
    local token="$5"
    local data="$6"

    TOTAL=$((TOTAL + 1))

    if [ "$method" == "GET" ]; then
        response=$(curl -s -w "\n%{http_code}" \
            -H "Authorization: Bearer $token" \
            "$BASE_URL$endpoint" 2>/dev/null || echo "000")
    else
        response=$(curl -s -w "\n%{http_code}" -X "$method" \
            -H "Content-Type: application/json" \
            -H "Authorization: Bearer $token" \
            -d "$data" \
            "$BASE_URL$endpoint" 2>/dev/null || echo "000")
    fi

    status_code=$(echo "$response" | tail -n1)
    body=$(echo "$response" | sed '$d')

    if [ "$status_code" == "$expected_status" ]; then
        echo -e "${GREEN}✓${NC} $name (HTTP $status_code)"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}✗${NC} $name"
        echo "  Expected: HTTP $expected_status"
        echo "  Got: HTTP $status_code"
        FAILED=$((FAILED + 1))
    fi
}

# =============================================================================
# 1. Infrastructure Health Checks
# =============================================================================
echo "--- Infrastructure Health Checks ---"

smoke_test "Health endpoint" "GET" "/health" "200"
smoke_test "Ready endpoint" "GET" "/ready" "200"

# =============================================================================
# 2. Authentication Endpoints
# =============================================================================
echo ""
echo "--- Authentication Endpoints ---"

smoke_test "Send OTP endpoint exists" "POST" "/api/auth/send-otp" "400" '{"phone_number": ""}'
smoke_test "Verify OTP endpoint exists" "POST" "/api/auth/verify-otp" "400" '{"phone_number": "", "otp": ""}'

# Test with valid phone format (should return 200 even if phone doesn't exist)
smoke_test "Send OTP with valid phone" "POST" "/api/auth/send-otp" "200" '{"phone_number": "+19999999999"}'

# =============================================================================
# 3. Public Endpoints
# =============================================================================
echo ""
echo "--- Public Endpoints ---"

smoke_test "Universities search" "GET" "/api/universities?search=test" "200"
smoke_test "Subscription plans" "GET" "/api/subscription/plans" "200"

# =============================================================================
# 4. Protected Endpoints (should return 401 without auth)
# =============================================================================
echo ""
echo "--- Protected Endpoints (Auth Required) ---"

smoke_test "Profile requires auth" "GET" "/api/profile" "401"
smoke_test "Discover requires auth" "GET" "/api/discover" "401"
smoke_test "Matches requires auth" "GET" "/api/matches" "401"
smoke_test "Swipe requires auth" "POST" "/api/swipe" "401" '{"target_user_id": 1, "action": "like"}'

# =============================================================================
# 5. GraphQL Endpoint
# =============================================================================
echo ""
echo "--- GraphQL ---"

graphql_query='{"query": "{ __schema { types { name } } }"}'
smoke_test "GraphQL introspection" "POST" "/graphql" "200" "$graphql_query"

# =============================================================================
# 6. API Gateway / Microservices (if running)
# =============================================================================
echo ""
echo "--- Microservices Health (Optional) ---"

# These may fail if microservices aren't running - that's OK for smoke tests
curl -s "$BASE_URL:8001/health" > /dev/null 2>&1 && smoke_test "API Gateway health" "GET" ":8001/health" "200" || echo -e "${YELLOW}○${NC} API Gateway not running (optional)"
curl -s "$BASE_URL:8003/health" > /dev/null 2>&1 && smoke_test "Auth service health" "GET" ":8003/health" "200" || echo -e "${YELLOW}○${NC} Auth service not running (optional)"
curl -s "$BASE_URL:8004/health" > /dev/null 2>&1 && smoke_test "User service health" "GET" ":8004/health" "200" || echo -e "${YELLOW}○${NC} User service not running (optional)"
curl -s "$BASE_URL:8005/health" > /dev/null 2>&1 && smoke_test "Match service health" "GET" ":8005/health" "200" || echo -e "${YELLOW}○${NC} Match service not running (optional)"

# =============================================================================
# 7. Response Time Checks
# =============================================================================
echo ""
echo "--- Response Time Checks ---"

check_response_time() {
    local name="$1"
    local endpoint="$2"
    local max_ms="$3"

    TOTAL=$((TOTAL + 1))

    start_time=$(date +%s%N)
    curl -s "$BASE_URL$endpoint" > /dev/null 2>&1
    end_time=$(date +%s%N)

    duration_ms=$(( (end_time - start_time) / 1000000 ))

    if [ "$duration_ms" -lt "$max_ms" ]; then
        echo -e "${GREEN}✓${NC} $name: ${duration_ms}ms (< ${max_ms}ms)"
        PASSED=$((PASSED + 1))
    else
        echo -e "${RED}✗${NC} $name: ${duration_ms}ms (expected < ${max_ms}ms)"
        FAILED=$((FAILED + 1))
    fi
}

check_response_time "Health endpoint response time" "/health" "100"

# =============================================================================
# Summary
# =============================================================================
echo ""
echo "=============================================="
echo "Smoke Test Results"
echo "=============================================="
echo -e "Passed: ${GREEN}$PASSED${NC}"
echo -e "Failed: ${RED}$FAILED${NC}"
echo "Total:  $TOTAL"
echo ""

if [ "$FAILED" -gt 0 ]; then
    echo -e "${RED}SMOKE TESTS FAILED${NC}"
    exit 1
else
    echo -e "${GREEN}ALL SMOKE TESTS PASSED${NC}"
    exit 0
fi
