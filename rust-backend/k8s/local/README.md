# NAVA Backend - Local Kubernetes Setup (M1 Mac)

## Hardware Requirements
- **Your Setup**: M1 Mac, 16GB RAM, 256GB Storage ✓
- **Recommended for local K8s**: 8GB RAM minimum, 50GB storage

## Option 1: Docker Desktop Kubernetes (Recommended)

### Enable Kubernetes
1. Open Docker Desktop → Settings → Kubernetes
2. Check "Enable Kubernetes"
3. Click "Apply & Restart"
4. Allocate resources in Settings → Resources:
   - CPUs: 4-6 cores
   - Memory: 8-10 GB (leaves 6-8GB for macOS)
   - Disk: 60GB

### Deploy
```bash
# Set context
kubectl config use-context docker-desktop

# Create namespace
kubectl create namespace nava

# Apply local config
kubectl apply -f k8s/local/
```

## Option 2: Minikube

### Install
```bash
brew install minikube

# Start with resource limits for M1
minikube start \
  --driver=docker \
  --cpus=4 \
  --memory=8192 \
  --disk-size=50g
```

### Deploy
```bash
kubectl apply -f k8s/local/
```

## Option 3: Rancher Desktop

### Install
```bash
brew install --cask rancher
```
- Open Rancher Desktop
- Set Kubernetes version (1.28+)
- Allocate 8GB RAM, 4 CPUs

## Local Resource Allocation

| Component | Memory | CPU | Purpose |
|-----------|--------|-----|---------|
| NAVA Backend (1 pod) | 512MB-1GB | 0.5-1 core | Main app |
| PostgreSQL | 512MB | 0.5 core | Database |
| Redis | 128MB | 0.25 core | Cache |
| Neo4j | 1GB | 0.5 core | Graph DB |
| K8s System | 2GB | 1 core | Control plane |
| **Total** | **~5-6GB** | **~3-4 cores** | |

This leaves ~10GB RAM for macOS and other apps.

## Quick Start Commands

```bash
# 1. Start local databases (if not using K8s for them)
docker-compose -f docker-compose.local.yml up -d

# 2. Apply local K8s config
kubectl apply -f k8s/local/

# 3. Check status
kubectl get pods -n nava

# 4. Port forward to access
kubectl port-forward svc/nava-backend 8080:80 -n nava

# 5. Test
curl http://localhost:8080/health
```

## Differences from Production

| Setting | Production | Local |
|---------|------------|-------|
| Replicas | 3-10 | 1 |
| Memory limit | 2GB | 1GB |
| CPU limit | 2 cores | 1 core |
| HPA | Enabled | Disabled |
| PDB | Enabled | Disabled |
| Network Policy | Strict | Permissive |
| Ingress | NGINX/ALB | NodePort |
