# NAVA Platform - Kubernetes Deployment

## Architecture

```
                         Internet
                            │
                            ▼
                    ┌───────────────┐
                    │   Ingress     │
                    │   (NGINX)     │
                    └───────┬───────┘
                            │
                            ▼
                    ┌───────────────┐
                    │    Gateway    │
                    │   (2-10 pods) │
                    └───────┬───────┘
                            │
        ┌───────────────────┼───────────────────┐
        │                   │                   │
        ▼                   ▼                   ▼
┌───────────────┐   ┌───────────────┐   ┌───────────────┐
│ Auth Service  │   │ User Service  │   │ Match Service │
│   (2-8 pods)  │   │   (2-8 pods)  │   │  (3-15 pods)  │
└───────────────┘   └───────────────┘   └───────────────┘

┌───────────────┐   ┌───────────────┐   ┌───────────────┐
│ Chat Service  │   │Payment Service│   │Ambassador Svc │
│  (3-20 pods)  │   │   (2-6 pods)  │   │   (2-6 pods)  │
└───────────────┘   └───────────────┘   └───────────────┘
        │                   │                   │
        └───────────────────┼───────────────────┘
                            │
        ┌───────────────────┼───────────────────┐
        │                   │                   │
        ▼                   ▼                   ▼
┌───────────────┐   ┌───────────────┐   ┌───────────────┐
│   PostgreSQL  │   │     Redis     │   │     Neo4j     │
│  (StatefulSet)│   │  (Deployment) │   │  (StatefulSet)│
└───────────────┘   └───────────────┘   └───────────────┘
```

## Directory Structure

```
k8s/
├── base/                    # Base manifests
│   ├── kustomization.yaml
│   ├── namespace.yaml
│   ├── configmap.yaml
│   ├── secrets.yaml
│   ├── postgres.yaml
│   ├── redis.yaml
│   ├── neo4j.yaml
│   ├── gateway.yaml
│   ├── auth-service.yaml
│   ├── user-service.yaml
│   ├── match-service.yaml
│   ├── chat-service.yaml
│   ├── payment-service.yaml
│   ├── ambassador-service.yaml
│   ├── ingress.yaml
│   └── hpa.yaml
│
├── overlays/
│   ├── dev/                 # Development overrides
│   │   └── kustomization.yaml
│   └── prod/                # Production overrides
│       └── kustomization.yaml
│
├── deploy.sh                # Deployment script
└── README.md
```

## Prerequisites

1. **Kubernetes Cluster** (1.25+)
   - GKE, EKS, AKS, or local (minikube/kind)

2. **Tools**
   - kubectl
   - kustomize (or kubectl with -k flag)
   - Docker

3. **Ingress Controller**
   ```bash
   # Install NGINX Ingress
   kubectl apply -f https://raw.githubusercontent.com/kubernetes/ingress-nginx/controller-v1.8.2/deploy/static/provider/cloud/deploy.yaml
   ```

4. **Cert Manager** (for TLS)
   ```bash
   kubectl apply -f https://github.com/cert-manager/cert-manager/releases/download/v1.13.0/cert-manager.yaml
   ```

## Quick Start

### Development

```bash
# Deploy to dev environment
kubectl apply -k overlays/dev

# Or use the script
./deploy.sh dev
```

### Production

```bash
# Deploy to production
kubectl apply -k overlays/prod

# Or use the script
./deploy.sh prod
```

## Configuration

### Update Secrets

Before deploying to production, update the secrets:

```bash
# Create secrets from literal values
kubectl create secret generic nava-secrets \
  --namespace=nava \
  --from-literal=DATABASE_URL='postgres://user:pass@host:5432/db' \
  --from-literal=JWT_SECRET='your-production-secret' \
  --from-literal=RAZORPAY_KEY_ID='your-key' \
  --from-literal=RAZORPAY_KEY_SECRET='your-secret' \
  --dry-run=client -o yaml | kubectl apply -f -
```

### Update ConfigMap

```bash
kubectl edit configmap nava-config -n nava
```

## Scaling

### Manual Scaling

```bash
# Scale a specific service
kubectl scale deployment match-service --replicas=10 -n nava
```

### Horizontal Pod Autoscaler (HPA)

HPA is configured for all services. View status:

```bash
kubectl get hpa -n nava
```

Default scaling rules:
- **Gateway**: 2-10 pods (70% CPU)
- **Auth Service**: 2-8 pods (70% CPU)
- **Match Service**: 3-15 pods (60% CPU, 70% Memory)
- **Chat Service**: 3-20 pods (60% CPU, 70% Memory)

## Monitoring

### Check Pod Status

```bash
kubectl get pods -n nava
kubectl get pods -n nava -w  # Watch mode
```

### View Logs

```bash
# Single pod
kubectl logs -f deployment/gateway -n nava

# All pods of a service
kubectl logs -f -l app=gateway -n nava
```

### Resource Usage

```bash
kubectl top pods -n nava
kubectl top nodes
```

## Troubleshooting

### Pod not starting

```bash
kubectl describe pod <pod-name> -n nava
kubectl logs <pod-name> -n nava --previous
```

### Service connectivity

```bash
# Test from within cluster
kubectl run test --rm -it --image=busybox -n nava -- sh
# Then: wget -qO- http://gateway:8000/health
```

### Database connectivity

```bash
kubectl exec -it postgres-0 -n nava -- psql -U nava -d nava_db
```

## Production Checklist

- [ ] Update secrets with production values
- [ ] Configure proper storage class for PVCs
- [ ] Set up external database (RDS/Cloud SQL) instead of in-cluster
- [ ] Configure proper Ingress with TLS
- [ ] Set up monitoring (Prometheus + Grafana)
- [ ] Configure log aggregation (ELK/Loki)
- [ ] Set up alerting
- [ ] Configure network policies
- [ ] Set up backup for databases
- [ ] Configure Pod Disruption Budgets

## Cloud-Specific Notes

### GKE (Google Kubernetes Engine)

```bash
# Create cluster
gcloud container clusters create nava-cluster \
  --num-nodes=3 \
  --machine-type=e2-standard-4 \
  --region=us-central1

# Get credentials
gcloud container clusters get-credentials nava-cluster --region=us-central1
```

### EKS (Amazon Elastic Kubernetes Service)

```bash
# Create cluster with eksctl
eksctl create cluster \
  --name nava-cluster \
  --region us-east-1 \
  --nodes 3 \
  --node-type t3.large
```

### AKS (Azure Kubernetes Service)

```bash
# Create cluster
az aks create \
  --resource-group nava-rg \
  --name nava-cluster \
  --node-count 3 \
  --node-vm-size Standard_D4s_v3
```
