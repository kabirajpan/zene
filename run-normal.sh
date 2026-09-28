#!/usr/bin/env bash
set -e

# Directory where the command was invoked
CALL_DIR="$(pwd)"

# Resolve directories
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Load .env (checks crates/agent/.env, then apps/zenthree/.env)
if [ -f "$SCRIPT_DIR/.env" ]; then
    export $(grep -v '^#' "$SCRIPT_DIR/.env" | xargs -d '\n')
elif [ -f "$ROOT_DIR/.env" ]; then
    export $(grep -v '^#' "$ROOT_DIR/.env" | xargs -d '\n')
fi

# Check if user explicitly provided a workspace argument
has_ws=false
for arg in "$@"; do
    if [ "$arg" = "-w" ] || [ "$arg" = "--workspace" ]; then
        has_ws=true
        break
    fi
done

cd "$ROOT_DIR"
if [ "$has_ws" = true ]; then
    cargo run -p agent --bin zene-agent -- "$@"
else
    cargo run -p agent --bin zene-agent -- --workspace "$CALL_DIR" "$@"
fi
