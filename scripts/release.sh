#!/usr/bin/env bash

set -e
set -x

VERSION="v$(cargo metadata --quiet --format-version 1 | jq -r '.packages[] | select(.name == "graphcast-sdk") | .version')"

if [[ -z "$VERSION" ]]; then
  echo "Usage: $0 <version>"
  exit 1
fi

git-cliff -o changelog.md

(
  git add changelog.md Cargo.lock Cargo.toml scripts/release.sh \
    && git commit -m "chore: release $VERSION"
) || true

# Publish to crates.io
cargo publish
