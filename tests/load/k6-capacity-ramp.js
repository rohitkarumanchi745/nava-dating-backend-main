// =============================================================================
// k6 capacity-finding ramp — one route at a time
// =============================================================================
// Purpose: find the knee of the latency curve for a single hot endpoint.
// The mixed-flow script (k6-load-test.js) is for soak / regression testing;
// this one isolates a route so DB-pool / Redis / CPU bottlenecks are
// attributable instead of guessable.
//
// Stages walk 250 -> 500 -> 1k -> 2k -> 4k VUs. Adjust MAX_VUS / DURATION_S
// via env if you want shorter/longer runs.
//
// Run:
//   AUTH_TOKEN=eyJhbGc... TARGET=spots_feed BASE_URL=http://localhost:8080 \
//       k6 run tests/load/k6-capacity-ramp.js
//
// TARGET values: spots_feed | discover | swipe | chat_history
// =============================================================================

import http from 'k6/http';
import { check } from 'k6';
import { Rate, Trend } from 'k6/metrics';

const BASE_URL = __ENV.BASE_URL || 'http://localhost:8080';
const AUTH_TOKEN = __ENV.AUTH_TOKEN;
const TARGET = __ENV.TARGET || 'spots_feed';
const MAX_VUS = parseInt(__ENV.MAX_VUS || '4000', 10);
const STAGE_S = parseInt(__ENV.STAGE_S || '60', 10);

if (!AUTH_TOKEN) {
  throw new Error(
    'AUTH_TOKEN env var required. Issue one with the existing OTP flow ' +
    'and pass it: AUTH_TOKEN=... k6 run tests/load/k6-capacity-ramp.js'
  );
}

// Stages scaled relative to MAX_VUS so smaller machines can probe a smaller knee.
const stage = (frac) => Math.max(1, Math.floor(MAX_VUS * frac));
export const options = {
  stages: [
    { duration: `${STAGE_S}s`, target: stage(0.0625) }, //  250 @ 4k
    { duration: `${STAGE_S}s`, target: stage(0.125)  }, //  500
    { duration: `${STAGE_S}s`, target: stage(0.25)   }, // 1000
    { duration: `${STAGE_S}s`, target: stage(0.5)    }, // 2000
    { duration: `${STAGE_S}s`, target: stage(1.0)    }, // 4000
    { duration: '30s',         target: 0 },
  ],
  thresholds: {
    // Loud failures: the point of this test is to find where SLOs break.
    http_req_duration: ['p(95)<1000', 'p(99)<2500'],
    http_req_failed:   ['rate<0.02'],
    checks:            ['rate>0.98'],
  },
  // Useful tag so InfluxDB / Cloud dashboards segment per route.
  tags: { target: TARGET },
};

const routeLatency = new Trend('route_latency');
const routeErrors  = new Rate('route_errors');

const headers = {
  Authorization: `Bearer ${AUTH_TOKEN}`,
  'Content-Type': 'application/json',
};

function hitSpotsFeed() {
  const res = http.get(`${BASE_URL}/spots/feed?limit=20`, { headers });
  recordResult(res, 200);
}

function hitDiscover() {
  const res = http.get(`${BASE_URL}/discover?limit=20`, { headers });
  recordResult(res, 200);
}

function hitSwipe() {
  // POST /swipe expects JSON body. target_user_id is randomized so the
  // server doesn't dedupe and short-circuit. Schema may differ — adjust if
  // the route signature has drifted.
  const targetId = 1000 + Math.floor(Math.random() * 100000);
  const body = JSON.stringify({ target_user_id: targetId, action: 'pass' });
  const res = http.post(`${BASE_URL}/swipe`, body, { headers });
  recordResult(res, [200, 201]);
}

function hitChatHistory() {
  // Read-only, hits Postgres + Redis cache path. Match-id may need to be
  // a real one for the test user; otherwise the server short-circuits to 404.
  const matchId = __ENV.MATCH_ID || '1';
  const res = http.get(`${BASE_URL}/matches/${matchId}/messages?limit=50`, { headers });
  recordResult(res, [200, 404]);
}

function recordResult(res, okStatuses) {
  const accepted = Array.isArray(okStatuses) ? okStatuses : [okStatuses];
  const ok = accepted.includes(res.status);
  check(res, { 'status is acceptable': () => ok });
  routeLatency.add(res.timings.duration);
  routeErrors.add(!ok);
}

const targets = {
  spots_feed:   hitSpotsFeed,
  discover:     hitDiscover,
  swipe:        hitSwipe,
  chat_history: hitChatHistory,
};

export default function () {
  const fn = targets[TARGET];
  if (!fn) {
    throw new Error(`Unknown TARGET '${TARGET}'. Valid: ${Object.keys(targets).join(', ')}`);
  }
  fn();
}
