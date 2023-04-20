#!/usr/bin/env bash

set -e
set -x

VERSION="v$(cargo metadata --quiet --format-version 1 | jq -r '.packages[] | select(.name == "graphcast-sdk") | .version')"

if [[ -z "$VERSION" ]]; then
  echo "Usage: $0 <version>"
  exit 1
fi

auto-changelog --latest-version VERSION --release-summary

(
  git add CHANGELOG.md \
    && git commit -m "chore: Update changelogs ahead of release"
) || true

# Publish to crates.io
cargo publish
