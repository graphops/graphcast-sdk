#!/usr/bin/env bash

set -e
set -x

VERSION="$1"

if [[ -z "$VERSION" ]]; then
  echo "Usage: $0 <version>"
  exit 1
fi

chan release --allow-prerelease "$VERSION" || true

(
  git add CHANGELOG.md \
    && git commit -m "chore: Update changelogs ahead of release"
) || true

# Publish to crates.io
cargo publish
