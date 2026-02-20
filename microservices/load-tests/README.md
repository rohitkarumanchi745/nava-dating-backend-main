# NAVA Load Testing

Load testing configuration for the NAVA dating platform using k6.

## Quick Start

### Prerequisites
- Docker & Docker Compose
- k6 (optional, for local runs)

### Running Load Tests

1. **Start the monitoring stack:**
   ```bash
   docker-compose up -d influxdb grafana
   ```

2. **Run load tests:**
   ```bash
   # Using Docker
   docker-compose run k6

   # Or using local k6
   k6 run k6-config.js
   ```

3. **View results in Grafana:**
   - Open http://localhost:3001
   - Default credentials: admin/admin
   - Navigate to the K6 dashboard

### Test Scenarios

| Scenario | Description | Duration | Max VUs |
|----------|-------------|----------|---------|
| Smoke | Basic functionality check | 1m | 1 |
| Load | Normal traffic simulation | 14m | 100 |
| Stress | Find breaking point | 16m | 400 |
| Spike | Sudden traffic spike | 3m | 500 |

### Configuration

Set environment variables to customize tests:

```bash
# API endpoint
export BASE_URL=http://localhost:8080

# Authentication token
export AUTH_TOKEN=your-jwt-token

# Run specific scenario
k6 run --env SCENARIO=load k6-config.js
```

### Thresholds

The tests have built-in thresholds:

| Metric | Threshold |
|--------|-----------|
| HTTP request duration (p95) | < 500ms |
| HTTP request duration (p99) | < 1500ms |
| HTTP failure rate | < 1% |
| Error rate | < 5% |
| Auth latency (p95) | < 300ms |
| Match latency (p95) | < 200ms |
| Chat latency (p95) | < 100ms |

### Custom Metrics

The tests track custom metrics:

- `errors` - Error rate
- `auth_latency` - Authentication endpoint latency
- `match_latency` - Matching endpoint latency
- `chat_latency` - Chat endpoint latency
- `api_calls` - Total API calls made

### Directory Structure

```
load-tests/
├── k6-config.js          # Main test configuration
├── docker-compose.yml    # Docker stack
├── grafana/
│   ├── dashboards/       # Grafana dashboards
│   └── datasources/      # Grafana datasources
└── README.md
```

### Tips

1. **Warm up your services** before running stress tests
2. **Monitor your infrastructure** during tests (CPU, memory, DB connections)
3. **Run tests from a separate machine** for accurate network measurements
4. **Start with smoke tests** to verify everything works

### Cleanup

```bash
docker-compose down -v
```
