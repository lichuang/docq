#!/usr/bin/env bash
set -euo pipefail

# docq integration test script
# Usage: cargo build && ./test-integration.sh
#
# Runs with a temporary workspace and user config.
# Models use the shared cache. The first run downloads them.

# Windows docq uses LocalAppData through the Known Folder API.
# HOME and XDG_CONFIG_HOME cannot isolate that config path.
case "$(uname -s)" in
  CYGWIN* | MINGW* | MSYS*)
    echo "error: test-integration.sh supports Unix-like docq binaries only." >&2
    exit 1
    ;;
esac

DOCQ=./target/debug/docq

if [[ ! -x "$DOCQ" ]]; then
  echo "error: $DOCQ not found. Run 'cargo build' first." >&2
  exit 1
fi

TEST_ROOT="$(mktemp -d)"
trap 'rm -rf "$TEST_ROOT"' EXIT

WORKSPACE="$TEST_ROOT/workspace"

# Save this path before HOME changes.
MODEL_CACHE="${DOCQ_MODEL_CACHE:-$HOME/.cache/docq/models}"

export HOME="$TEST_ROOT/home"
export XDG_CONFIG_HOME="$TEST_ROOT/config"
mkdir -p "$HOME" "$XDG_CONFIG_HOME"

DOCQ_ARGS=(
  --workspace "$WORKSPACE"
  --model-cache "$MODEL_CACHE"
)

GREEN='\033[0;32m'
RESET='\033[0m'

echo -e "\n${GREEN}=== Init (workspace: $WORKSPACE) ===${RESET}"
"$DOCQ" "${DOCQ_ARGS[@]}" init

echo -e "\n${GREEN}=== Add collection ===${RESET}"
"$DOCQ" "${DOCQ_ARGS[@]}" add ./testdata/md --name notes

echo -e "\n${GREEN}=== Index ===${RESET}"
"$DOCQ" "${DOCQ_ARGS[@]}" index -v --log-stdout

echo -e "\n${GREEN}=== Ask ===${RESET}"
"$DOCQ" "${DOCQ_ARGS[@]}" ask "What are the improvements of Multi-Paxos over the Paxos algorithm?" -vv

echo -e "\n${GREEN}=== Search ===${RESET}"
"$DOCQ" "${DOCQ_ARGS[@]}" search "Multi-Paxos" --explain

echo -e "\n${GREEN}=== Status ===${RESET}"
"$DOCQ" "${DOCQ_ARGS[@]}" status

echo -e "\n${GREEN}=== Done ===${RESET}"
