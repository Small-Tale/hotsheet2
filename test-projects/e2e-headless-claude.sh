#!/usr/bin/env bash
# End-to-end smoke for the headless AI-tool loop (HS2-95 / HS2-99 / HS2-102 / HS2-103).
#
# Proves a fresh project, prepared only by `hotsheet-cli setup <tool>`, is drivable
# through Hot Sheet by a real AI tool (Claude and Codex), headless, in BOTH MCP modes:
#   serverless    (hotsheet-mcp --path <store>, no server)
#   server-backed (hotsheet-mcp --server <url> --secret, real hotsheet-server)
#
# Tiers:
#   * Deterministic (always): drive the shim binary over real stdio with a scripted
#     JSON-RPC sequence, asserting on-disk state via the CLI. No LLM, no creds.
#   * Live (opt-in, billable): HS2_LIVE_CLAUDE=1 and/or HS2_LIVE_CODEX=1 run a real
#     `claude -p` / `codex exec` session per mode.
#
# HS2-103 safety (dev machines where HS1 owns /usr/local/bin/hotsheet): a bare
# `hotsheet` there launches HS1 production and can kill a running dev instance. Before
# any live agent runs, we (1) install a transient `hotsheet` shim in ~/.local/bin
# (which precedes /usr/local/bin on PATH) that redirects to our hotsheet-cli, removed
# on exit; (2) point every agent's MCP at an ABSOLUTE hotsheet-mcp path; (3) assert a
# login shell resolves `hotsheet` to our shim before launching; and (4) assert no HS1
# instance (`<proj>/.hotsheet`) was created after each live run.
#
# Usage:
#   test-projects/e2e-headless-claude.sh
#   HS2_LIVE_CLAUDE=1 HS2_LIVE_CODEX=1 test-projects/e2e-headless-claude.sh
# Optional: HS2_E2E_WORKDIR=<dir> to choose the work dir (default: a fresh mktemp).

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BIN="$ROOT/target/debug"
HOTSHEET="$BIN/hotsheet-cli"
MCP="$BIN/hotsheet-mcp"
SERVER="$BIN/hotsheet-server"
LOCALBIN="$HOME/.local/bin"

pass() { printf '  \033[32mok\033[0m   %s\n' "$1"; }
warn() { printf '  \033[33m%s\033[0m %s\n' "$1" "$2"; }
fail() { printf '  \033[31mFAIL\033[0m %s\n' "$1"; exit 1; }
step() { printf '\n\033[1m== %s\033[0m\n' "$1"; }

SERVER_PIDS=()
CREATED_SHIMS=()
cleanup() {
  for p in ${SERVER_PIDS[@]+"${SERVER_PIDS[@]}"}; do kill "$p" 2>/dev/null || true; done
  for f in ${CREATED_SHIMS[@]+"${CREATED_SHIMS[@]}"}; do rm -f "$f"; done
}
trap cleanup EXIT

# result_text <id>: read JSON-RPC responses on stdin, print the tool text for that id.
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

# ---- HS2-103 safety helpers ------------------------------------------------------

any_live() { [ "${HS2_LIVE_CLAUDE:-0}" = "1" ] || [ "${HS2_LIVE_CODEX:-0}" = "1" ]; }

# Install ~/.local/bin shims so a bare `hotsheet` (and hotsheet-cli/hotsheet-mcp)
# resolve to OUR binaries during live runs — never the HS1 launcher. Only creates
# entries that don't already exist, and records them for removal on exit.
install_safe_shims() {
  mkdir -p "$LOCALBIN"
  if [ ! -e "$LOCALBIN/hotsheet" ]; then
    printf '#!/bin/bash\nexec "%s/hotsheet-cli" "$@"\n' "$BIN" > "$LOCALBIN/hotsheet"
    chmod +x "$LOCALBIN/hotsheet"; CREATED_SHIMS+=("$LOCALBIN/hotsheet")
  else
    warn "note" "$LOCALBIN/hotsheet already exists — leaving it (assuming it's safe)"
  fi
  for b in hotsheet-cli hotsheet-mcp; do
    if [ ! -e "$LOCALBIN/$b" ]; then ln -sf "$BIN/$b" "$LOCALBIN/$b"; CREATED_SHIMS+=("$LOCALBIN/$b"); fi
  done
}

# Refuse to launch a live agent unless a login shell resolves bare `hotsheet` to our
# shim (i.e. it can't reach the HS1 production launcher).
assert_hotsheet_safe() {
  local resolved
  resolved="$("${SHELL:-/bin/zsh}" -lic 'command -v hotsheet' 2>/dev/null | tail -1)"
  if [ "$resolved" = "$LOCALBIN/hotsheet" ]; then
    pass "preflight: bare 'hotsheet' -> $resolved (HS1 unreachable)"
  else
    fail "preflight UNSAFE: bare 'hotsheet' -> '$resolved'; refusing to launch a live agent"
  fi
}

# After a live run, a Hot Sheet 1 instance would have created <proj>/.hotsheet.
assert_no_hs1() {
  if [ -e "$1/.hotsheet" ]; then fail "HS1 was launched: $1/.hotsheet exists"; fi
  pass "no HS1 instance was launched (no $1/.hotsheet)"
}

# Write the tool's MCP config pointing at an ABSOLUTE hotsheet-mcp + given args.
write_mcp_json() { # <proj> <arg...>
  local proj="$1"; shift
  python3 -c '
import sys, json, os
proj, cmd = sys.argv[1], sys.argv[2]; args = sys.argv[3:]
json.dump({"mcpServers": {"hotsheet": {"command": cmd, "args": args}}},
          open(os.path.join(proj, ".mcp.json"), "w"), indent=2)
' "$proj" "$BIN/hotsheet-mcp" "$@"
}
write_codex_toml() { # <proj> <arg...>
  local proj="$1"; shift
  mkdir -p "$proj/.codex"
  python3 -c '
import sys, os
proj, cmd = sys.argv[1], sys.argv[2]; args = sys.argv[3:]
body = "[mcp_servers.hotsheet]\ncommand = \"%s\"\nargs = [%s]\n" % (
    cmd, ", ".join("\"%s\"" % a for a in args))
open(os.path.join(proj, ".codex", "config.toml"), "w").write(body)
' "$proj" "$BIN/hotsheet-mcp" "$@"
}

# ---- flow helpers ----------------------------------------------------------------

# prepare <proj> <tool>: init + `setup <tool>` + seed a ticket; echo the slug.
prepare() {
  local proj="$1" tool="$2"
  rm -rf "$proj"; mkdir -p "$proj"
  "$HOTSHEET" -C "$proj" init >/dev/null
  "$HOTSHEET" -C "$proj" setup "$tool" >/dev/null
  "$HOTSHEET" -C "$proj" new --title "Write a greeting" --category task >/dev/null
  "$HOTSHEET" -C "$proj" ls | awk 'NR==1{print $1}'
}

# drive_and_assert <proj> <slug> <shim-args...>: drive the shim as an AI tool would.
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

# start_server <store>: boot a server on an ephemeral loopback port. Must run in the
# CURRENT shell (not a subshell) so SERVER_PIDS is tracked for cleanup — sets globals
# SRV_URL and SRV_SECRET rather than echoing.
SRV_URL=""; SRV_SECRET=""
start_server() {
  local store="$1" n="${#SERVER_PIDS[@]}" secret log url=""
  secret="test-secret-$$-$n"; log="$WORK/server-$n.log"
  "$SERVER" -C "$store" --bind 127.0.0.1:0 --secret "$secret" --index "$store/.index-$n.sqlite" \
    >"$log" 2>&1 &
  SERVER_PIDS+=("$!")
  for _ in $(seq 1 50); do
    url="$(sed -n 's#.*listening on \(http://[0-9.:]*\).*#\1#p' "$log" | head -1)"
    [ -n "$url" ] && break; sleep 0.1
  done
  [ -n "$url" ] || { cat "$log"; fail "server did not start for $store"; }
  SRV_URL="$url"; SRV_SECRET="$secret"
}

GREET_PROMPT="Work the Hot Sheet ticket about GREETING.txt for this project using the hotsheet_* MCP tools: create a file named GREETING.txt containing the word hello, then mark that ticket completed. If you use the shell, the CLI is hotsheet-cli (never run a bare 'hotsheet')."

# live_claude <proj> [url secret]
live_claude() {
  local proj="$1" url="${2:-}" secret="${3:-}"
  if [ "${HS2_LIVE_CLAUDE:-0}" != "1" ] || ! command -v claude >/dev/null 2>&1; then
    warn skip "real Claude (set HS2_LIVE_CLAUDE=1 with claude on PATH)"; return
  fi
  assert_hotsheet_safe
  if [ -n "$url" ]; then write_mcp_json "$proj" --server "$url" --secret "$secret"
  else write_mcp_json "$proj" --path "$proj"; fi
  step "LIVE: Claude in $proj ($([ -n "$url" ] && echo server-backed || echo serverless))"
  "$HOTSHEET" -C "$proj" new --title "Create GREETING.txt containing hello" --category task >/dev/null
  # --strict-mcp-config: load ONLY our (absolute-path) MCP server — never the user's
  # global/project MCP servers (which may include an HS1 hotsheet-channel).
  ( cd "$proj" && claude -p "$GREET_PROMPT" --dangerously-skip-permissions \
      --strict-mcp-config --mcp-config "$proj/.mcp.json" ) || fail "claude session errored"
  [ -f "$proj/GREETING.txt" ] || fail "Claude did not create GREETING.txt"
  pass "Claude created GREETING.txt"; assert_no_hs1 "$proj"
  "$HOTSHEET" -C "$proj" ls | sed 's/^/    /'
}

# live_codex <proj> [url secret]
live_codex() {
  local proj="$1" url="${2:-}" secret="${3:-}"
  if [ "${HS2_LIVE_CODEX:-0}" != "1" ] || ! command -v codex >/dev/null 2>&1; then
    warn skip "real Codex (set HS2_LIVE_CODEX=1 with codex on PATH)"; return
  fi
  assert_hotsheet_safe
  # Isolated CODEX_HOME (so it never loads the user's ~/.codex hotsheet-channel) + auth.
  mkdir -p "$proj/.codex"
  [ -f "$HOME/.codex/auth.json" ] && cp "$HOME/.codex/auth.json" "$proj/.codex/auth.json"
  if [ -n "$url" ]; then write_codex_toml "$proj" --server "$url" --secret "$secret"
  else write_codex_toml "$proj" --path "$proj"; fi
  step "LIVE: Codex in $proj ($([ -n "$url" ] && echo server-backed || echo serverless))"
  "$HOTSHEET" -C "$proj" new --title "Create GREETING.txt containing hello" --category task >/dev/null
  ( cd "$proj" && CODEX_HOME="$proj/.codex" codex exec "$GREET_PROMPT" \
      --dangerously-bypass-approvals-and-sandbox ) || fail "codex session errored"
  [ -f "$proj/GREETING.txt" ] || fail "Codex did not create GREETING.txt"
  pass "Codex created GREETING.txt"; assert_no_hs1 "$proj"
  "$HOTSHEET" -C "$proj" ls | sed 's/^/    /'
}

# --- build ------------------------------------------------------------------------
step "build binaries"
cargo build -q --manifest-path "$ROOT/Cargo.toml" \
  --bin hotsheet-cli --bin hotsheet-mcp --bin hotsheet-server
pass "hotsheet-cli, hotsheet-mcp, hotsheet-server"

WORK="${HS2_E2E_WORKDIR:-$(mktemp -d)}"
mkdir -p "$WORK"

if any_live; then step "safety: install transient ~/.local/bin shims (HS2-103)"; install_safe_shims; assert_hotsheet_safe; fi

# ===== Claude =====================================================================
step "MODE A — Claude, serverless"
A="$WORK/claude-serverless"; SLUG_A="$(prepare "$A" claude)"; pass "setup claude + seeded $SLUG_A"
drive_and_assert "$A" "$SLUG_A" --path "$A"; live_claude "$A"

step "MODE B — Claude, server-backed"
B="$WORK/claude-server"; SLUG_B="$(prepare "$B" claude)"; pass "setup claude + seeded $SLUG_B"
start_server "$B"; pass "hotsheet-server up on $SRV_URL"
drive_and_assert "$B" "$SLUG_B" --server "$SRV_URL" --secret "$SRV_SECRET"; live_claude "$B" "$SRV_URL" "$SRV_SECRET"

# ===== Codex (proves the interface isn't Claude-shaped) ===========================
step "MODE C — Codex, serverless"
C="$WORK/codex-serverless"; SLUG_C="$(prepare "$C" codex)"; pass "setup codex + seeded $SLUG_C"
drive_and_assert "$C" "$SLUG_C" --path "$C"; live_codex "$C"

step "MODE D — Codex, server-backed"
D="$WORK/codex-server"; SLUG_D="$(prepare "$D" codex)"; pass "setup codex + seeded $SLUG_D"
start_server "$D"; pass "hotsheet-server up on $SRV_URL"
drive_and_assert "$D" "$SLUG_D" --server "$SRV_URL" --secret "$SRV_SECRET"; live_codex "$D" "$SRV_URL" "$SRV_SECRET"

echo; printf '\033[1;32mE2E headless-tool smoke passed.\033[0m\n'
printf 'work dir: %s\n' "$WORK"
