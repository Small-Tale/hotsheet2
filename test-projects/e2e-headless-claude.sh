#!/usr/bin/env bash
# End-to-end smoke for the headless AI-tool loop (HS2-95 / HS2-99).
#
# Proves that a fresh project, prepared only by `hotsheet setup claude`, is drivable
# through Hot Sheet via the MCP shim — in BOTH modes:
#   A. serverless    (hotsheet-mcp --path <store>, no server)
#   B. server-backed  (hotsheet-mcp --server <url> --secret, real hotsheet-server)
# and, when opted in, by a REAL headless Claude session in the project directory.
#
# Tiers:
#   * Deterministic (always runs): drives the shim binary over real stdio with a
#     scripted JSON-RPC sequence, exactly as an AI tool would, and asserts the
#     on-disk ticket state via the CLI. No LLM, no credentials — CI-safe.
#   * Live (opt-in): set HS2_LIVE_CLAUDE=1 with `claude` on PATH to also run a real
#     `claude -p` session for each mode. Billable; skipped by default.
#
# Usage:
#   test-projects/e2e-headless-claude.sh
#   HS2_LIVE_CLAUDE=1 test-projects/e2e-headless-claude.sh
# Optional: HS2_E2E_WORKDIR=<dir> to choose the work dir (default: a fresh mktemp).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug"
HOTSHEET="$BIN/hotsheet"
MCP="$BIN/hotsheet-mcp"
SERVER="$BIN/hotsheet-server"

pass() { printf '  \033[32mok\033[0m   %s\n' "$1"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n' "$1"; exit 1; }
step() { printf '\n\033[1m== %s\033[0m\n' "$1"; }

# result_text <id>: read JSON-RPC responses on stdin, print the tool text for that id.
# (Uses `python3 -c` so stdin stays the piped data — a heredoc script would consume it.)
result_text() {
  python3 -c '
import sys, json
want = sys.argv[1]
for ln in sys.stdin:
    ln = ln.strip()
    if not ln:
        continue
    m = json.loads(ln)
    if str(m.get("id")) == want:
        r = m.get("result", {})
        print(r["content"][0]["text"] if "content" in r else json.dumps(r))
' "$1"
}

# prepare <proj>: init + `setup claude` + assert artifacts + seed a ticket; echo slug.
prepare() {
  local proj="$1"
  rm -rf "$proj"; mkdir -p "$proj"
  "$HOTSHEET" -C "$proj" init >/dev/null
  "$HOTSHEET" -C "$proj" setup claude >/dev/null
  grep -q "BEGIN hotsheet:claude" "$proj/CLAUDE.md" 2>/dev/null || fail "CLAUDE.md block missing"
  [ -f "$proj/.claude/skills/hotsheet/SKILL.md" ] || fail "skill missing"
  grep -q '"hotsheet-mcp"' "$proj/.mcp.json" 2>/dev/null || fail ".mcp.json not registered"
  "$HOTSHEET" -C "$proj" new --title "Write a greeting" --category task >/dev/null
  "$HOTSHEET" -C "$proj" ls | awk 'NR==1{print $1}'
}

# drive_and_assert <proj> <slug> <shim-args...>: drive the shim as an AI tool would,
# then assert the on-disk result via the CLI.
drive_and_assert() {
  local proj="$1" slug="$2"; shift 2
  local out
  out="$(printf '%s\n' \
    '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}' \
    '{"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"hotsheet_query","arguments":{"open":true}}}' \
    "{\"jsonrpc\":\"2.0\",\"id\":3,\"method\":\"tools/call\",\"params\":{\"name\":\"hotsheet_update\",\"arguments\":{\"id\":\"$slug\",\"status\":\"completed\"}}}" \
    '{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"hotsheet_create","arguments":{"title":"Follow-up from the agent","category":"task"}}}' \
    | "$MCP" "$@")"

  printf '%s\n' "$out" | result_text 2 | grep -q "$slug"                 || fail "query did not return $slug"
  pass "hotsheet_query returned the seeded ticket"
  printf '%s\n' "$out" | result_text 3 | grep -q '"status": "completed"' || fail "update did not complete the ticket"
  pass "hotsheet_update set status=completed"
  "$HOTSHEET" -C "$proj" show "$slug" | grep -q "status: completed" || fail "ticket not completed on disk"
  "$HOTSHEET" -C "$proj" ls | grep -q "Follow-up from the agent"    || fail "agent-created ticket not on disk"
  pass "disk reflects the completion + the agent's new ticket"
}

# live_claude <proj>: optional real headless Claude run (billable, opt-in).
live_claude() {
  local proj="$1"
  if [ "${HS2_LIVE_CLAUDE:-0}" = "1" ] && command -v claude >/dev/null 2>&1; then
    step "LIVE: real headless Claude session in $proj"
    "$HOTSHEET" -C "$proj" new --title "Create GREETING.txt containing hello" --category task >/dev/null
    ( cd "$proj" && claude -p "Use the Hot Sheet skill. Work the Up Next ticket about GREETING.txt: create the file with the word hello, then mark that ticket completed." ) \
      || fail "claude session errored"
    [ -f "$proj/GREETING.txt" ] || fail "Claude did not create GREETING.txt"
    pass "Claude created GREETING.txt"
  else
    printf '  \033[33mskip\033[0m real Claude (set HS2_LIVE_CLAUDE=1 with claude on PATH)\n'
  fi
}

# --- build ------------------------------------------------------------------------
step "build binaries"
cargo build -q --manifest-path "$ROOT/Cargo.toml" \
  --bin hotsheet --bin hotsheet-mcp --bin hotsheet-server
pass "hotsheet, hotsheet-mcp, hotsheet-server"

WORK="${HS2_E2E_WORKDIR:-$(mktemp -d)}"
mkdir -p "$WORK"
SERVER_PID=""
trap '[ -n "$SERVER_PID" ] && kill "$SERVER_PID" 2>/dev/null || true' EXIT

# ===== Mode A: serverless =========================================================
step "MODE A — serverless (no server)"
A="$WORK/serverless"
SLUG_A="$(prepare "$A")"; pass "setup + seeded $SLUG_A"
drive_and_assert "$A" "$SLUG_A" --path "$A"
live_claude "$A"

# ===== Mode B: server-backed ======================================================
step "MODE B — server-backed (real hotsheet-server)"
B="$WORK/server-backed"
SLUG_B="$(prepare "$B")"; pass "setup + seeded $SLUG_B"
SECRET="test-secret-$$"
"$SERVER" -C "$B" --bind 127.0.0.1:0 --secret "$SECRET" --index "$B/.index.sqlite" \
  >"$WORK/server.log" 2>&1 &
SERVER_PID=$!
URL=""
for _ in $(seq 1 50); do
  URL="$(sed -n 's#.*listening on \(http://[0-9.:]*\).*#\1#p' "$WORK/server.log" | head -1)"
  [ -n "$URL" ] && break
  sleep 0.1
done
[ -n "$URL" ] || { cat "$WORK/server.log"; fail "server did not start"; }
pass "hotsheet-server up on $URL (pid $SERVER_PID)"
drive_and_assert "$B" "$SLUG_B" --server "$URL" --secret "$SECRET"
live_claude "$B"

echo; printf '\033[1;32mE2E headless-Claude smoke passed.\033[0m\n'
printf 'work dir: %s\n' "$WORK"
