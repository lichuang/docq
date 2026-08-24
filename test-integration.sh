#!/usr/bin/env bash
set -euo pipefail

# docq integration test script
# Usage: ./test-integration.sh

DOCQ=./target/debug/docq
WORKSPACE="$HOME/.config/docq"

GREEN='\033[0;32m'
RESET='\033[0m'

echo -e "\n${GREEN}=== Clean workspace ===${RESET}"
rm -rf "$WORKSPACE"
mkdir -p "$WORKSPACE"

echo -e "\n${GREEN}=== Init ===${RESET}"
"$DOCQ" init

echo -e "\n${GREEN}=== Add collection ===${RESET}"
"$DOCQ" add ./testdata/md --name notes

echo -e "\n${GREEN}=== Index ===${RESET}"
"$DOCQ" index -v --log-stdout

echo -e "\n${GREEN}=== Ask ===${RESET}"
"$DOCQ" ask "What are the improvements of Multi-Paxos over the Paxos algorithm?" -vv

echo -e "\n${GREEN}=== Search ===${RESET}"
"$DOCQ" search "Multi-Paxos" --explain

echo -e "\n${GREEN}=== Status ===${RESET}"
"$DOCQ" status

echo -e "\n${GREEN}=== Done ===${RESET}"