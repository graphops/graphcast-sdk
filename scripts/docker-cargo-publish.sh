#!/usr/bin/env bash

# NOTE: Run from project root ./scripts/docker-cargo-publish.sh

docker build -t graphcast-sdk-dev -f Dockerfile.dev .
docker run --rm -e CARGO_REGISTRY_TOKEN=$CARGO_REGISTRY_TOKEN -v .:/app graphcast-sdk-dev cargo publish
