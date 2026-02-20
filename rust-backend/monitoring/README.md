# Nava Platform Monitoring

This directory contains monitoring configuration for the Nava dating platform backend.

## Components

### 1. Prometheus Alerting Rules (`prometheus-rules.yaml`)

Comprehensive alerting rules covering:

- **API Availability**: Error rates, latency, endpoint health
- **Database Health**: Connection pools, slow queries, replication lag
- **Payment System**: Failure rates, DLQ backlog, gateway health
- **Infrastructure**: CPU, memory, disk, pod health
- **WebSocket**: Connection counts, drop rates
- **Business Metrics**: User activity, match rates, conversion
- **Security**: Rate limiting, suspicious auth, unauthorized access

#### Deploying to Kubernetes

```bash
kubectl create configmap prometheus-rules \
  --from-file=prometheus-rules.yaml \
  -n monitoring
```

#### Adding to Prometheus config

```yaml
rule_files:
  - "/etc/prometheus/rules/prometheus-rules.yaml"
```

### 2. AlertManager Configuration (`alertmanager.yaml`)

Routes alerts to appropriate channels:

- **Critical alerts** → PagerDuty + Slack urgent channels
- **Payment alerts** → #nava-payments-alerts / #nava-payments-urgent
- **Platform alerts** → #nava-platform-alerts / #nava-platform-urgent
- **Security alerts** → #nava-security-alerts + email
- **Growth/business alerts** → #nava-growth-metrics

#### Required Environment Variables

```bash
SLACK_WEBHOOK_URL=https://hooks.slack.com/services/...
PAGERDUTY_PLATFORM_KEY=your-platform-key
PAGERDUTY_PAYMENTS_KEY=your-payments-key
PAGERDUTY_CRITICAL_KEY=your-critical-key
SMTP_SMARTHOST=smtp.example.com:587
```

#### Deploying to Kubernetes

```bash
kubectl create secret generic alertmanager-config \
  --from-file=alertmanager.yaml \
  -n monitoring
```

## Backend Metrics Endpoint

The backend exposes Prometheus metrics at `/metrics`:

```
GET /metrics
```

### Available Metrics

| Metric | Type | Description |
|--------|------|-------------|
| `app_requests_total` | counter | Total HTTP requests |
| `app_requests_active` | gauge | Currently active requests |
| `app_errors_total` | counter | Total errors |
| `app_db_queries_total` | counter | Total database queries |
| `app_cache_hits` | counter | Redis cache hits |
| `app_cache_misses` | counter | Redis cache misses |
| `app_websocket_connections` | gauge | Active WebSocket connections |
| `app_uptime_seconds` | counter | Server uptime |
| `app_db_pool_size` | gauge | Database connection pool size |
| `app_db_pool_idle` | gauge | Idle database connections |
| `app_chat_rooms_active` | gauge | Active chat rooms |
| `app_chat_subscribers_total` | gauge | Total chat subscribers |
| `dlq_entries_pending` | gauge | Pending DLQ entries (by queue) |
| `dlq_entries_resolved_total` | counter | Resolved DLQ entries |
| `dlq_entries_abandoned_total` | counter | Abandoned DLQ entries |

## OpenTelemetry Distributed Tracing

Enable OpenTelemetry support with the `otel` feature:

```bash
cargo build --release --features otel
```

### Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `OTEL_EXPORTER_OTLP_ENDPOINT` | `http://localhost:4317` | OTLP collector endpoint |
| `OTEL_SERVICE_NAME` | `nava-backend` | Service name in traces |
| `OTEL_TRACES_SAMPLER_ARG` | `1.0` | Sampling rate (0.0-1.0) |

### Compatible Backends

- Jaeger
- Zipkin
- AWS X-Ray (via ADOT collector)
- Google Cloud Trace
- DataDog
- Honeycomb
- Grafana Tempo

### Docker Compose Example (Jaeger)

```yaml
services:
  jaeger:
    image: jaegertracing/all-in-one:1.50
    ports:
      - "4317:4317"   # OTLP gRPC
      - "16686:16686" # UI
    environment:
      - COLLECTOR_OTLP_ENABLED=true

  nava-backend:
    build:
      context: .
      args:
        FEATURES: "otel"
    environment:
      - OTEL_EXPORTER_OTLP_ENDPOINT=http://jaeger:4317
      - OTEL_TRACES_SAMPLER_ARG=0.1  # 10% sampling
```

## Kubernetes Deployment

### ServiceMonitor for Prometheus Operator

```yaml
apiVersion: monitoring.coreos.com/v1
kind: ServiceMonitor
metadata:
  name: nava-backend
  namespace: monitoring
spec:
  selector:
    matchLabels:
      app: nava-backend
  endpoints:
    - port: http
      path: /metrics
      interval: 15s
```

### Grafana Dashboard

Import the included dashboard or create one with these queries:

```promql
# Request rate
rate(app_requests_total[5m])

# Error rate
rate(app_errors_total[5m]) / rate(app_requests_total[5m])

# P95 latency (requires histogram)
histogram_quantile(0.95, rate(http_request_duration_seconds_bucket[5m]))

# DLQ backlog
sum(dlq_entries_pending)

# WebSocket connections
app_websocket_connections
```

## Recommended Alert Thresholds

| Alert | Warning | Critical |
|-------|---------|----------|
| Error rate | 2% | 5% |
| P95 latency | 1s | 2s |
| CPU usage | 70% | 85% |
| Memory usage | 75% | 90% |
| DLQ backlog | 50 | 100 |
| Payment failure | 5% | 10% |

## Runbooks

Create runbooks at `https://docs.nava.app/runbooks/` for each critical alert:

- `high-error-rate` - Steps to diagnose high error rates
- `database-connection-pool` - Connection pool exhaustion
- `payment-failures` - Payment gateway issues
- `dlq-backlog` - DLQ processing issues
- `security-alerts` - Security incident response

## Health Endpoints

| Endpoint | Purpose |
|----------|---------|
| `/health` | Basic health check |
| `/health/detailed` | Detailed health with DB/Redis status |
| `/ready` | Kubernetes readiness probe |
| `/live` | Kubernetes liveness probe |
| `/api/payments/dlq/stats` | DLQ statistics (authenticated) |
