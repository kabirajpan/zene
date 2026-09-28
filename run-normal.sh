#!/usr/bin/env bash
set -e

# Directory where the command was invoked
CALL_DIR="$(pwd)"

# Resolve script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

# Load .env (checks script dir, parent workspace dir, or ~/.config/zene/.env)
if [ -f "$SCRIPT_DIR/.env" ]; then
    set -a; source "$SCRIPT_DIR/.env"; set +a
elif [ -f "$SCRIPT_DIR/../../.env" ]; then
    set -a; source "$SCRIPT_DIR/../../.env"; set +a
elif [ -f "$HOME/.config/zene/.env" ]; then
    set -a; source "$HOME/.config/zene/.env"; set +a
fi

# Check if user explicitly provided a workspace argument
has_ws=false
for arg in "$@"; do
    if [ "$arg" = "-w" ] || [ "$arg" = "--workspace" ]; then
        has_ws=true
        break
    fi
done

WS_ARGS=()
if [ "$has_ws" = false ]; then
    WS_ARGS=(--workspace "$CALL_DIR")
fi

if [ -f "$SCRIPT_DIR/Cargo.toml" ]; then
    cargo run --manifest-path "$SCRIPT_DIR/Cargo.toml" --bin zene -- "${WS_ARGS[@]}" "$@"
else
    cargo run --bin zene -- "${WS_ARGS[@]}" "$@"
fi
