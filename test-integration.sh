#!/usr/bin/env bash
set -euo pipefail

# docq integration test script
# Usage: cargo build && ./test-integration.sh
#
# Runs against an isolated temporary workspace, so the user's own
# ~/.config/docq data is never touched. Models are read from the shared
# model cache; the first run downloads them.

DOCQ=./target/debug/docq

if [[ ! -x "$DOCQ" ]]; then
  echo "error: $DOCQ not found. Run 'cargo build' first." >&2
  exit 1
fi

WORKSPACE="$(mktemp -d)"
trap 'rm -rf "$WORKSPACE"' EXIT

GREEN='\033[0;32m'
RESET='\033[0m'

echo -e "\n${GREEN}=== Init (workspace: $WORKSPACE) ===${RESET}"
"$DOCQ" --workspace "$WORKSPACE" init

echo -e "\n${GREEN}=== Add collection ===${RESET}"
"$DOCQ" --workspace "$WORKSPACE" add ./testdata/md --name notes

echo -e "\n${GREEN}=== Index ===${RESET}"
"$DOCQ" --workspace "$WORKSPACE" index -v --log-stdout

echo -e "\n${GREEN}=== Ask ===${RESET}"
"$DOCQ" --workspace "$WORKSPACE" ask "What are the improvements of Multi-Paxos over the Paxos algorithm?" -vv

echo -e "\n${GREEN}=== Search ===${RESET}"
"$DOCQ" --workspace "$WORKSPACE" search "Multi-Paxos" --explain

echo -e "\n${GREEN}=== Status ===${RESET}"
"$DOCQ" --workspace "$WORKSPACE" status

echo -e "\n${GREEN}=== Done ===${RESET}"
