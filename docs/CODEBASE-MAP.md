# Codebase Map

The AI-read orientation doc: where things are, how to build/test, and where to look
for X. Keep in sync in the same change that adds a file/dir, a command, a schema
field, or a setting. Requirements status lives in [README.md](README.md); the field
schema in [17-ticket-file-format.md](17-ticket-file-format.md); feature-level unit/E2E
evidence in [TEST-COVERAGE.md](TEST-COVERAGE.md).

> **Status:** early implementation. Built: the Rust core model + file format, a
> filesystem store, the shared engine `ops`, the SQLite/FTS **index** + the server's
> **filesystem watcher** (live reindex), the `hotsheet-cli` CLI + `hotsheet-migrate`,
> the Node HS1 exporter, the **`hotsheet-server`** (index-backed HTTP REST + WS,
> loopback auth; **file-backed index restored + reconciled on launch**), and the
> **`hotsheet-mcp`** shim, the **server-side lifecycle** (instance registry, writer
> lock, `serve --stop`; HS2-59), the `hotsheet-cli reindex` CLI, the plugin host +
> AI-tool **drive/permission/metrics** stack (`hotsheet-aitools`, `hotsheet-plugins`),
> and **`hotsheet-terminals`** (PTY + manager + busy) with server `/terminals*` routes.
> Still design-only or incomplete: **client** auto-start/supervise, live-tool protocol
> verification, and the clients. No-server index maintenance, git-aware fast-path reindex,
> and Tier-1 mTLS are built.

## Directory tree

```
hot-sheet2/                  # this repo = CODE only; tickets are a SEPARATE store (below)
  Cargo.toml                 # Rust workspace (edition 2024, resolver 3)
  scripts/check-test-coverage.mjs # CI validator for docs/TEST-COVERAGE.md evidence
  rust-toolchain.toml        # pinned stable + rustfmt + clippy
  spikes/kerf-webawesome/    # Kerf 4.4 + Web Awesome 3.11 Vite/Playwright compatibility proof
  clients/web/               # Kerf + Web Awesome API-only web/Tauri UI foundation
    src/api.ts               #   Typed server client for providers plus checkout-scoped real ticket/repository operations
    src/main.tsx             #   Real AppShell: project tabs, tickets/views, and cross-project permission polling/resolution
    src/permission-notifications.ts # Machine-local permission inbox/history plus visible-presentation-only automation timers
    src/not-working-workflow.ts # Compensating evidence upload + completed-ticket reopen workflow
    src/ticket-views.ts      #   Disjoint active Queue, Backlog, and Archive view semantics
    src/ticket-operations.ts #   Checkout-scoped field/external undo/redo and structured attachment-aware ticket clipboard operations
    src/ticket-mutation.ts   #   Optimistic field projection, targeted response reconciliation, stale guards, and phase telemetry
    src/project-bridge.ts    #   Vite-only local server discovery/detached start + credential-hiding API proxy
    src/dev-server.ts        #   Hono local project bridge plus dev-only /ux-demo and review routes
    src/dev-review/          #   Content-anchored capture/delete overlay, upload/removal review UI, and single-commit local-dev CLI submission adapter (shell.ts: POSIX arg-quoting + runCommand for copy-paste-runnable failure messages)
    src/components/          #   Production domain UI components, including shared Toolbar/ToolbarText/ToolbarControlGroup, Select, MenuItem/MenuHeader, project/page headers, sidebar/tab-shell surfaces; shared palette, cursor semantics, and Lucide policy
    src/ux-demo/             #   Categorized master/detail catalog with evocative icons and dependency-aware modification recency, connected workspace/composer/inspector/sidebar mock state, optional non-modal settings inspector
    tests/providers.spec.ts  #   Real-browser project onboarding/ticket flows + opt-in live visual review
    src/components/*.tsx     #   Production web components; each imports its colocated component CSS
    src/components/*.css     #   Production styles exercised unchanged by /ux-demo and the real app
    tests/ux-demo.spec.ts    #   Real-browser catalog/component contracts plus pixel-verified dev-review draw/resize/scrolled-capture/review/submit flow
  crates/
    hotsheet-extsync/          # Direct authoritative external providers (network deps, no terminals)
      src/github.rs            #   GitHub Issues mapping, pagination/incremental reads, webhook invalidation, errors/auth/concurrency, fake + opt-in live tests (HS2-JAXS4Z)
      src/gitlab.rs            #   GitLab Issues mapping, native IDs/URLs, pagination/incremental reads, typed errors/concurrency, fake + opt-in live drift tests (HS2-0RK4YC)
      src/jira.rs              #   Jira Cloud issue/ADF mapping, token pagination/incremental JQL, honest workflow capabilities, fake + opt-in live drift tests (HS2-0RK4YC)
    hotsheet-model/          # pure domain model + ticket file format (no I/O)
      src/lib.rs             #   re-exports; SCHEMA_VERSION
      src/enums.rs           #   Priority/Status/CloseReason/NoteKind/ReviewKind
      src/ids.rs             #   Ulid re-export + derive_slug (FNV-1a -> Crockford)
      src/ticket.rs          #   Ticket/Note/ReviewRequest/ExternalLink; Ticket::new
      src/timestamp.rs       #   Timestamp: lenient RFC3339 (raw text + parsed instant)
      src/format.rs          #   parse_file / to_file_string (YAML + bounded/escaped Markdown body + five note kinds + created/edited timestamps; legacy reader)
    hotsheet-ticketing/      # engine crate (sync API, injected ports)
      src/lib.rs             #   mint_ulid(clock, rng)
      src/ops.rs             #   query/create/update/close/claim/copy_ticket/move_ticket/assign — the one op impl (CLI+server+MCP); TicketQuery.assignee filter + keyset page_after (HS2-20/HS2-TCDTCH)
      src/provider.rs        #   provider-neutral identity/capabilities/errors/CRUD+claim contract; registry + GitProvider; idempotent cross-provider copy/move coordinator and provenance (HS2-ZVZP80/HS2-A90JRH)
      src/identity.rs        #   current-user identity: current_user_email (git user.email) + resolve_me — the `me` sentinel for assignee/review filters (docs/10 §10.3, HS2-TCDTCH)
      src/activity.rs        #   cross-tool activity events (docs/15, HS2-KP31ZE/4C68Y8): ActivityEvent/Kind/Importance + bounded rolling store/timeline + mappers; server persists then broadcasts full payloads on WS/poll and live drive emits coarse attributed events
      src/roster.rs          #   Roster/Person: committed people.json (git email → name/github); display_name; docs/10 §10.2 (HS2-20)
      src/distclaim.rs       #   git-native distributed claim/lease: refs/hotsheet/claims/<ulid> push-CAS (first-wins), force-with-lease renew/steal, ls-remote enumerate, expiry sweep (docs/08 §8.5, HS2-84)
      src/distwork.rs        #   distributed self-claim fan-out: select_and_claim (worklist candidate + git-ref CAS → local claim; two machines never double-claim) + work_once (claim→drive→renew/release lease cycle, injected drive); docs/08 §8.5, HS2-E7RXXR/HS2-DTPX2V
      src/metrics.rs         #   usage/cost metrics: UsageEvent → raw JSONL (metrics/raw/<day>.jsonl, gitignored) + rollup/summary aggregation + record_priced (fills cost from the table); rollup FILES (metrics/rollups/<git-email>/rollup.json, per-contributor) via roll_up_through + summary_settled (settled+tail read) + prune_raw_before (retention) + team_summary; docs/14, HS2-69/HS2-8BCRHS
      src/pricing.rs         #   per-model price table (USD/Mtok): default_prices + metrics/prices.json override + cost(); so cost is always present (docs/14 §14.2, HS2-8BCRHS)
      src/merge.rs           #   merge_tickets: semantic 3-way merge (field-by-field/set-union/notes-union/reviews-union-by-ULID/body) behind `hotsheet merge-driver` (HS2-18, HS2-20)
      src/sync.rs            #   sync_once: one fetch → rebase-through-merge-driver → push cycle (offline/conflict-tolerant) behind `hotsheet sync` (HS2-19)
      src/ports.rs           #   Clock, Rng (FileSystem/GitLocal/... to come)
      src/store.rs           #   FsStore: init/open/read/write/list + StoreMetadata; git-diff fast path (head_commit/is_working_tree_clean/changed_ticket_ids_between, HS2-90)
      src/registry.rs        #   StoreRegistry: resolve a ULID across multiple stores, follow moved_to_store tombstones (docs/02 §2.2.1, HS2-4)
      src/settings.rs        #   Settings: global (${HOTSHEET_HOME}) / shared (committed) / local (gitignored) scopes; effective precedence global<shared<local (HS2-34)
      src/auto_context.rs    #   HS1-compatible category/tag guidance defaults + override/suppression/matching (HS2-BZBVAS)
      src/secrets.rs         #   injected SecretStore + OS keychain adapters + metadata-only provider registry (HS2-M1XMSX)
      src/checkouts.rs       #   readable path-derived checkout ids + machine registry; many-to-many checkout/store discovery and scoped routing (HS2-NGC8AE/VSPFD9)
      src/repository_status.rs # git porcelain-v2 repository snapshot parser/runner (HS2-RPVFA4)
      src/analytics.rs       #   current ticket-flow, throughput, and cycle-time aggregates (HS2-38RJMK)
      src/commands.rs        #   typed safe argv command settings schema (HS2-JN3X4W)
      src/overlay.rs         #   LocalOverlay: per-user Tier B data under gitignored <store>/local/ (read-tracking; docs/02 §2.11, HS2-21)
      src/wire.rs            #   wire SSOT: ApiTicket/ApiNote/ApiAttachment timestamp metadata/TicketRow incl. provider identity + compact body-optional lists (shared by server + MCP)
      src/worklist.rs        #   checkout-local .hotsheet/worklist.md: aggregates configured git stores; active-only Up Next; refreshed by CLI/project-open and watcher-coalesced external changes
    hotsheet-cli/            # two binaries + a shared lib
      src/main.rs            #   `hotsheet-cli`: default git commands plus providers/provider-ls/get/new/edit/close, provider-copy/move, setup/plugins/settings/server/workflows
      src/permission_hook.rs #   Claude PreToolUse hook adapter (HS2-YMR9HE): pure map of Claude hook JSON → bridge (tool,action) + allow/deny/ask decision; the `permission-hook` cmd POSTs /permissions/ask ($HOTSHEET_SERVER/$HOTSHEET_SECRET), else `ask`
      src/external_launch.rs #   capability-aware external-terminal launch preparation: per-store server-instance discovery + permission route-back data; Claude hook supported, native Codex rejected until adapted (HS2-C46G58)
      src/bin/hotsheet-migrate.rs #   `hotsheet-migrate`: standalone HS1 migrator (spawns Node exporter + imports)
      src/lib.rs             #   shared: run_import / run_migrate / git helpers (pglite-free); re-exports hotsheet_aitools::launch_safety
      src/workloop.rs        #   `work` loop pure helpers: Up Next queue signature + thrash-guard Stall counter
      # (launch_safety + SafeTrigger/prepare_trigger moved to hotsheet-aitools so the server reuses them — HS2-1TY7GC)
      src/setup.rs           #   `hotsheet-cli setup <tool>`: thin wrapper over hotsheet_plugins::run_setup (adds the enabled-plugin filter from Settings)
      src/plugin.rs          #   `hotsheet-cli plugin list|info|install|verify|remove`: manage + trust-gate external plugins
      src/import.rs          #   hotsheet-export.json -> store (two-pass, idempotent)
      tests/cli.rs, tests/migrate.rs #  E2E for each binary (assert_cmd)
      tests/plugin_conformance.rs #  HS2-64 hard gate: every plugin (builtin + on-disk) validated — capabilities + headless-setup E2E; a new tool inherits it by existing
    hotsheet-server/         # `hotsheet-server` binary (axum HTTP + WS)
      src/lib.rs             #   app() router + ticket/terminal/permission/activity APIs; provider discovery/scoped routes plus idempotent /provider-transfers/copy|move; /stores remains compatible
      src/main.rs            #   bind + serve (loopback = Tier-0 plaintext; off-loopback = Tier-1 mTLS via tls::build_server_config + serve_tls, HS2-VT3JMF); instance file + writer lock + graceful shutdown + --stop (lifecycle, HS2-59); prints port + secret
      src/tls.rs             #   Tier-1 mTLS (HS2-VT3JMF/MPC0QF): required client cert + live revocation verifier; serve_tls_with_acl fingerprints each peer and applies live optional read-only/read-write/deny authorization before routing HTTP
      src/dist_work_loop.rs  #   server-hosted distributed driving loop (HS2-DTPX2V/HS2-1TY7GC): DistWorkConfig + work_pass + spawn_dist_work_loop + live_drive (SafeTrigger per claimed ticket, permission bridge, attributed usage, and coarse turn_start/permission/turn_end activity sink, HS2-0WCRZY/4C68Y8) + outcome_from_turn; wired into main.rs via --drive-tool
      src/lifecycle.rs       #   server lifecycle: InstanceInfo registry + discovery (one machine server writes a discovery file per hosted store, HS2-87 topology A), per-store index-writer lock, stop_instance (HS2-59)
      src/commands.rs        #   configured argv execution, cursor output, cancellation, bounded history (HS2-JN3X4W)
      src/notifications.rs   #   targeted/deduplicated/acknowledged notification routing (HS2-ZP869N)
      src/tts.rs             #   server-owned TTS provider boundary; no provider secrets on client wire (HS2-5PSQJQ)
      src/multistore.rs      #   StoreHost: registry of served stores (StoreEntry{store,index}) keyed by a short URL id + StoreInfo listing (HS2-87). Per-store fs-watcher via WatchTarget; cross-store resolve; configured_store_paths (stores.json startup discovery); file-backed index_path_for in persistent mode
      src/sync_loop.rs       #   background sync loop: sync_once per hosted store on interval + kick-on-write + exponential backoff (sync_all/next_delay pure + tested; docs/02 §2.12, HS2-19 follow-up)
      src/terminal_broker.rs #   server↔detached-broker integration (HS2-ERT00F): TerminalBroker::ensure (discover/spawn the broker per project under ${HOTSHEET_HOME}/broker) + call (per-request BrokerClient round-trip); `serve --terminal-broker` routes /terminals ops + the live WS attach (bridged to a BrokerStream — broker_attach_loop in lib.rs) + the connect busy feed (polls the broker's Read) through it so terminals survive a server restart; idle-GC/health is a follow-up (HS2-SV3XS8)
      tests/http.rs          #   in-process HTTP E2E (tower::oneshot)
    hotsheet-mcp/            # `hotsheet-mcp` binary (MCP shim)
      src/lib.rs             #   JSON-RPC handle_message + hotsheet_* tools over a Backend
                             #     (provider-aware providers/query/get/create/update/close/assign plus batch/claim/release/renew/copy/move):
                             #     CoreBackend (direct-to-disk, serverless) | HttpBackend (proxy a server)
      src/main.rs            #   stdio JSON-RPC loop; --path <store> (serverless) | --server <url> --secret
    hotsheet-tls/            # Tier-1 mTLS material (rcgen-only, no rustls; used by CLI + server) — docs/04 §4.6, HS2-VT3JMF
      src/lib.rs             #   per-project CA + explicit cert lifetimes; issue/renew/revoke device leaves; optional fingerprint-keyed acl.json roles; Paths (${HOTSHEET_HOME}/tls/<project-id>/); project_id matches lifecycle's path-hash
    hotsheet-terminals/      # PTY manager (nearly standalone; dep portable-pty) — docs/05 §5.4, HS2-10
      src/terminal.rs        #   Terminal: spawn a command in a PTY, scrollback ring, drain thread, write/resize/kill; env-scrubbed + parent-env-inherited; subscribe() live output fan-out (tokio broadcast) for the WS attach (HS2-XTTTMV)
      src/manager.rs         #   TerminalManager: per-(project,terminal) lazy spawn + shared Arc<Terminal> + list/get/kill/reap
      src/busy.rs            #   BusyDetector: streaming OSC-133 busy/idle inference + contains_spinner hint (feeds HS2-107)
      src/osc.rs             #   OscScanner: streaming OSC 7/8/9 parser → TermState{cwd,link,progress,notify}; surfaced on the terminal read/list state (HS2-RCKEJ9)
      src/sizing.rs          #   SizeArbiter: server-arbitrated multi-viewer PTY sizing (focus-follows default + smallest/largest/pinned; leased ViewportClaims + heartbeat/expire; SIZE_FOCUS_HOLD/MIN_DELTA/RESIZE_MIN_INTERVAL guards; self-heal on disconnect) — pure, injected-clock, transition+adversarial tested (HS2-BD7Q74)
      src/broker.rs          #   detached broker (HS2-8HHFHN/HS2-ERT00F): line-delimited JSON Request/Response over a Unix socket, serve_broker (hosts a TerminalManager) + BrokerClient; plus a streaming Attach op (StreamOut/StreamIn frames) + BrokerStream client for the live attach (replay scrollback → stream output + size decisions; apply input + size claims, self-heal on disconnect) — PTYs live in a separate process so terminals survive a server restart
      src/bin/broker.rs      #   `hotsheet-terminal-broker <socket> <project>` binary: bind the UDS + serve_broker until killed
      src/bin/fake_agent.rs  #   `hs-fake-agent` — deterministic PTY-byte emulator for terminal E2E (OSC 133/7/9 + spinner + print/hold/exit as a left-to-right script), HS2-1GJY50
      tests/fake_agent.rs    #   integrated terminal E2E: hs-fake-agent drives busy/cwd/progress/output→idle→exit + spinner detection, in one realistic sequence
      src/env.rs             #   scrub_env: drop TSX_/npm_/NODE_/HOTSHEET_ markers before a child inherits
    hotsheet-index/          # disposable SQLite + FTS5 index (cache over the store)
      src/lib.rs             #   Index (SCHEMA_VERSION 4): open_reconciled/reconcile (git-diff fast path)/rebuild/upsert/delete/query + hash_bytes; facets: tags + assignees + reviews(who/requested_by) tables; filters incl. assignee/review_requested/review_by (facet joins) + claimed + blocked/unblocked (json_each over blocked_by vs done set) + created/updated date-range + moved-tombstones-hidden-by-default; keyset page_after paging
    hotsheet-aitools/        # AI-tool host (behavioral half): the drive/transport interface
      src/acp.rs             #   live ACP v1 stdio JSON-RPC client/session + AcpDrive, streaming/cancel, safe permission fallback, usage mapper, OpenCode drift oracle (HS2-PEQ6Q8)
      src/drive.rs           #   Drive trait (+ service() -> BackingService accessor) + BackingService trait + Transport/Target/DriveCtx/TurnHandle/DoneReason
      src/host.rs            #   drive_for(plugin) + trigger(): plugins[drive] -> Drive -> registry glue
      src/appserver.rs       #   AppServerDrive (Codex persistent daemon: turn on a resumed thread; with_daemon() exposes its CodexDaemonService via Drive::service) + AppServerClient port
      src/codex.rs           #   CodexAppServer: real AppServerClient (codex 0.148 JSON-RPC engine) + StdioTransport (live-verified) / UdsWsTransport (shared daemon: WebSocket over the control socket, HS2-115) + codex_control_socket_path + ensure_codex_daemon + CodexDaemonService (BackingService prestart, tool-id-free, HS2-V5Z2EY) + turn_usage (parse token usage from turn/completed → drive::Usage, the codex-usage metrics mapper, HS2-8PSAFE); loopback + scripted-WS daemon tests
      src/claude.rs          #   ClaudeChannelDrive + ClaudeChannel: turn injected into a running `claude` stream-json session, async TurnEvent stream; ClaudeStreamTransport + claude_result_usage (parse token usage from the `result` event → drive::Usage, the claude-usage metrics mapper, HS2-TJ8FGR); scripted-claude tests
      src/procio.rs          #   StreamChild: shared piped-stdio plumbing (spawn -> RpcWriter/RpcReader) for the stream transports
      src/live.rs            #   run_trigger → TurnDone{reason, session_id}: spawn a REAL tool per its [drive] transport (codex app-server: StdioTransport, or shared-daemon UdsWsTransport when --shared-daemon), build DriveCtx, pump_turn one turn (behind `hotsheet-cli trigger`); pump_turn heartbeats ConnectionRegistry busy at a LIVE clock per streamed event + emits TurnEvent::Usage + idle on Done (HS2-34X6BW); surfaces the tool session/thread id for cross-turn resume (HS2-3C1XK3)
      src/launch_safety.rs   #   HS2-103 safety: hotsheet->hotsheet-cli PATH shim, assert_no_hs1, shell-free executable resolution, absolute hotsheet-mcp path, IsolatedCodexHome (auto MCP-free CODEX_HOME, HS2-YRDQNX) — moved here so CLI + server share it (HS2-1TY7GC)
      tests/fixtures/       #   sanitized, version-pinned real Codex/Claude protocol cassettes replayed in fast CI (live drift oracle remains ignored/creds-gated)
      src/safe_trigger.rs    #   SafeTrigger + prepare_trigger: resolve a tool + assemble launch safety once, run_turn(on_event sink, conn_id) per turn; shared by `hotsheet-cli trigger`/`work` and the server driving loop (HS2-1TY7GC)
      src/spawn.rs           #   SpawnDrive (spawn-per-run, Codex `exec` shape) + SpawnDrive::codex()
      src/ports.rs           #   ProcessSpawner/SpawnedProcess + AcpClient + AppServerClient/Turn + RpcTransport/Reader/Writer (injected) + SpawnSpec
      src/system.rs          #   SystemSpawner (real std::process adapter)
      src/registry.rs        #   ConnectionRegistry: live connections + sliding-window busy tracking
      src/permission.rs      #   FIFO permission queue + allow-rules + route-back; production rules live under ${HOTSHEET_HOME}/permissions/<primary-store-id>.json and Claude/Codex share a 24-hour safe-deny guard
      src/codex.rs (perm)    #   Codex approval ServerRequests route through a PermissionPolicy (Arc<SharedPermissionBridge> + connection + timeout) via decide_approval → request_blocking_timeout: allow-rules auto-resolve, else BLOCK the turn for a human over the route-back (up to timeout, then safe default) — HS2-0QGW07 → HS2-Q1F6HV; attached in live.rs when a bridge is threaded through LiveTrigger/SafeTrigger
      src/permission.rs (2)  #   SharedPermissionBridge: thread-safe request_blocking (blocks a tool until a human resolve() over the route-back) + pending() + resolve() + set_on_pending (event-bus nudge) + reseed_rules; durable rule storage load_rules/append_rule (StoredRule) — the human-round-trip transport (docs/05 §5.7, HS2-9R9YZW) + request_blocking_timeout (block for a human, safe fallback); codex is activated through the drive (HS2-Q1F6HV); Claude PreToolUse hook adapter pending (HS2-YMR9HE)
    hotsheet-plugins/        # AI-tool plugin loader + registry (core `plugins` module)
      src/setup.rs           #   run_setup: core-owned one-shot setup writers (instructions/skill/MCP-config + permission-hook via [hooks] capability, merge-safe idempotent, HS2-XCTAHM) + mcp_command — shared by CLI + server (HS2-91)
      src/lib.rs             #   Manifest/Plugin, from_dir (bundled) + from_fs_dir (on-disk),
                             #     all_plugins(search_dirs)/find_in/builtin_plugins; ${HOTSHEET_HOME:-~/.hotsheet2}/plugins
      src/tests.rs           #   built-in + on-disk loading, first-party-wins-collision
      tests/no_tool_id_branches.rs # HS2-9 plugin-first lint: core must not branch on a tool id (ids derived from the registry)
  plugins/                   # first-party AI-tool plugin dirs, bundled into the binary (docs/05 §5.11)
    claude/                  #   manifest.toml + instructions.md (CLAUDE.md) + SKILL.md; claude-json MCP; [drive] claude-channel (async)
    codex/                   #   AGENTS.md instructions; no skill; codex-toml MCP; [drive] app-server (persistent)
    antigravity/             #   AGENTS.md instructions; no skill; .agents/mcp_config.json; [drive] spawn + --conversation resume
  migrator/                  # disposable Node HS1 exporter (docs/07)
    src/export.mjs           #   exportFromDb(db, project) + CLI (opens a datadir copy)
    src/introspect.mjs       #   schema-dump helper
    test/export.test.mjs     #   vitest: synthetic HS1 DB + cross-language conformance
  test-projects/             # full-binary E2E harnesses (not in-process unit tests)
    e2e-headless-claude.sh   #   headless loop: setup + drive hotsheet-mcp (serverless + server) [HS2-99]
  docs/                      # design docs 00–17 (+ this map)
  .github/workflows/ci.yml   # fmt --check · clippy -D warnings · nextest · migrator vitest · gated cargo-llvm-cov (--fail-under-lines 80)
  .github/workflows/live.yml # creds-gated nightly live tier (#[ignore] codex/claude turns)
```

## Entry points

- **CLI:** `crates/hotsheet-cli/src/main.rs` → binary `hotsheet-cli` (live ticket ops).
  Global `-C/--path` selects the store dir. `init --standalone [--at/--remote]` creates a
  separate git store and links the current code project in one shot. Subcommands: `init`, `link`, `new`
  (incl. `--blocked-by`), `ls` (filters/sort/text/`--limit`), `show`, `edit`
  (incl. `--blocked-by`/`--clear-blocked-by`), `attach`, `close`, `providers`, `setup` (AI-tool setup, headless),
  `plugin` (list/install/remove external plugins), `settings` (get/set/list,
  global|shared|local), `key` (OS-keychain-backed set/get/list/delete),
  `import`, `doctor`, `claim-next`, `release`, `renew`, `trigger` (the headless "play":
  `launch <tool>` replaces itself with a hook-capable interactive tool in the caller's
  terminal, discovering the store from `.hotsheet/store` and the permission route-back
  from `${HOTSHEET_HOME}/instances` (Claude today — HS2-C46G58). `trigger` can
  drive a real AI tool for the project and stream one turn — HS2-109; HS2-103 launch
  safety baked in — HS2-117), `work` (the headless loop: `trigger` one turn at a time
  until Up Next drains, with a thrash guard — HS2-118).
- **Migrator CLI:** `src/bin/hotsheet-migrate.rs` → **separate** binary
  `hotsheet-migrate` (rarely-used, one-time, needs Node). `hotsheet-migrate
  <old/.hotsheet> -C <store>` spawns the Node exporter against a *copy* of the old
  database, then imports. Then `hotsheet-cli`'s `ls`/`show`/`edit`/`new` operate on the
  store. Kept out of `hotsheet-cli` so the live CLI carries no Node/migrator runtime dep.
- **Server:** `hotsheet-server -C <store> [--bind 127.0.0.1:8787] [--secret …]` — HTTP
  REST (`/health`, `/tickets`…) + `/ws/sync`, `X-Hotsheet-Secret` auth (Tier 0,
  loopback only). Queries go through the file-backed SQLite index (restored + reconciled
  on launch), with `ops` as the write path. Unauthenticated `/health` returns the HS2
  generation/API marker plus non-secret store prefix/schema so MCP clients can diagnose
  wrong-secret vs. HS1/wrong-endpoint failures (HS2-8H8BQM).
- **MCP shim:** `hotsheet-mcp --path <store>` (serverless, direct-to-disk — the
  headless default) **or** `--server <url> --secret <s>` (proxy a running server).
  Stdio JSON-RPC exposing the `hotsheet_*` tools. An AI tool spawns it per project.
- **Migrator:** `migrator/src/export.mjs` → `node src/export.mjs <.hotsheet> [--out …]`.
- **Library:** `hotsheet_model::{parse_file, to_file_string, Ticket}` is the format
  SSOT; `hotsheet_ticketing::FsStore` is the on-disk store.

## Data / formats

- **Ticket file:** `tickets/<2-char shard>/<ULID>.md` — YAML frontmatter + explicitly
  bounded, collision-escaped Markdown body (`details`) and notes. Notes have stable
  ULIDs, five kinds, and `created_at`/`edited_at`; legacy one-sided note files remain
  readable and migrate deterministically. Schema: [17](17-ticket-file-format.md).
- **Attachment payload:** `attachments/<ticket-ULID>/<attachment-ULID>/<filename>`;
  `{id, filename, created_at}` lives in ticket frontmatter. Legacy direct children use
  deterministic metadata based on ticket identity, never filesystem mtime.
- **Store metadata:** `hotsheet-store.json` (camelCase: `schemaVersion`,
  `ticketPrefix`, `idStrategy`, `shard`). See `store.rs::StoreMetadata`.
- **Settings:** `hotsheet-settings.json` (shared, committed) + `hotsheet-settings.local.json`
  (local, gitignored), plus **global** `${HOTSHEET_HOME}/settings.json` (machine-wide) —
  flat key→JSON maps, effective precedence global<shared<local. See `settings.rs::Settings`.
- **Provider keys:** `${HOTSHEET_HOME}/keys.json` contains non-secret provider metadata;
  values live behind native macOS Security.framework or Linux Secret Service adapters.
  `key set` uses a hidden terminal prompt or piped stdin. Settings carry only `{ "secret": "provider" }`.
  See `secrets.rs::{SecretStore,KeyRegistry,resolve_setting_secret}`.
- **Checkout discovery:** `${HOTSHEET_HOME}/checkouts.json` maps readable path-derived
  checkout ids to optional repository identities and any number of ticket stores. It
  never contains authentication material; use `checkout register|list|resolve`.
- **People roster:** `people.json` (shared, committed) — `{people:[{email,name?,github?}]}`
  mapping git identity → display name for assignment. See `roster.rs::Roster` (HS2-20).
- **Multi-store discovery:** `${HOTSHEET_HOME}/stores.json` — `{"stores":["/path/a",…]}`,
  the extra local projects a machine server auto-hosts at startup. See
  `server::multistore::configured_store_paths` (HS2-87).
- **Export interchange:** `hotsheet-export.json` (`exportVersion`, `project`,
  `tickets[]`) — [07](07-migration.md) §7.2.1; produced by the migrator, consumed by
  `hotsheet-cli import` (`import.rs::ExportFile`).

## Build / test

- **Build:** `cargo build`
- **Rust tests:** `cargo nextest run` (fallback `cargo test`)
- **Migrator tests:** `cd migrator && npm install && npx vitest run` (the conformance
  test needs `target/debug/hotsheet-cli` built first; it skips otherwise)
- **Lint (must pass with zero warnings before push):** `cargo fmt --all --check` +
  `cargo lint`; then `npm run lint` in `clients/web`, `migrator`, and
  `spikes/kerf-webawesome`. `.cargo/config.toml` owns the pinned workspace Clippy
  policy; each Node package owns its ESLint flat config. The web and spike configs use
  the Glassbox ESLint/TypeScript/import/TSDoc/Kerf baseline.

## Where do I look for X?

| X | Where |
|---|---|
| Ticket fields / enums | `hotsheet-model/src/{ticket,enums}.rs`, [17](17-ticket-file-format.md) |
| Reading/writing a ticket file | `hotsheet-model/src/format.rs` |
| Slug derivation | `hotsheet-model/src/ids.rs` |
| Store layout / sharding | `hotsheet-ticketing/src/store.rs`, [02](02-ticket-storage.md) §2.3 |
| HS1 → git migration | `migrator/` + `hotsheet-cli/src/import.rs` (stable HS2 ids, close-state normalization; no retained HS1 fields), [07](07-migration.md) |
| Web client UI foundation | `clients/web/src/{dev-server.ts,project-bridge.ts,compatibility.ts,not-working-workflow.ts,components/,ux-demo/,dev-review/}` (dev-only Hono catalog + real components; authenticated server compatibility negotiation; content-anchored, batched html2canvas review overlay and single-commit local CLI bridge in `dev-review/`; application shell composition in `components/{app-shell,workspace-header,page-header,project-tab-bar,project-tab,resizable-region,connection-state-banner,loading-spinner}.*`; shared sidebar rows/headings in `components/{menu-item,menu-header}.*`; board composition in `components/{ticket-board,ticket-board-column,ticket-row}.*`; ticket content reading/editing in `components/{note-card,note-composer,ticket-notes,markdown-preview,markdown-editor,ticket-info-panel,ticket-inspector,ticket-reader,not-working-dialog,pending-attachment-picker}.*`, including provider-gated note create/edit/delete and compensating Not Working evidence submission; shared ticket colors in `components/ticket-state-colors.css`), `spikes/kerf-webawesome/` (Kerf/Web Awesome compatibility proof), [06](06-clients.md), [UX component catalog](ux-components.md), [Dev Review](18-dev-review-tool.md) |
| Adapter seams (Clock/Rng/…) | `hotsheet-ticketing/src/ports.rs`, [12](12-code-organization-and-testing.md) §12.1 |
| AI-tool plugins (loader + first-party) | `hotsheet-plugins/src/lib.rs`, `plugins/`, [05](05-ai-tool-plugins.md) §5.11 |
| Wire DTOs (server + MCP JSON shape) | `hotsheet-ticketing/src/wire.rs` |
| Requirements status | [README.md](README.md) |
| This project's own tickets (dogfood) | A **standalone HS2 store** in its own git repo (separate from this code repo, so ticket churn stays out of code history — docs/02 §2.8 option 2). Migrated from HS1. Read with `hotsheet-cli -C <store> ls`; the store path is per-machine (not committed here — `.mcp.json` is gitignored). |
