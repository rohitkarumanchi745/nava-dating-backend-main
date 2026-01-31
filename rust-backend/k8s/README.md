# NAVA Backend - Kubernetes Deployment Guide

## Architecture for 10,000+ Concurrent Users

```
                    ┌─────────────────┐
                    │  Ingress/ALB    │
                    │  (SSL + WS)     │
                    └────────┬────────┘
                             │
              ┌──────────────┼──────────────┐
              │              │              │
         ┌────▼────┐   ┌────▼────┐   ┌────▼────┐
         │  Pod 1  │   │  Pod 2  │   │  Pod 3  │
         │ (nava)  │   │ (nava)  │   │ (nava)  │
         └────┬────┘   └────┬────┘   └────┬────┘
              │              │              │
              └──────────────┼──────────────┘
                             │
         ┌───────────────────┼───────────────────┐
         │                   │                   │
    ┌────▼────┐        ┌────▼────┐        ┌────▼────┐
    │ PgBouncer│        │  Redis  │        │  Neo4j  │
    │ (Pool)  │        │(Session)│        │ (Graph) │
    └────┬────┘        └─────────┘        └─────────┘
         │
    ┌────▼────┐
    │PostgreSQL│
    │(Primary) │
    └─────────┘
```

## Prerequisites

- Kubernetes cluster (EKS, GKE, AKS, or self-hosted)
- kubectl configured with cluster access
- NGINX Ingress Controller (or AWS ALB Controller for EKS)
- cert-manager (for automatic TLS certificates)
- Container registry access (Docker Hub, ECR, GCR, etc.)

## Quick Start

### 1. Build and Push Docker Image
```bash
# Build the image
docker build -t nava/backend:latest .

# Tag for your registry (example: AWS ECR)
docker tag nava/backend:latest 123456789.dkr.ecr.us-east-1.amazonaws.com/nava-backend:latest

# Push to registry
docker push 123456789.dkr.ecr.us-east-1.amazonaws.com/nava-backend:latest
```

### 2. Create Namespace
```bash
kubectl create namespace nava
kubectl config set-context --current --namespace=nava
```

### 3. Create Secrets
```bash
# Option 1: From .env file
kubectl create secret generic nava-secrets \
  --from-env-file=../.env.production

# Option 2: From individual values
kubectl create secret generic nava-secrets \
  --from-literal=DATABASE_URL='postgresql://user:pass@host:5432/nava' \
  --from-literal=REDIS_URL='redis://:password@redis:6379' \
  --from-literal=SECRET_KEY='your-64-char-secret-key' \
  --from-literal=S3_ACCESS_KEY='AKIAXXXXXXXX' \
  --from-literal=S3_SECRET_KEY='your-secret-key'
```

### 4. Deploy All Resources
```bash
# Apply in order (ConfigMap -> Secrets -> PVC -> Deployment)
kubectl apply -f configmap.yaml
kubectl apply -f pvc.yaml
kubectl apply -f pdb.yaml
kubectl apply -f network-policy.yaml
kubectl apply -f deployment.yaml
kubectl apply -f ingress.yaml

# Or apply all at once
kubectl apply -f .
```

### 5. Verify Deployment
```bash
# Check pods are running
kubectl get pods -l app=nava-backend

# Check HPA status
kubectl get hpa nava-backend-hpa

# Check service endpoints
kubectl get endpoints nava-backend

# View logs
kubectl logs -l app=nava-backend --tail=100 -f
```

## Scaling

### Manual Scaling
```bash
# Scale to 5 replicas
kubectl scale deployment nava-backend --replicas=5

# Scale to 10 replicas for peak traffic
kubectl scale deployment nava-backend --replicas=10
```

### Auto-Scaling (HPA)
The HPA automatically scales based on:
- CPU utilization > 70%
- Memory utilization > 80%

```bash
# Check HPA status
kubectl get hpa nava-backend-hpa

# Watch scaling events
kubectl describe hpa nava-backend-hpa

# View scaling history
kubectl get events --field-selector reason=SuccessfulRescale
```

### Scaling Configuration
Edit `deployment.yaml` to adjust HPA limits:
```yaml
spec:
  minReplicas: 3   # Minimum pods (always running)
  maxReplicas: 20  # Maximum pods (during peak)
```

## Capacity Planning

| Replicas | Concurrent Users | CPU (total) | Memory (total) | Cost (approx) |
|----------|------------------|-------------|----------------|---------------|
| 3        | ~3,000           | 1.5-6 cores | 1.5-6 GB       | $150/mo       |
| 5        | ~5,000           | 2.5-10 cores| 2.5-10 GB      | $250/mo       |
| 10       | ~10,000          | 5-20 cores  | 5-20 GB        | $500/mo       |
| 15       | ~15,000          | 7.5-30 cores| 7.5-30 GB      | $750/mo       |
| 20       | ~20,000          | 10-40 cores | 10-40 GB       | $1000/mo      |

*Costs vary by cloud provider and instance types*

## Health Checks

| Endpoint | Purpose | Used By |
|----------|---------|---------|
| `/health` | Basic health | General monitoring |
| `/health/detailed` | Detailed status (DB, Redis, Neo4j) | Dashboards |
| `/ready` | Readiness probe | K8s (traffic routing) |
| `/live` | Liveness probe | K8s (restart decision) |
| `/metrics` | Prometheus metrics | Monitoring stack |

### Test Health Endpoints
```bash
# Port-forward for testing
kubectl port-forward svc/nava-backend 8080:80

# Test endpoints
curl http://localhost:8080/health
curl http://localhost:8080/health/detailed
curl http://localhost:8080/ready
curl http://localhost:8080/metrics
```

## Configuration

### ConfigMap (Non-sensitive)
Edit `configmap.yaml` to change:
- Rate limits
- Feature flags
- Discovery settings
- WebSocket buffer sizes

### Secrets (Sensitive)
Update secrets with:
```bash
# Update a single secret value
kubectl patch secret nava-secrets -p='{"stringData":{"SECRET_KEY":"new-key"}}'

# Replace entire secret
kubectl create secret generic nava-secrets \
  --from-env-file=../.env.production \
  --dry-run=client -o yaml | kubectl apply -f -
```

## Troubleshooting

### Pod not starting
```bash
# Check pod events
kubectl describe pod <pod-name>

# Check previous container logs
kubectl logs <pod-name> --previous

# Check resource limits
kubectl top pods -l app=nava-backend
```

### Database connection issues
```bash
# Exec into pod for debugging
kubectl exec -it <pod-name> -- /bin/bash

# Test database connectivity (from inside pod)
curl -v pgbouncer:6432

# Check secrets are mounted
kubectl exec -it <pod-name> -- env | grep DATABASE
```

### WebSocket issues
Ensure ingress has WebSocket annotations:
```yaml
nginx.ingress.kubernetes.io/proxy-read-timeout: "3600"
nginx.ingress.kubernetes.io/proxy-send-timeout: "3600"
nginx.ingress.kubernetes.io/websocket-services: "nava-backend"
```

### High Memory Usage
```bash
# Check memory usage
kubectl top pods -l app=nava-backend

# View memory metrics
kubectl exec -it <pod-name> -- cat /proc/meminfo

# Increase limits if needed
kubectl patch deployment nava-backend -p '{"spec":{"template":{"spec":{"containers":[{"name":"nava-backend","resources":{"limits":{"memory":"4Gi"}}}]}}}}'
```

### Rolling Back Deployment
```bash
# View rollout history
kubectl rollout history deployment/nava-backend

# Rollback to previous version
kubectl rollout undo deployment/nava-backend

# Rollback to specific revision
kubectl rollout undo deployment/nava-backend --to-revision=2
```

## Monitoring

### Prometheus Metrics
The backend exposes metrics at `/metrics`:
- `app_requests_total` - Total requests
- `app_requests_active` - Active requests
- `app_errors_total` - Error count
- `app_db_queries_total` - Database queries
- `app_websocket_connections` - WebSocket connections
- `app_cache_hits` / `app_cache_misses` - Cache performance

### Grafana Dashboard
Import the provided dashboard or create alerts for:
- High error rate (> 5%)
- High latency (p99 > 500ms)
- Pod restarts
- HPA scaling events

### Sample Prometheus Alert Rules
```yaml
groups:
- name: nava-backend
  rules:
  - alert: HighErrorRate
    expr: rate(app_errors_total[5m]) / rate(app_requests_total[5m]) > 0.05
    for: 5m
    labels:
      severity: critical
  - alert: HighLatency
    expr: histogram_quantile(0.99, rate(app_request_duration_seconds_bucket[5m])) > 0.5
    for: 5m
    labels:
      severity: warning
```

## Production Checklist

### Security
- [ ] Secrets are stored securely (not in Git)
- [ ] TLS certificates configured
- [ ] Network policies applied
- [ ] Pod security policies enabled
- [ ] RBAC configured

### Infrastructure
- [ ] PgBouncer deployed for connection pooling
- [ ] Redis cluster for session/rate limiting
- [ ] Neo4j cluster for graph queries
- [ ] S3 + CloudFront for media storage

### Monitoring
- [ ] Prometheus + Grafana deployed
- [ ] Alerting configured
- [ ] Log aggregation (ELK/Loki)
- [ ] APM (Datadog/New Relic) optional

### Operations
- [ ] Backup strategy for databases
- [ ] Disaster recovery plan
- [ ] CI/CD pipeline configured
- [ ] Canary/Blue-green deployment strategy

## Cloud-Specific Notes

### AWS EKS
```bash
# Use AWS ALB Ingress Controller
kubectl apply -f https://raw.githubusercontent.com/kubernetes-sigs/aws-load-balancer-controller/main/docs/install/iam_policy.json

# Use EFS for ReadWriteMany PVCs
kubectl apply -k "github.com/kubernetes-sigs/aws-efs-csi-driver/deploy/kubernetes/overlays/stable/?ref=master"
```

### Google GKE
```bash
# Enable Filestore for ReadWriteMany
gcloud services enable file.googleapis.com

# Use Cloud CDN for static assets
gcloud compute backend-buckets create nava-media --gcs-bucket-name=nava-media
```

### Azure AKS
```bash
# Use Azure Files for ReadWriteMany
kubectl apply -f https://raw.githubusercontent.com/kubernetes-sigs/azurefile-csi-driver/master/deploy/example/storageclass-azurefile-csi.yaml
```

## File Structure
```
k8s/
├── README.md           # This file
├── configmap.yaml      # Non-sensitive configuration
├── secrets.yaml        # Template for secrets (DO NOT COMMIT REAL VALUES)
├── pvc.yaml            # Persistent volume claims
├── pdb.yaml            # Pod disruption budget
├── network-policy.yaml # Network security policies
├── deployment.yaml     # Deployment + Service + HPA
└── ingress.yaml        # Ingress configuration
```
