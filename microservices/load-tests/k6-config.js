/**
 * K6 Load Testing Configuration for NAVA Dating Platform
 *
 * Usage:
 *   k6 run k6-config.js
 *   k6 run --vus 100 --duration 5m k6-config.js
 *
 * Environment Variables:
 *   BASE_URL: API base URL (default: http://localhost:8080)
 *   AUTH_TOKEN: Bearer token for authenticated requests
 */

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Rate, Trend, Counter } from 'k6/metrics';

// Custom metrics
const errorRate = new Rate('errors');
const authLatency = new Trend('auth_latency');
const matchLatency = new Trend('match_latency');
const chatLatency = new Trend('chat_latency');
const apiCalls = new Counter('api_calls');

// Configuration
const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080';
const AUTH_TOKEN = __ENV.AUTH_TOKEN || '';

// Test scenarios
export const options = {
  scenarios: {
    // Smoke test: quick validation
    smoke: {
      executor: 'constant-vus',
      vus: 1,
      duration: '1m',
      tags: { test_type: 'smoke' },
      exec: 'smokeTest',
    },

    // Load test: normal traffic
    load: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '2m', target: 50 },   // Ramp up to 50 users
        { duration: '5m', target: 50 },   // Stay at 50 users
        { duration: '2m', target: 100 },  // Ramp up to 100 users
        { duration: '5m', target: 100 },  // Stay at 100 users
        { duration: '2m', target: 0 },    // Ramp down
      ],
      tags: { test_type: 'load' },
      exec: 'loadTest',
      startTime: '1m', // Start after smoke test
    },

    // Stress test: find breaking point
    stress: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '2m', target: 100 },
        { duration: '5m', target: 200 },
        { duration: '2m', target: 300 },
        { duration: '5m', target: 400 },
        { duration: '2m', target: 0 },
      ],
      tags: { test_type: 'stress' },
      exec: 'stressTest',
      startTime: '18m', // Start after load test
    },

    // Spike test: sudden traffic spike
    spike: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '10s', target: 100 },  // Quick ramp up
        { duration: '1m', target: 100 },   // Stay at spike
        { duration: '10s', target: 500 },  // Massive spike
        { duration: '30s', target: 500 },  // Stay at massive spike
        { duration: '1m', target: 100 },   // Back to normal
        { duration: '30s', target: 0 },    // Ramp down
      ],
      tags: { test_type: 'spike' },
      exec: 'spikeTest',
      startTime: '35m', // Start after stress test
    },
  },

  thresholds: {
    // HTTP request thresholds
    http_req_duration: ['p(95)<500', 'p(99)<1500'],
    http_req_failed: ['rate<0.01'],

    // Custom metric thresholds
    errors: ['rate<0.05'],
    auth_latency: ['p(95)<300'],
    match_latency: ['p(95)<200'],
    chat_latency: ['p(95)<100'],
  },
};

// Headers for authenticated requests
function authHeaders() {
  return {
    'Authorization': `Bearer ${AUTH_TOKEN}`,
    'Content-Type': 'application/json',
    'X-API-Version': '1',
  };
}

// Smoke test - basic functionality
export function smokeTest() {
  group('Health Check', function() {
    const res = http.get(`${BASE_URL}/health`);
    check(res, {
      'health check status 200': (r) => r.status === 200,
      'health check response time < 100ms': (r) => r.timings.duration < 100,
    });
    apiCalls.add(1);
  });

  sleep(1);
}

// Load test - typical user journey
export function loadTest() {
  const userId = Math.floor(Math.random() * 10000);

  group('Authentication Flow', function() {
    // Login
    const loginRes = http.post(`${BASE_URL}/api/v1/auth/login`, JSON.stringify({
      phone: `+1555${userId.toString().padStart(7, '0')}`,
      otp: '123456',
    }), { headers: { 'Content-Type': 'application/json' } });

    authLatency.add(loginRes.timings.duration);
    apiCalls.add(1);

    check(loginRes, {
      'login successful': (r) => r.status === 200 || r.status === 201,
    }) || errorRate.add(1);
  });

  sleep(1);

  group('Discovery Flow', function() {
    // Get discover profiles
    const discoverRes = http.get(`${BASE_URL}/api/v1/discover?limit=20`, {
      headers: authHeaders(),
    });

    matchLatency.add(discoverRes.timings.duration);
    apiCalls.add(1);

    check(discoverRes, {
      'discover returns profiles': (r) => r.status === 200,
      'discover response time < 300ms': (r) => r.timings.duration < 300,
    }) || errorRate.add(1);

    // Like a profile
    if (discoverRes.status === 200) {
      const profiles = JSON.parse(discoverRes.body);
      if (profiles.length > 0) {
        const likeRes = http.post(`${BASE_URL}/api/v1/matches/like`, JSON.stringify({
          target_user_id: profiles[0].user_id,
        }), { headers: authHeaders() });

        matchLatency.add(likeRes.timings.duration);
        apiCalls.add(1);

        check(likeRes, {
          'like successful': (r) => r.status === 200 || r.status === 201,
        });
      }
    }
  });

  sleep(2);

  group('Chat Flow', function() {
    // Get conversations
    const convRes = http.get(`${BASE_URL}/api/v1/chat/conversations`, {
      headers: authHeaders(),
    });

    chatLatency.add(convRes.timings.duration);
    apiCalls.add(1);

    check(convRes, {
      'conversations retrieved': (r) => r.status === 200,
      'chat response time < 200ms': (r) => r.timings.duration < 200,
    }) || errorRate.add(1);
  });

  sleep(3);
}

// Stress test - heavy operations
export function stressTest() {
  group('Heavy Operations', function() {
    // Profile search with filters
    const searchRes = http.get(`${BASE_URL}/api/v1/search?age_min=21&age_max=35&distance_km=50&verified=true`, {
      headers: authHeaders(),
    });

    apiCalls.add(1);

    check(searchRes, {
      'search under stress': (r) => r.status === 200,
    }) || errorRate.add(1);

    // Get analytics (heavy query)
    const analyticsRes = http.get(`${BASE_URL}/api/v1/analytics/dashboard`, {
      headers: authHeaders(),
    });

    apiCalls.add(1);

    check(analyticsRes, {
      'analytics under stress': (r) => r.status === 200,
    }) || errorRate.add(1);
  });

  sleep(1);
}

// Spike test - burst traffic
export function spikeTest() {
  group('Spike Handling', function() {
    // Quick succession of requests
    for (let i = 0; i < 5; i++) {
      const res = http.get(`${BASE_URL}/api/v1/discover?limit=10`, {
        headers: authHeaders(),
      });

      apiCalls.add(1);

      check(res, {
        'spike handling': (r) => r.status === 200 || r.status === 429, // Accept rate limiting
      }) || errorRate.add(1);
    }
  });

  sleep(0.5);
}

// Teardown
export function teardown(data) {
  console.log('Load test completed');
  console.log(`Total API calls: ${apiCalls.value}`);
}
