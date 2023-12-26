#!/bin/env sh

# Run shoebill generate and apply
TAG=$(git rev-parse HEAD)
IMAGE="git.badhouseplants.net/allanger/shoebill-operator"
NAMESPACE="test-shoebill-operator"

kubectl create namespace $NAMESPACE
shoebill manifests -i $IMAGE -t $TAG -n $NAMESPACE > /tmp/manifests.yaml
kubectl apply -f /tmp/manifests.yaml
kubectl rollout status -n $NAMESPACE deployment shoebill-controller

kubectl delete -f /tmp/manifests.yaml
kubectl delete namespace $NAMESPACE
