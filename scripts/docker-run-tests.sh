#!/usr/bin/env bash

# NOTE: Run from project root ./scripts/docker-run-tests.sh

docker build -t graphcast-sdk-dev -f Dockerfile.dev .
docker run --rm -v .:/app graphcast-sdk-dev cargo nextest run 
