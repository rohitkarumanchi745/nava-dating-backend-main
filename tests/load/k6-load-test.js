// =============================================================================
// NAVA Platform - Load Testing with k6
// =============================================================================
// Install: brew install k6
// Run: k6 run tests/load/k6-load-test.js
// Run with options: k6 run --vus 50 --duration 30s tests/load/k6-load-test.js
// =============================================================================

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Rate, Trend, Counter } from 'k6/metrics';

// Custom metrics
const errorRate = new Rate('errors');
const authLatency = new Trend('auth_latency');
const discoverLatency = new Trend('discover_latency');
const profileLatency = new Trend('profile_latency');
const matchLatency = new Trend('match_latency');
const requestCount = new Counter('requests');

// Test configuration
export const options = {
  // Ramp-up pattern
  stages: [
    { duration: '30s', target: 10 },   // Ramp up to 10 users
    { duration: '1m', target: 50 },    // Ramp up to 50 users
    { duration: '2m', target: 50 },    // Stay at 50 users
    { duration: '1m', target: 100 },   // Ramp up to 100 users
    { duration: '2m', target: 100 },   // Stay at 100 users
    { duration: '30s', target: 0 },    // Ramp down
  ],

  // Thresholds for pass/fail
  thresholds: {
    http_req_duration: ['p(95)<500', 'p(99)<1000'], // 95% < 500ms, 99% < 1s
    errors: ['rate<0.1'],                            // Error rate < 10%
    auth_latency: ['p(95)<300'],                     // Auth endpoints < 300ms
    discover_latency: ['p(95)<800'],                 // Discover with ML < 800ms
    profile_latency: ['p(95)<200'],                  // Profile < 200ms
    match_latency: ['p(95)<300'],                    // Match operations < 300ms
  },
};

// Environment configuration
const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080';
const TEST_PHONE = __ENV.TEST_PHONE || '+1234567890';

// Helper: Generate random phone number
function randomPhone() {
  return '+1' + Math.floor(Math.random() * 9000000000 + 1000000000);
}

// Helper: Create auth headers
function authHeaders(token) {
  return {
    headers: {
      'Authorization': `Bearer ${token}`,
      'Content-Type': 'application/json',
    },
  };
}

// =============================================================================
// Test Scenarios
// =============================================================================

export default function () {
  // Simulate different user behaviors with weighted distribution
  const scenario = Math.random();

  if (scenario < 0.3) {
    // 30% - New user registration flow
    newUserFlow();
  } else if (scenario < 0.6) {
    // 30% - Active user browsing
    activeUserFlow();
  } else if (scenario < 0.8) {
    // 20% - Matching and chatting
    matchingFlow();
  } else {
    // 20% - Profile updates
    profileUpdateFlow();
  }
}

// =============================================================================
// Flow 1: New User Registration
// =============================================================================

function newUserFlow() {
  group('New User Registration', () => {
    const phone = randomPhone();

    // Step 1: Send OTP
    let res = http.post(`${BASE_URL}/api/auth/send-otp`, JSON.stringify({
      phone_number: phone,
    }), {
      headers: { 'Content-Type': 'application/json' },
    });

    requestCount.add(1);
    authLatency.add(res.timings.duration);

    check(res, {
      'OTP sent': (r) => r.status === 200,
    }) || errorRate.add(1);

    sleep(1);

    // Step 2: Verify OTP (in test mode, OTP is returned)
    const otp = res.json('otp') || '123456';

    res = http.post(`${BASE_URL}/api/auth/verify-otp`, JSON.stringify({
      phone_number: phone,
      otp: otp,
    }), {
      headers: { 'Content-Type': 'application/json' },
    });

    requestCount.add(1);
    authLatency.add(res.timings.duration);

    const success = check(res, {
      'OTP verified': (r) => r.status === 200,
      'Token received': (r) => r.json('access_token') !== undefined,
    });

    if (!success) {
      errorRate.add(1);
      return;
    }

    const token = res.json('access_token');

    // Step 3: Complete profile
    res = http.post(`${BASE_URL}/api/profile/update`, JSON.stringify({
      name: 'Load Test User',
      gender: 'male',
      date_of_birth: '1995-01-15',
      bio: 'Test user for load testing',
      interests: ['music', 'travel', 'food'],
    }), authHeaders(token));

    requestCount.add(1);
    profileLatency.add(res.timings.duration);

    check(res, {
      'Profile updated': (r) => r.status === 200,
    }) || errorRate.add(1);

    sleep(2);
  });
}

// =============================================================================
// Flow 2: Active User Browsing (Discover)
// =============================================================================

function activeUserFlow() {
  group('Active User Browsing', () => {
    // Use pre-authenticated test token
    const token = getTestToken();

    // Step 1: Get discover feed
    let res = http.get(`${BASE_URL}/api/discover`, authHeaders(token));

    requestCount.add(1);
    discoverLatency.add(res.timings.duration);

    check(res, {
      'Discover loaded': (r) => r.status === 200,
      'Has profiles': (r) => r.json('profiles') !== undefined,
    }) || errorRate.add(1);

    sleep(2); // Simulate viewing profiles

    // Step 2: View individual profiles
    const profiles = res.json('profiles') || [];

    for (let i = 0; i < Math.min(3, profiles.length); i++) {
      res = http.get(`${BASE_URL}/api/users/${profiles[i].id}`, authHeaders(token));

      requestCount.add(1);
      profileLatency.add(res.timings.duration);

      check(res, {
        'Profile loaded': (r) => r.status === 200,
      }) || errorRate.add(1);

      sleep(1.5); // Time spent viewing profile
    }

    // Step 3: Swipe actions
    for (let i = 0; i < Math.min(5, profiles.length); i++) {
      const action = Math.random() > 0.7 ? 'like' : 'pass';

      res = http.post(`${BASE_URL}/api/swipe`, JSON.stringify({
        target_user_id: profiles[i].id,
        action: action,
      }), authHeaders(token));

      requestCount.add(1);
      matchLatency.add(res.timings.duration);

      check(res, {
        'Swipe recorded': (r) => r.status === 200 || r.status === 201,
      }) || errorRate.add(1);

      sleep(0.5);
    }
  });
}

// =============================================================================
// Flow 3: Matching and Chatting
// =============================================================================

function matchingFlow() {
  group('Matching and Chatting', () => {
    const token = getTestToken();

    // Step 1: Get matches
    let res = http.get(`${BASE_URL}/api/matches`, authHeaders(token));

    requestCount.add(1);
    matchLatency.add(res.timings.duration);

    check(res, {
      'Matches loaded': (r) => r.status === 200,
    }) || errorRate.add(1);

    const matches = res.json('matches') || [];

    if (matches.length === 0) {
      sleep(1);
      return;
    }

    // Step 2: Open conversation with first match
    const matchId = matches[0].id;

    res = http.get(`${BASE_URL}/api/chat/${matchId}/messages`, authHeaders(token));

    requestCount.add(1);

    check(res, {
      'Messages loaded': (r) => r.status === 200,
    }) || errorRate.add(1);

    sleep(1);

    // Step 3: Send a message
    res = http.post(`${BASE_URL}/api/chat/${matchId}/send`, JSON.stringify({
      content: 'Hello! This is a load test message.',
    }), authHeaders(token));

    requestCount.add(1);

    check(res, {
      'Message sent': (r) => r.status === 200 || r.status === 201,
    }) || errorRate.add(1);

    sleep(2);
  });
}

// =============================================================================
// Flow 4: Profile Updates
// =============================================================================

function profileUpdateFlow() {
  group('Profile Updates', () => {
    const token = getTestToken();

    // Step 1: Get current profile
    let res = http.get(`${BASE_URL}/api/profile`, authHeaders(token));

    requestCount.add(1);
    profileLatency.add(res.timings.duration);

    check(res, {
      'Profile fetched': (r) => r.status === 200,
    }) || errorRate.add(1);

    sleep(1);

    // Step 2: Update bio
    res = http.post(`${BASE_URL}/api/profile/update`, JSON.stringify({
      bio: `Updated at ${new Date().toISOString()}`,
    }), authHeaders(token));

    requestCount.add(1);
    profileLatency.add(res.timings.duration);

    check(res, {
      'Bio updated': (r) => r.status === 200,
    }) || errorRate.add(1);

    sleep(1);

    // Step 3: Update location
    res = http.post(`${BASE_URL}/api/profile/location`, JSON.stringify({
      latitude: 37.7749 + (Math.random() - 0.5) * 0.1,
      longitude: -122.4194 + (Math.random() - 0.5) * 0.1,
    }), authHeaders(token));

    requestCount.add(1);
    profileLatency.add(res.timings.duration);

    check(res, {
      'Location updated': (r) => r.status === 200,
    }) || errorRate.add(1);

    sleep(2);
  });
}

// =============================================================================
// Helper: Get test token (would be pre-generated in real tests)
// =============================================================================

function getTestToken() {
  // In production tests, use environment variable or login
  return __ENV.TEST_TOKEN || 'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxIiwiZXhwIjoxOTk5OTk5OTk5LCJpc19hZG1pbiI6ZmFsc2V9.test';
}

// =============================================================================
// Stress Test Configuration (run separately)
// =============================================================================

export function stressTest() {
  // Stress test: Push to breaking point
  const token = getTestToken();

  // Rapid-fire requests
  for (let i = 0; i < 10; i++) {
    http.get(`${BASE_URL}/api/discover`, authHeaders(token));
    requestCount.add(1);
  }
}

// To run stress test: k6 run --vus 200 --duration 1m -e SCENARIO=stress tests/load/k6-load-test.js
