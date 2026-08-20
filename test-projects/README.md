# test-projects

Scaffolding + harnesses that exercise Hot Sheet 2 the way a real user (and a real AI
tool) would — full binaries, real stdio, real server — rather than in-process unit
tests. These complement the per-crate `cargo nextest` suites; they don't replace them.

## `e2e-headless-claude.sh` — the headless AI-tool loop (HS2-95 / HS2-99)

Proves that a **fresh project prepared only by `hotsheet setup claude`** is drivable
through Hot Sheet by an AI tool, headless (no HS2 client), in **both** MCP modes:

| Mode | Shim invocation | Server? |
|---|---|---|
| **A. serverless** | `hotsheet-mcp --path <store>` | none — direct to disk |
| **B. server-backed** | `hotsheet-mcp --server <url> --secret` | a real `hotsheet-server` |

For each mode the harness: runs `hotsheet init` + `hotsheet setup claude`, seeds a
ticket, then **drives the `hotsheet-mcp` binary over real stdio** with a scripted
JSON-RPC sequence (`initialize` → `hotsheet_query` → `hotsheet_update` →
`hotsheet_create`) exactly as an AI tool would, and asserts the on-disk result via
the CLI (`hotsheet show` / `ls`).

### Run it

```sh
# Deterministic tiers only (no LLM, no credentials — CI-safe):
test-projects/e2e-headless-claude.sh

# Also run a REAL headless Claude session per mode (billable, opt-in):
HS2_LIVE_CLAUDE=1 test-projects/e2e-headless-claude.sh
```

- The deterministic tiers always run and gate nothing on credentials.
- The **live tier** runs only when `HS2_LIVE_CLAUDE=1` **and** `claude` is on `PATH`;
  it launches `claude -p …` in the prepared project (which picks up the generated
  `CLAUDE.md`, `.claude/skills/hotsheet/SKILL.md`, and `.mcp.json`) and checks the
  agent produced the expected file. This is the [docs/12 §12.7] live-tool smoke tier.
- `HS2_E2E_WORKDIR=<dir>` overrides the work dir (default: a fresh `mktemp -d`).

### What it depends on

`cargo build` of `hotsheet`, `hotsheet-mcp`, `hotsheet-server` (the harness builds
them), plus `python3` (used only to pluck a field out of the JSON-RPC responses).

> The deterministic MCP loop is also covered hermetically by the `hotsheet-mcp`
> `CoreBackend` unit tests. This harness adds the real binaries, the server-backed
> path, and the gated real-tool run. The forthcoming `hs-fake-agent` conformance
> suite (HS2-64) will give the live tier a deterministic, CI-gated stand-in.
