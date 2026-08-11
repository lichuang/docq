#!/usr/bin/env bash
# Publish a single workspace crate to crates.io, but only if its local version
# differs from the version already published.
#
# Usage:
#   scripts/publish-crate.sh docq-core
#   scripts/publish-crate.sh docq-model --dry-run
#   scripts/publish-crate.sh docq --allow-dirty
#
# Dependency order for the first release (publish from bottom to top).
# Dev-dependencies are also resolved from crates.io during publish, so they
# must be published before the crate that references them.
#
#   1. docq-core
#   2. docq-storage        (dev-dependency of docq-model)
#   3. docq-model          (depends on docq-core; dev-depends on docq-storage)
#   4. docq-indexer        (depends on docq-core, docq-model, docq-storage)
#   5. docq-retrieve       (depends on docq-core, docq-model, docq-storage)
#   6. docq-synth          (depends on docq-core, docq-model, docq-retrieve;
#                         dev-depends on docq-indexer, docq-storage)
#   7. docq                (depends on all of the above)

set -euo pipefail

CRATE=""
DRY_RUN=false
ALLOW_DIRTY=false

while [[ $# -gt 0 ]]; do
  case "$1" in
    --dry-run)
      DRY_RUN=true
      shift
      ;;
    --allow-dirty)
      ALLOW_DIRTY=true
      shift
      ;;
    -h|--help)
      echo "Usage: $0 <crate-name> [--dry-run] [--allow-dirty]"
      exit 0
      ;;
    -*)
      echo "Unknown option: $1" >&2
      echo "Usage: $0 <crate-name> [--dry-run] [--allow-dirty]" >&2
      exit 1
      ;;
    *)
      CRATE="$1"
      shift
      ;;
  esac
done

if [[ -z "$CRATE" ]]; then
  echo "Usage: $0 <crate-name> [--dry-run] [--allow-dirty]" >&2
  exit 1
fi

for tool in cargo curl jq; do
  if ! command -v "$tool" &> /dev/null; then
    echo "Error: required tool '$tool' is not installed" >&2
    exit 1
  fi
done

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
cd "$ROOT"

# Verify the crate is part of the workspace.
if ! cargo metadata --no-deps --format-version 1 \
     | jq -e --arg name "$CRATE" '.packages[] | select(.name == $name)' > /dev/null; then
  echo "Error: crate '$CRATE' is not a member of this workspace" >&2
  exit 1
fi

LOCAL_VERSION=$(cargo metadata --no-deps --format-version 1 \
  | jq -r --arg name "$CRATE" '.packages[] | select(.name == $name) | .version')

echo "crate:  $CRATE"
echo "local:  $LOCAL_VERSION"

REMOTE_VERSION=$(curl -sS \
  -H "User-Agent: docq-publish-script" \
  "https://crates.io/api/v1/crates/$CRATE" 2>/dev/null \
  | jq -r '.crate.max_version // empty' 2>/dev/null || true)

if [[ -z "${REMOTE_VERSION:-}" ]]; then
  echo "remote: <not published yet>"
else
  echo "remote: $REMOTE_VERSION"
fi

if [[ -n "${REMOTE_VERSION:-}" && "$LOCAL_VERSION" == "$REMOTE_VERSION" ]]; then
  echo "Versions are identical. Skipping publish."
  exit 0
fi

PUBLISH_ARGS=(-p "$CRATE")

if $DRY_RUN; then
  PUBLISH_ARGS+=(--dry-run)
fi

if $ALLOW_DIRTY; then
  PUBLISH_ARGS+=(--allow-dirty)
fi

echo "Publishing $CRATE@$LOCAL_VERSION..."
cargo publish "${PUBLISH_ARGS[@]}"
