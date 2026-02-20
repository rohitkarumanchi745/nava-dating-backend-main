#!/bin/bash

# NAVA Platform - Kubernetes Deployment Script

set -e

ENVIRONMENT=${1:-dev}
NAMESPACE="nava"

if [ "$ENVIRONMENT" == "dev" ]; then
    NAMESPACE="nava-dev"
fi

echo "========================================"
echo "  NAVA Platform Deployment"
echo "  Environment: $ENVIRONMENT"
echo "  Namespace: $NAMESPACE"
echo "========================================"

# Check if kubectl is installed
if ! command -v kubectl &> /dev/null; then
    echo "Error: kubectl is not installed"
    exit 1
fi

# Check cluster connection
if ! kubectl cluster-info &> /dev/null; then
    echo "Error: Cannot connect to Kubernetes cluster"
    exit 1
fi

echo ""
echo "Step 1: Building images..."
echo "----------------------------------------"

# Build all service images
SERVICES=("gateway" "auth-service" "user-service" "match-service" "chat-service" "payment-service" "ambassador-service")

for service in "${SERVICES[@]}"; do
    echo "Building nava/$service..."
    if [ "$service" == "gateway" ]; then
        docker build -t nava/$service:latest -f gateway/Dockerfile .
    else
        docker build -t nava/$service:latest -f services/$service/Dockerfile .
    fi
done

echo ""
echo "Step 2: Applying Kubernetes manifests..."
echo "----------------------------------------"

# Apply using kustomize
kubectl apply -k overlays/$ENVIRONMENT

echo ""
echo "Step 3: Waiting for deployments..."
echo "----------------------------------------"

# Wait for all deployments to be ready
for service in "${SERVICES[@]}"; do
    if [ "$ENVIRONMENT" == "dev" ]; then
        deployment_name="dev-$service"
    else
        deployment_name="$service"
    fi
    echo "Waiting for $deployment_name..."
    kubectl rollout status deployment/$deployment_name -n $NAMESPACE --timeout=300s
done

echo ""
echo "Step 4: Checking pod status..."
echo "----------------------------------------"

kubectl get pods -n $NAMESPACE

echo ""
echo "========================================"
echo "  Deployment Complete!"
echo "========================================"
echo ""
echo "Access the API:"
if [ "$ENVIRONMENT" == "dev" ]; then
    echo "  https://dev-api.nava.app"
else
    echo "  https://api.nava.app"
fi
echo ""
echo "Useful commands:"
echo "  kubectl get pods -n $NAMESPACE"
echo "  kubectl logs -f deployment/gateway -n $NAMESPACE"
echo "  kubectl get hpa -n $NAMESPACE"
