#!/usr/bin/env bash
# Quick Diagnostic Tool: Test all API keys and Model endpoints for ZENE Agent

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT_DIR="$(cd "$SCRIPT_DIR/../.." && pwd)"

# Load .env (checks crates/agent/.env, then apps/zenthree/.env)
if [ -f "$SCRIPT_DIR/.env" ]; then
    set -a
    source "$SCRIPT_DIR/.env"
    set +a
elif [ -f "$ROOT_DIR/.env" ]; then
    set -a
    source "$ROOT_DIR/.env"
    set +a
fi

BOLD="\033[1m"
GREEN="\033[32m"
RED="\033[31m"
YELLOW="\033[33m"
CYAN="\033[36m"
DIM="\033[2m"
RESET="\033[0m"

echo -e "${BOLD}${CYAN}====================================================${RESET}"
echo -e "${BOLD}${CYAN}          ZENE Agent Provider & Model Tester        ${RESET}"
echo -e "${BOLD}${CYAN}====================================================${RESET}\n"

test_gemini() {
    local model="$1"
    if [ -z "$GEMINI_API_KEY" ]; then
        echo -e "  [Gemini] ${RED}GEMINI_API_KEY is not set${RESET}"
        return
    fi
    echo -ne "  Testing Gemini (${BOLD}$model${RESET})... "
    local start=$(date +%s%N)
    local resp
    resp=$(curl -s -w "\n%{http_code}" -H "Content-Type: application/json" \
        -d '{"contents":[{"parts":[{"text":"ping"}]}]}' \
        "https://generativelanguage.googleapis.com/v1beta/models/${model}:generateContent?key=${GEMINI_API_KEY}")
    local code=$(echo "$resp" | tail -n1)
    local body=$(echo "$resp" | sed '$d')
    local elapsed=$(( ($(date +%s%N) - start) / 1000000 ))

    if [ "$code" = "200" ]; then
        echo -e "${GREEN}✓ OK${RESET} (${elapsed} ms)"
    elif [ "$code" = "429" ]; then
        echo -e "${YELLOW}⚠ Rate Limited / Quota (429)${RESET}"
        echo -e "    ${DIM}$(echo "$body" | grep -o '"message": "[^"]*' | head -n1)${RESET}"
    else
        echo -e "${RED}✗ Error ($code)${RESET}"
        echo -e "    ${DIM}$(echo "$body" | grep -o '"message": "[^"]*' | head -n1)${RESET}"
    fi
}

test_groq() {
    local model="$1"
    if [ -z "$GROQ_API_KEY" ]; then
        echo -e "  [Groq] ${RED}GROQ_API_KEY is not set${RESET}"
        return
    fi
    echo -ne "  Testing Groq (${BOLD}$model${RESET})... "
    local start=$(date +%s%N)
    local resp
    resp=$(curl -s -w "\n%{http_code}" \
        -H "Authorization: Bearer ${GROQ_API_KEY}" \
        -H "Content-Type: application/json" \
        -d "{\"model\":\"${model}\",\"max_tokens\":10,\"messages\":[{\"role\":\"user\",\"content\":\"hi\"}]}" \
        "https://api.groq.com/openai/v1/chat/completions")
    local code=$(echo "$resp" | tail -n1)
    local body=$(echo "$resp" | sed '$d')
    local elapsed=$(( ($(date +%s%N) - start) / 1000000 ))

    if [ "$code" = "200" ]; then
        echo -e "${GREEN}✓ OK${RESET} (${elapsed} ms)"
    elif [ "$code" = "429" ]; then
        echo -e "${YELLOW}⚠ Rate Limited / Quota (429)${RESET}"
        echo -e "    ${DIM}$(echo "$body" | grep -o '"message":"[^"]*' | head -n1)${RESET}"
    else
        echo -e "${RED}✗ Error ($code)${RESET}"
        echo -e "    ${DIM}$(echo "$body" | grep -o '"message":"[^"]*' | head -n1)${RESET}"
    fi
}

echo -e "${BOLD}Gemini Models:${RESET}"
test_gemini "gemini-3.5-flash"
test_gemini "gemini-3.5-flash-lite"
test_gemini "gemini-2.5-flash"

echo -e "\n${BOLD}Groq Models:${RESET}"
test_groq "openai/gpt-oss-120b"
test_groq "openai/gpt-oss-20b"
test_groq "qwen/qwen3.8-27b"

echo -e "\n${CYAN}Done. Run './run-normal.sh' or './run-verbose.sh' to start chatting.${RESET}\n"
