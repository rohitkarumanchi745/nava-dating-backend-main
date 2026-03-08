// =============================================================================
// PgBouncer + Read Replica Verification Load Test
// =============================================================================
// Validates:
//   1. ML fallback rate stays near 0% under load
//   2. Replica lag stays < 2s
//   3. Pool saturation alerts don't fire
//   4. app_reads_fallback_to_primary stays near 0
//
// Run:
//   k6 run tests/load/k6-pgbouncer-replica.js
//   k6 run --env BASE_URL=http://pgbouncer:6432-proxy tests/load/k6-pgbouncer-replica.js
// =============================================================================

import http from 'k6/http';
import { check, sleep, group } from 'k6';
import { Rate, Trend, Counter, Gauge } from 'k6/metrics';

// Custom metrics
const errorRate = new Rate('errors');
const discoverLatency = new Trend('discover_latency');
const metricsCheckLatency = new Trend('metrics_check_latency');
const mlFallbackObserved = new Counter('ml_fallback_observed');
const replicaFallbackObserved = new Counter('replica_fallback_observed');

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080';
const METRICS_URL = __ENV.METRICS_URL || `${BASE_URL}/metrics`;

export const options = {
  scenarios: {
    // Sustained load: simulate normal traffic through PgBouncer
    discover_load: {
      executor: 'ramping-vus',
      startVUs: 0,
      stages: [
        { duration: '15s', target: 20 },   // Ramp up
        { duration: '2m',  target: 50 },   // Sustained load
        { duration: '30s', target: 100 },  // Spike
        { duration: '1m',  target: 100 },  // Hold spike
        { duration: '15s', target: 0 },    // Ramp down
      ],
      exec: 'discoverFlow',
    },
    // Periodic metrics scrape to verify SLO compliance
    metrics_monitor: {
      executor: 'constant-arrival-rate',
      rate: 1,          // 1 iteration per timeUnit
      timeUnit: '5s',   // every 5 seconds
      duration: '4m',
      preAllocatedVUs: 1,
      exec: 'checkMetrics',
    },
  },

  thresholds: {
    discover_latency: ['p(95)<500', 'p(99)<1000'],
    errors: ['rate<0.05'],           // < 5% error rate
    ml_fallback_observed: ['count<5'], // Near-zero ML fallbacks
    replica_fallback_observed: ['count<10'], // Near-zero replica fallbacks
  },
};

// Helper: auth headers
function authHeaders(token) {
  return {
    headers: {
      'Authorization': `Bearer ${token}`,
      'Content-Type': 'application/json',
    },
  };
}

function getTestToken() {
  return __ENV.TEST_TOKEN || 'eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxIiwiZXhwIjoxOTk5OTk5OTk5LCJpc19hZG1pbiI6ZmFsc2V9.test';
}

// =============================================================================
// Scenario 1: Discover flow (read-heavy, exercises replica + ML ranking)
// =============================================================================
export function discoverFlow() {
  const token = getTestToken();

  group('Discover via PgBouncer', () => {
    // Hit discover endpoint (uses read replica + ML ranking)
    const res = http.get(`${BASE_URL}/api/discover?limit=20`, authHeaders(token));

    discoverLatency.add(res.timings.duration);

    const ok = check(res, {
      'discover 2xx': (r) => r.status >= 200 && r.status < 300,
      'has profiles': (r) => {
        try { return r.json('profiles') !== undefined; } catch (e) { return false; }
      },
      'latency < 500ms': (r) => r.timings.duration < 500,
    });

    if (!ok) errorRate.add(1);

    // Simulate user think time (viewing profiles)
    sleep(Math.random() * 2 + 1);

    // Swipe on a random profile (write path, exercises PgBouncer transaction pooling)
    const profiles = (() => { try { return res.json('profiles') || []; } catch (e) { return []; } })();
    if (profiles.length > 0) {
      const target = profiles[Math.floor(Math.random() * profiles.length)];
      const action = Math.random() > 0.6 ? 'like' : 'pass';

      const swipeRes = http.post(`${BASE_URL}/api/${action}/${target.id}`, null, authHeaders(token));

      check(swipeRes, {
        'swipe accepted': (r) => r.status >= 200 && r.status < 300,
      }) || errorRate.add(1);
    }

    sleep(Math.random() * 1 + 0.5);
  });
}

// =============================================================================
// Scenario 2: Periodic metrics check (verifies SLO compliance in real-time)
// =============================================================================
export function checkMetrics() {
  const res = http.get(METRICS_URL);
  metricsCheckLatency.add(res.timings.duration);

  if (res.status !== 200) {
    console.warn(`Metrics endpoint returned ${res.status}`);
    return;
  }

  const body = res.body;

  // Parse key metrics from Prometheus text format
  const replicaLag = parseMetric(body, 'app_replica_lag_ms');
  const replicaHealthy = parseMetric(body, 'app_replica_healthy');
  const readsFallback = parseMetric(body, 'app_reads_fallback_to_primary');
  const mlFallback = parseMetric(body, 'app_ml_fallback_total');
  const discoverTotal = parseMetric(body, 'app_discover_requests_total');
  const poolIdle = parseMetric(body, 'app_db_pool_idle');
  const poolSize = parseMetric(body, 'app_db_pool_size');

  // Check replica lag < 2000ms
  if (replicaLag !== null) {
    const lagOk = check(null, {
      'replica lag < 2s': () => replicaLag < 2000,
    });
    if (!lagOk) console.error(`REPLICA LAG HIGH: ${replicaLag}ms`);
  }

  // Check replica is healthy
  if (replicaHealthy !== null) {
    check(null, {
      'replica healthy': () => replicaHealthy === 1,
    });
  }

  // Track fallback counters for threshold checks
  if (readsFallback !== null && readsFallback > 0) {
    replicaFallbackObserved.add(readsFallback);
    console.warn(`Replica fallbacks detected: ${readsFallback}`);
  }

  if (mlFallback !== null && mlFallback > 0) {
    mlFallbackObserved.add(mlFallback);
    const fallbackRate = discoverTotal > 0 ? (mlFallback / discoverTotal * 100).toFixed(2) : 'N/A';
    console.warn(`ML fallbacks: ${mlFallback} / ${discoverTotal} (${fallbackRate}%)`);
  }

  // Check pool saturation < 80%
  if (poolIdle !== null && poolSize !== null && poolSize > 0) {
    const utilization = 1 - (poolIdle / poolSize);
    check(null, {
      'pool utilization < 80%': () => utilization < 0.8,
    });
    if (utilization > 0.8) {
      console.error(`POOL SATURATED: ${(utilization * 100).toFixed(1)}% (idle: ${poolIdle}/${poolSize})`);
    }
  }
}

// Parse a single numeric metric value from Prometheus text format
function parseMetric(body, name) {
  const lines = body.split('\n');
  for (const line of lines) {
    if (line.startsWith(name + ' ') || line.startsWith(name + '{')) {
      const parts = line.split(' ');
      const val = parseFloat(parts[parts.length - 1]);
      return isNaN(val) ? null : val;
    }
  }
  return null;
}
