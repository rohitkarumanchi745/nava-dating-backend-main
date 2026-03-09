# NAVA Platform — Operations Runbook

## Table of Contents
1. [K8s Secret Rotation](#k8s-secret-rotation)
2. [SLO Definitions](#slo-definitions)
3. [Alert Response Playbooks](#alert-response-playbooks)
4. [ML Fallback Monitoring](#ml-fallback-monitoring)
5. [Vision/LLM Service Degradation](#visionllm-service-degradation)
6. [Notification Policy Operations](#notification-policy-operations)
7. [PgBouncer Operations](#pgbouncer-operations)
8. [Read Replica Operations](#read-replica-operations)
9. [Write Scaling Roadmap](#write-scaling-roadmap)
10. [External Exporters](#external-exporters)

---

## K8s Secret Rotation

### How It Works
Secrets use the `SECRET_KEY_FILE` pattern: the app reads from a mounted file first, falling back to the env var. This allows rotation without redeploying container images.

**Config keys that support file-based rotation:**
- `SECRET_KEY_FILE` → JWT signing key
- `RAZORPAY_KEY_SECRET_FILE` → Razorpay API secret
- `RAZORPAY_WEBHOOK_SECRET_FILE` → Razorpay webhook verification
- `STRIPE_SECRET_KEY_FILE` → Stripe API secret
- `STRIPE_WEBHOOK_SECRET_FILE` → Stripe webhook verification

### Rotation Procedure
```bash
# 1. Update the K8s Secret
kubectl create secret generic nava-secrets \
  --from-literal=secret-key="$(openssl rand -base64 32)" \
  --dry-run=client -o yaml | kubectl apply -f -

# 2. Verify pending changes (non-destructive)
curl -s https://nava-api/admin/secrets/status | jq .

# 3. Rolling restart to pick up new files
kubectl rollout restart deployment/nava-api

# 4. Monitor rollout
kubectl rollout status deployment/nava-api --timeout=120s

# 5. Verify health after restart
curl -s https://nava-api/health/detailed | jq .status
```

### Important Notes
- Secrets are applied on pod restart only — the app does NOT hot-reload secrets
- The `/admin/secrets/status` endpoint shows which secrets would change on restart
- Always rotate in staging first before production
- JWT `SECRET_KEY` rotation will invalidate all existing user tokens — coordinate with client teams

---

## SLO Definitions

| SLO | Target | Metric | Alert Threshold | Burn Window |
|-----|--------|--------|----------------|-------------|
| Availability | 99.9% (43 min/month) | `nava:http_error_rate:5m` | > 0.1% for 5m | 5m |
| API Latency | p99 < 500ms | `http_request_duration_seconds` | > 500ms for 5m | 5m |
| Discover Latency | p99 < 200ms | `http_request_duration_seconds{path="/discover"}` | > 200ms for 5m | 5m |
| DB Pool | < 80% utilization | `nava:db_pool_utilization` | > 80% for 3m | 3m |
| Payment DLQ | < 50 pending | `dlq_entries_pending{queue="payments"}` | > 50 for 10m | 10m |
| ML Scoring | avg < 10ms | `app_ml_avg_scoring_latency_us` | > 10000us for 5m | 5m |
| ML Fallback Rate | < 5% | `app_ml_fallback_total / app_discover_requests_total` | > 5% for 5m | 5m |
| WebSocket Capacity | < 8000 connections | `app_websocket_connections` | > 8000 for 5m | 5m |
| Notif Throttle Rate | < 40% blocked | `notif_blocked_* / notif_sent_total` | > 40% for 15m | 15m |
| Notif Engagement | > 5% open rate | `notif_engagement_success / total` | < 5% for 2h | 2h |

**SLO alert rules are defined in:** `deploy/slo-alerts.yml`

---

## Alert Response Playbooks

### HighErrorRate (critical)
**Trigger:** Error rate > 0.1% for 5 minutes

1. Check `/health/detailed` for component status
2. Review recent deployments: `kubectl rollout history deployment/nava-api`
3. Check DB connectivity: `kubectl exec -it <pod> -- curl localhost:3000/ready`
4. Check application logs: `kubectl logs -l app=nava-api --tail=100`
5. If caused by a bad deploy: `kubectl rollout undo deployment/nava-api`

### ServiceUnhealthy (critical)
**Trigger:** Health endpoint returning non-200 for 2+ minutes

1. Check pod status: `kubectl get pods -l app=nava-api`
2. Check DB pool: is `pool_idle` at 0 in `/health/detailed`?
3. If DB is down, check PostgreSQL: `kubectl logs -l app=postgres --tail=50`
4. If Redis is down, the app degrades gracefully — check if Redis recovery is needed

### HighApiLatency / HighDiscoverLatency (warning)
**Trigger:** p99 > 500ms (API) or > 200ms (discover) for 5 minutes

1. Check DB pool saturation in `/metrics` — look at `app_db_pool_idle`
2. Check for slow queries: `SELECT * FROM pg_stat_activity WHERE state = 'active' AND query_start < now() - interval '5 seconds'`
3. Check ML scoring latency: `app_ml_avg_scoring_latency_us` in `/metrics`
4. Check replica lag: if reads are falling back to primary, it doubles primary load

### DbPoolSaturated / DbPoolExhausted (warning/critical)
**Trigger:** Pool > 80% utilized for 3 minutes / zero idle connections for 1 minute

1. Check active queries: `SELECT count(*), state FROM pg_stat_activity GROUP BY state`
2. Look for long-running queries: `SELECT pid, now() - query_start as duration, query FROM pg_stat_activity WHERE state = 'active' ORDER BY duration DESC LIMIT 5`
3. If pool exhausted, consider increasing `DB_MAX_CONNECTIONS` (config) or scaling read replicas
4. Check PgBouncer stats if using connection pooling: `psql -p 6432 pgbouncer -c "SHOW POOLS"`

### PaymentDlqBacklog / PaymentDlqAbandoned (warning/critical)
**Trigger:** > 50 pending DLQ entries for 10m / > 5 abandoned in last hour

1. Check DLQ stats: `curl -s https://nava-api/api/payments/dlq/stats | jq .`
2. Check payment gateway status (Razorpay/Stripe status pages)
3. Review DLQ entries for common error patterns
4. For abandoned entries: manual review required — these are permanent failures

### MlFallbackRateHigh / MlFallbackRateCritical (warning/critical)
**Trigger:** > 5% / > 20% of discover requests using attractiveness fallback

See [ML Fallback Monitoring](#ml-fallback-monitoring) below.

### ReplicaLagHigh / ReplicaUnhealthy (warning/critical)
**Trigger:** Replica lag > 2s for 1 minute / replica unhealthy for 5 minutes

1. Check replica lag: `app_replica_lag_ms` in `/metrics`
2. On the replica: `SELECT now() - pg_last_xact_replay_timestamp() AS lag`
3. Check WAL replay: `SELECT * FROM pg_stat_wal_receiver` on replica
4. Check network between primary and replica
5. If lag is sustained, check `max_standby_streaming_delay` in postgresql.conf (default: 100ms)
6. All reads automatically fall back to primary when lag > 2s — no user impact, but primary load increases

### PgBouncerPoolExhausted / PgBouncerClientWaiting (critical/warning)
**Trigger:** Server pool > 90% / > 50 clients waiting

1. Check PgBouncer stats: `psql -p 6432 pgbouncer -c "SHOW POOLS"`
2. Check for connection leaks: `psql -p 6432 pgbouncer -c "SHOW CLIENTS"` — look for long-lived active connections
3. Increase `default_pool_size` in pgbouncer.ini if server connections are available
4. Check PostgreSQL `max_connections` — PgBouncer can't exceed this

---

## ML Fallback Monitoring

### What the Fallback Means
The discover endpoint uses RL-based ranking (`rank_candidates`) with a 2-second timeout. When the timeout fires, candidates are sorted by `attractiveness_score` instead. This is safe but produces lower-quality rankings.

### Metrics
- `app_ml_fallback_total` — counter of discover requests that hit the 2s timeout
- `app_discover_requests_total` — counter of all discover requests
- **Fallback rate** = `rate(app_ml_fallback_total[5m]) / rate(app_discover_requests_total[5m])`

### Investigation Steps
1. Check `app_ml_avg_scoring_latency_us` — is average scoring latency approaching 2s (2,000,000 us)?
2. Check for RwLock contention: if many concurrent discover requests, the `state.ml.write().await` lock serializes them. High concurrency → queuing → timeouts.
3. Check model size: the RL agent's scoring time scales with the number of arms/features. If the model has grown, consider pruning.
4. Check CPU usage on the pod — ML scoring is CPU-bound.

### Remediation
- **Short-term:** Scale horizontally (more pods) to reduce per-pod concurrency
- **Medium-term:** Move ML scoring to a read-only snapshot (RwLock → read lock) if possible
- **Long-term:** Offload ML scoring to a sidecar or dedicated service with pre-computed scores

---

## Notification Policy Operations

### Architecture
The notification service (`microservices/services/notification-service/`) gates every notification through a policy layer before delivery. The policy enforces:
- **Per-user daily cap** (default: 12/day)
- **Cooldown** (default: 5 minutes between sends to same user)
- **Quiet hours** (default: 22:00–07:00 in user's local time)
- **Send-time optimization** (defers if user's current-hour activity < 0.3)
- **Thompson Sampling bandit** for variant selection on match and re-engage notifications

### Database Tables

| Table | Purpose |
|-------|---------|
| `notification_outcomes` | Logs every sent notification: variant_id, bandit_selected, sent_at_hour, engaged |
| `notification_preferences` | Per-user opt-out by category (or `category='all'` for global opt-out) |

Tables are auto-created on startup via `ensure_policy_tables()`.

### Metrics (Prometheus)
Exported at `notification-service:8007/metrics`:

| Metric | Type | Description |
|--------|------|-------------|
| `notif_sent_total` | counter | Notifications that passed the gate |
| `notif_blocked_cap` | counter | Blocked by daily cap |
| `notif_blocked_cooldown` | counter | Blocked by cooldown |
| `notif_blocked_optout` | counter | Blocked by user opt-out |
| `notif_deferred_quiet` | counter | Deferred to after quiet hours |
| `notif_deferred_timing` | counter | Deferred due to low-activity hour |
| `notif_engagement_success` | counter | Notification opens/clicks |
| `notif_engagement_failure` | counter | Notification ignores |
| `notif_variant_sends{variant}` | gauge | Per-variant send count |
| `notif_variant_expected_rate{variant}` | gauge | Per-variant expected open rate |

### Shadow Mode (Bandit A/B Safety Net)
The bandit can run in **shadow mode** where it selects a variant via Thompson Sampling but always sends the default (control) copy. This is controlled by `PolicyConfig::bandit_shadow_mode`.

**Rollout procedure:**
1. Deploy with `bandit_shadow_mode: true` (default)
2. Monitor `notif_variant_sends` and `notif_variant_expected_rate` in Grafana
3. After 1-2 weeks, verify the bandit has converged (one variant clearly ahead)
4. Compare engagement rates between shadow-selected variants using the `notification_outcomes` table:
   ```sql
   SELECT variant_id,
          COUNT(*) AS sends,
          COUNT(*) FILTER (WHERE engaged) AS opens,
          ROUND(COUNT(*) FILTER (WHERE engaged)::numeric / COUNT(*)::numeric, 4) AS open_rate
   FROM notification_outcomes
   WHERE sent_at > NOW() - INTERVAL '7 days'
   GROUP BY variant_id
   ORDER BY open_rate DESC;
   ```
5. If the bandit-preferred variant shows meaningful lift (>10% relative), set `bandit_shadow_mode: false`
6. Monitor `NotifEngagementLow` and `NotifVariantSkew` alerts for 48 hours post-enable

### Alert Response

**NotifThrottleRateHigh** — Over 40% blocked/deferred for 15m:
1. Check which gate is firing most: `curl notification-service:8007/metrics | grep notif_blocked`
2. If `notif_blocked_cap` is dominant, consider raising `daily_cap` from 12
3. If `notif_deferred_quiet` is dominant, check if quiet hours are too wide for your user base
4. If `notif_blocked_cooldown` is dominant, reduce `cooldown_secs` (default: 300)

**NotifOptOutRateHigh** — Opt-out rate > 10% for 30m:
1. Check which category is driving opt-outs:
   ```sql
   SELECT category, COUNT(*) FROM notification_preferences
   WHERE opted_out = TRUE AND updated_at > NOW() - INTERVAL '24 hours'
   GROUP BY category ORDER BY count DESC;
   ```
2. If `re_engage` is highest, reduce re-engagement frequency
3. Consider user fatigue — may need to lower daily cap

**NotifVariantSkew** — One variant getting >80% of sends:
1. Check bandit stats: `curl notification-service:8007/metrics | grep notif_variant`
2. If the dominant variant genuinely has higher open rates, this is expected convergence
3. If it's early in deployment (low send counts), the bandit may have locked on too fast — consider resetting priors by restarting the service

**NotifSendRateZero** — No sends for 30m:
1. Check notification service health: `curl notification-service:8007/health`
2. Check Kafka consumer groups: `kafka-consumer-groups.sh --describe --group notification-consumer-group`
3. Check database connectivity from the service pod

### Timezone Resolution
User UTC offset is resolved in this order:
1. **Device-reported offset** — from `user_devices.utc_offset_hours` (set during push registration)
2. **Country code** — from `users.country_code`, mapped to approximate offset (e.g., `IN` → +5, `US` → -5)
3. **UTC (0)** — global fallback

To improve accuracy, ensure the mobile app sends `utc_offset_hours` during device registration.

---

## PgBouncer Operations

### Config Location
- `deploy/pgbouncer.ini` — main config
- Runs on port 6432, proxies to PostgreSQL on 5432

### Key Settings
| Setting | Value | Purpose |
|---------|-------|---------|
| `pool_mode` | transaction | Connection returned after each transaction |
| `default_pool_size` | 50 | Server connections per database |
| `max_client_conn` | 1000 | Max app connections accepted |
| `server_reset_query` | DISCARD ALL | Cleans session state between clients |
| `max_prepared_statements` | 0 | Disabled (required for transaction pooling) |

### Common Commands
```bash
# Connect to PgBouncer admin console
psql -p 6432 pgbouncer

# Show pool status
SHOW POOLS;

# Show connected clients
SHOW CLIENTS;

# Show server connections
SHOW SERVERS;

# Show statistics
SHOW STATS;

# Gracefully disconnect idle clients
KILL nava;

# Reload config without restart
RELOAD;
```

### Important: statement_timeout
Session-level `SET statement_timeout` does NOT work with transaction pooling (the SET is lost when the connection returns to the pool). Statement timeout is configured in `postgresql.conf` instead (default: 30s). Migrations that need longer timeouts should use `SET LOCAL statement_timeout` within a transaction.

---

## Read Replica Operations

### Architecture
- Primary pool: `state.db` — all writes, fallback reads
- Replica pool: `state.db_read` — read-heavy queries (discover, matches, history)
- Background task checks replica lag every 5 seconds via `pg_last_xact_replay_timestamp()`
- If lag > 2 seconds, `replica_healthy` flag is set to false and all reads fall back to primary

### Monitoring
- `app_replica_lag_ms` — current replica lag in milliseconds
- `app_replica_healthy` — 1 if healthy, 0 if degraded
- `app_reads_from_replica` — counter of reads served by replica
- `app_reads_fallback_to_primary` — counter of reads that fell back

### Scaling Reads
To add more read replicas:
1. Set up PostgreSQL streaming replication for the new replica
2. Add the replica to PgBouncer config under `[databases]`
3. Configure the app's `DATABASE_READ_URL` to point to PgBouncer's replica pool
4. Monitor `app_replica_lag_ms` to ensure the new replica is keeping up

---

## Vision/LLM Service Degradation

### Architecture
Vision/LLM is an optional sidecar. When unavailable (`state.vision` is `None`), the two affected endpoints return **503 Service Unavailable** (not 500):
- `POST /vision/analyze` — photo content analysis
- `POST /verify/selfie` — selfie liveness + face match

All other endpoints (discover, matches, chat, payments) are unaffected by vision downtime.

### Metrics
- `app_vision_unavailable_total` — counter of requests that hit the 503 path

### Degradation Behavior
| Component | When Unavailable | User Impact |
|-----------|-----------------|-------------|
| Vision (photo analysis) | 503 on `/vision/analyze` | Photo uploads still work, analysis skipped |
| Vision (selfie verify) | 503 on `/verify/selfie` | Users can't verify, but can still use app |
| ML ranking | 2s timeout, falls back to attractiveness score | Lower-quality discover rankings |
| ML `record_swipe` | Fire-and-forget (`tokio::spawn`) | Zero impact on swipe latency |
| Neo4j (graph) | Dual-write manager queues, falls back to PG-only | No user impact |
| Redis (cache) | App runs without cache, higher DB load | Slightly slower responses |

### Investigation
If `app_vision_unavailable_total` is climbing:
1. Check the vision sidecar pod: `kubectl logs -l app=vision-sidecar --tail=50`
2. Check if the model file is mounted: `kubectl exec <pod> -- ls /models/`
3. Check memory — vision models can OOM on constrained pods

---

## Write Scaling Roadmap

### Current State: Single Primary
All writes go to one PostgreSQL 16 primary. This is the main scaling bottleneck.

**Current mitigations:**
- Read replicas offload all SELECT-heavy paths (discover, matches, history)
- PgBouncer multiplexes 1000 app connections → 50 real PG connections
- Swipes hash-partitioned (8 partitions on `from_user_id`) for write distribution within the table
- Connection pool tuned (300 max, 50 min via configmap)

**Current write capacity estimate:**
- 4vCPU/16GB primary with tuned WAL settings
- ~5,000-8,000 write TPS (swipes + messages + profile updates)
- Adequate for ~50,000 DAU (assuming 100 swipes/day + messaging)

### When to Shard (Triggers)
Monitor these signals — if any persist for a week, start sharding work:
1. `nava:db_pool_utilization` consistently > 60% during peak
2. Write latency p99 > 100ms
3. WAL generation rate > 500MB/min
4. Autovacuum can't keep up (`AutovacuumBehind` alert firing regularly)
5. DAU exceeds 100,000

### Sharding Strategy (Phase 1: Functional Sharding)
Split write-heavy tables to dedicated databases:

| Shard | Tables | Write Pattern |
|-------|--------|---------------|
| **Primary** | users, profiles, preferences, subscriptions | Low-medium write rate |
| **Swipes DB** | swipes (already partitioned) | Highest write rate, upsert-heavy |
| **Messages DB** | messages, conversation_events | High write rate, append-only |
| **Events DB** | interaction_events, analytics | Append-only, time-series pattern |

**Implementation steps:**
1. Add `db_swipes: PgPool` and `db_messages: PgPool` to `AppState`
2. Route swipe writes to swipes DB, message writes to messages DB
3. Keep reads on replica(s) — cross-shard joins happen at app layer
4. Deploy each shard behind its own PgBouncer instance

### Sharding Strategy (Phase 2: Horizontal Sharding by User)
If functional sharding isn't enough:
1. Hash-shard users across N databases by `user_id % N`
2. Add a routing layer that maps `user_id` → shard
3. Cross-shard queries (e.g., discover across all users) use scatter-gather
4. Consider Citus for transparent sharding within PostgreSQL

### What NOT to Do
- Don't shard prematurely — the current single-primary handles 50K+ DAU
- Don't use multi-master replication (conflict resolution is a nightmare for dating app data)
- Don't move to a different database — PostgreSQL with proper tuning scales further than most teams need

---

## External Exporters

The SLO alerts in `deploy/slo-alerts.yml` depend on metrics from three external Prometheus exporters. These must be deployed alongside the application for the corresponding alerts to fire.

### pgbouncer_exporter

**Repository:** https://github.com/prometheus-community/pgbouncer_exporter

**Metrics used by alerts:**
- `pgbouncer_pools_server_active` — active server connections per pool
- `pgbouncer_pools_server_maxconn` — max server connections per pool
- `pgbouncer_pools_client_waiting` — clients queued waiting for a connection

**Alerts that depend on it:** `PgBouncerPoolExhausted`, `PgBouncerClientWaiting` (group `pgbouncer_alerts`)

**Deployment:**
```bash
# Deploy as a sidecar or standalone pod pointing at PgBouncer's admin interface
kubectl apply -f - <<EOF
apiVersion: apps/v1
kind: Deployment
metadata:
  name: pgbouncer-exporter
  namespace: nava-prod
spec:
  replicas: 1
  selector:
    matchLabels:
      app: pgbouncer-exporter
  template:
    metadata:
      labels:
        app: pgbouncer-exporter
    spec:
      containers:
        - name: pgbouncer-exporter
          image: prometheuscommunity/pgbouncer-exporter:latest
          args:
            - --pgBouncer.connectionString=postgres://stats:@pgbouncer:6432/pgbouncer?sslmode=disable
          ports:
            - containerPort: 9127
---
apiVersion: v1
kind: Service
metadata:
  name: pgbouncer-exporter
  namespace: nava-prod
spec:
  selector:
    app: pgbouncer-exporter
  ports:
    - port: 9127
      targetPort: 9127
EOF
```

**Verification:** `curl pgbouncer-exporter:9127/metrics | grep pgbouncer_pools_server_active`

### postgres_exporter

**Repository:** https://github.com/prometheus-community/postgres_exporter

**Metrics used by alerts:**
- `pg_stat_activity_max_tx_duration` — longest running transaction duration (by database)
- `pg_stat_user_tables_n_dead_tup` — dead tuple count per table (autovacuum health)

**Alerts that depend on it:** `StatementTimeoutsRising`, `AutovacuumBehind` (group `query_alerts`)

**Deployment:**
```bash
kubectl apply -f - <<EOF
apiVersion: apps/v1
kind: Deployment
metadata:
  name: postgres-exporter
  namespace: nava-prod
spec:
  replicas: 1
  selector:
    matchLabels:
      app: postgres-exporter
  template:
    metadata:
      labels:
        app: postgres-exporter
    spec:
      containers:
        - name: postgres-exporter
          image: prometheuscommunity/postgres-exporter:latest
          env:
            - name: DATA_SOURCE_NAME
              valueFrom:
                secretKeyRef:
                  name: nava-secrets
                  key: postgres-exporter-dsn
          ports:
            - containerPort: 9187
---
apiVersion: v1
kind: Service
metadata:
  name: postgres-exporter
  namespace: nava-prod
spec:
  selector:
    app: postgres-exporter
  ports:
    - port: 9187
      targetPort: 9187
EOF
```

The DSN secret should use a read-only PostgreSQL role, e.g.: `postgresql://exporter:password@postgres:5432/nava?sslmode=require`

**Verification:** `curl postgres-exporter:9187/metrics | grep pg_stat_user_tables_n_dead_tup`

### blackbox_exporter

**Repository:** https://github.com/prometheus/blackbox_exporter

**Metrics used by alerts:**
- `probe_http_status_code` — HTTP status code returned by the probed target

**Alerts that depend on it:** `ServiceUnhealthy` (group `slo_availability`)

The `nava-health` scrape job in `prometheus.yml` probes the `/health` endpoint. If you switch to using the blackbox exporter for synthetic checks, configure it as follows:

**Deployment:**
```bash
kubectl apply -f - <<EOF
apiVersion: apps/v1
kind: Deployment
metadata:
  name: blackbox-exporter
  namespace: monitoring
spec:
  replicas: 1
  selector:
    matchLabels:
      app: blackbox-exporter
  template:
    metadata:
      labels:
        app: blackbox-exporter
    spec:
      containers:
        - name: blackbox-exporter
          image: prom/blackbox-exporter:latest
          args:
            - --config.file=/etc/blackbox/blackbox.yml
          ports:
            - containerPort: 9115
          volumeMounts:
            - name: config
              mountPath: /etc/blackbox
      volumes:
        - name: config
          configMap:
            name: blackbox-config
---
apiVersion: v1
kind: Service
metadata:
  name: blackbox-exporter
  namespace: monitoring
spec:
  selector:
    app: blackbox-exporter
  ports:
    - port: 9115
      targetPort: 9115
EOF
```

Blackbox config (`blackbox.yml`):
```yaml
modules:
  http_2xx:
    prober: http
    timeout: 5s
    http:
      valid_http_versions: ["HTTP/1.1", "HTTP/2.0"]
      valid_status_codes: [200]
      method: GET
```

**Verification:** `curl "blackbox-exporter:9115/probe?target=http://nava-backend:3000/health&module=http_2xx" | grep probe_http_status_code`

### Quick health check — all exporters

```bash
# Verify all exporter targets are up in Prometheus
curl -s prometheus:9090/api/v1/targets | jq '.data.activeTargets[] | select(.labels.job | test("pgbouncer|postgres|blackbox")) | {job: .labels.job, health: .health}'
```

If any target shows `health: "down"`, the corresponding alert group will not evaluate and SLO violations will go undetected.
