# Codebase Map

The AI-read orientation doc: where things are, how to build/test, and where to look
for X. Keep in sync in the same change that adds a file/dir, a command, a schema
field, or a setting. Requirements status lives in [README.md](README.md); the field
schema in [17-ticket-file-format.md](17-ticket-file-format.md).

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
  rust-toolchain.toml        # pinned stable + rustfmt + clippy
  crates/
    hotsheet-model/          # pure domain model + ticket file format (no I/O)
      src/lib.rs             #   re-exports; SCHEMA_VERSION
      src/enums.rs           #   Priority/Status/CloseReason/NoteKind/ReviewKind
      src/ids.rs             #   Ulid re-export + derive_slug (FNV-1a -> Crockford)
      src/ticket.rs          #   Ticket/Note/ReviewRequest/ExternalLink; Ticket::new
      src/timestamp.rs       #   Timestamp: lenient RFC3339 (raw text + parsed instant)
      src/format.rs          #   parse_file / to_file_string (YAML frontmatter + body + notes)
    hotsheet-ticketing/      # engine crate (sync API, injected ports)
      src/lib.rs             #   mint_ulid(clock, rng)
      src/ops.rs             #   query/create/update/close/claim/copy_ticket/move_ticket/assign — the one op impl (CLI+server+MCP); TicketQuery.assignee filter + keyset page_after (HS2-20/HS2-TCDTCH)
      src/identity.rs        #   current-user identity: current_user_email (git user.email) + resolve_me — the `me` sentinel for assignee/review filters (docs/10 §10.3, HS2-TCDTCH)
      src/activity.rs        #   cross-tool activity events (docs/15, HS2-KP31ZE): ActivityEvent/Kind/Importance + bounded rolling store (activity/recent/<day>.jsonl) + timeline() + claude_activity/codex_activity mappers
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
      src/overlay.rs         #   LocalOverlay: per-user Tier B data under gitignored <store>/local/ (read-tracking; docs/02 §2.11, HS2-21)
      src/wire.rs            #   wire SSOT: ApiTicket/ApiNote/TicketRow (compact list row, body-optional) + From<&Ticket> (shared by server + MCP)
      src/worklist.rs        #   derived worklist.md: render(tickets)→md + regenerate(store) (gitignored, watcher-regenerated; docs/03 §3.6, HS2-90)
    hotsheet-cli/            # two binaries + a shared lib
      src/main.rs            #   `hotsheet-cli`: init (incl. --standalone [--at/--remote] one-shot create+link, HS2-77YTS1)/link/new/ls/show/edit/close/copy/move/assign/people/read/setup/plugin/settings/import/sync/merge-driver/doctor/reindex/worklist/metrics/serve/cert/claim-next/release/renew/trigger/work/permission-hook; resolve_store_path finds a standalone store without -C (-C > $HOTSHEET_STORE > .hotsheet/store link, HS2-5CXKZ0); `cert init/issue/revoke` manages Tier-1 mTLS material (HS2-VT3JMF)
      src/permission_hook.rs #   Claude PreToolUse hook adapter (HS2-YMR9HE): pure map of Claude hook JSON → bridge (tool,action) + allow/deny/ask decision; the `permission-hook` cmd POSTs /permissions/ask ($HOTSHEET_SERVER/$HOTSHEET_SECRET), else `ask`
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
      src/lib.rs             #   app() router, handlers over ops (store-generic do_create/update/close), ApiTicket DTO (incl. copied_from/moved_to_store/moved_at provenance), auth, /ws/sync (ChangeEvent tagged by store) + /ws/poll long-poll fallback (sequenced EventLog ring + cursor, HS2-P3P3CC), GET/POST /permissions live human round-trip (SharedPermissionBridge in AppState + permission_asked event nudge + durable Always-rules via with_permission_rules, HS2-9R9YZW), GET /connections (what the driving loop is running, via a shared drive_registry, HS2-TCV3BF), POST /permissions/ask (raise a blocking request — the Claude-hook asking side, HS2-YMR9HE), GET/POST /terminals + /terminals/{id}[/input] (open/list/read-scrollback/input/kill PTYs over the TerminalManager, HS2-A6R5QV; POST /terminals `connect:<tool>` registers the terminal as a Pty connection + feeds its busy→drive_registry, HS2-4M67VN) + WS /terminals/{id}/attach (live: replay scrollback then stream new output + forward input; Text frames carry {resize} viewport size claims → the SizeArbiter and stream {pty_size,driven_by} decisions; query-secret auth, HS2-XTTTMV/HS2-BD7Q74) — with `serve --terminal-broker` the terminal ops AND the live WS attach (streaming output + size claims + the connect busy feed) route through the detached broker so terminals survive a restart (HS2-ERT00F), GET/POST /activity (ingest an activity event + read the per-ticket/session timeline over ticketing::activity, HS2-KP31ZE), POST /announce (ephemeral store-level WS broadcast — ChangeEvent{kind:"announce",message}; not persisted in the poll ring, HS2-HHDNTH), POST /setup/{tool} (HS2-91), POST /batch (bulk update), GET/POST /stores + scoped /stores/{id}/tickets[/{id}[/close]] read+write + GET /resolve/{ulid} cross-store + POST /tickets/{id}/copy|move {to,confirm} cross-store copy/move (multi-store, HS2-87/HS2-S4H2AM)
      src/main.rs            #   bind + serve (loopback = Tier-0 plaintext; off-loopback = Tier-1 mTLS via tls::build_server_config + serve_tls, HS2-VT3JMF); instance file + writer lock + graceful shutdown + --stop (lifecycle, HS2-59); prints port + secret
      src/tls.rs             #   Tier-1 mTLS (HS2-VT3JMF): build_server_config (rustls ServerConfig requiring a client cert) + RevocationCheckingVerifier (wraps WebPkiClientVerifier + a revoked-fingerprint gate) + serve_tls (manual tokio-rustls acceptor + hyper-util conn loop, since axum 0.8 has no built-in TLS)
      src/dist_work_loop.rs  #   server-hosted distributed driving loop (HS2-DTPX2V/HS2-1TY7GC): DistWorkConfig (off by default; tool/worker/lease/max_in_flight/participation) + work_pass (filter + NoRemote skip + in-flight bound + sweep_expired, over distwork::work_once) + spawn_dist_work_loop + live_drive (SafeTrigger per claimed ticket, passes the permission bridge + records TurnEvent::Usage via metrics::record_priced attributed to the ticket, HS2-0WCRZY) + outcome_from_turn (done/open/progress -> WorkOutcome, stall guard); wired into main.rs via --drive-tool
      src/lifecycle.rs       #   server lifecycle: InstanceInfo registry + discovery (one machine server writes a discovery file per hosted store, HS2-87 topology A), per-store index-writer lock, stop_instance (HS2-59)
      src/multistore.rs      #   StoreHost: registry of served stores (StoreEntry{store,index}) keyed by a short URL id + StoreInfo listing (HS2-87). Per-store fs-watcher via WatchTarget; cross-store resolve; configured_store_paths (stores.json startup discovery); file-backed index_path_for in persistent mode
      src/sync_loop.rs       #   background sync loop: sync_once per hosted store on interval + kick-on-write + exponential backoff (sync_all/next_delay pure + tested; docs/02 §2.12, HS2-19 follow-up)
      src/terminal_broker.rs #   server↔detached-broker integration (HS2-ERT00F): TerminalBroker::ensure (discover/spawn the broker per project under ${HOTSHEET_HOME}/broker) + call (per-request BrokerClient round-trip); `serve --terminal-broker` routes /terminals ops + the live WS attach (bridged to a BrokerStream — broker_attach_loop in lib.rs) + the connect busy feed (polls the broker's Read) through it so terminals survive a server restart; idle-GC/health is a follow-up (HS2-SV3XS8)
      tests/http.rs          #   in-process HTTP E2E (tower::oneshot)
    hotsheet-mcp/            # `hotsheet-mcp` binary (MCP shim)
      src/lib.rs             #   JSON-RPC handle_message + hotsheet_* tools over a Backend
                             #     (query/get/create/update/close/batch/claim_next/release/renew/copy/move):
                             #     CoreBackend (direct-to-disk, serverless) | HttpBackend (proxy a server)
      src/main.rs            #   stdio JSON-RPC loop; --path <store> (serverless) | --server <url> --secret
    hotsheet-tls/            # Tier-1 mTLS material (rcgen-only, no rustls; used by CLI + server) — docs/04 §4.6, HS2-VT3JMF
      src/lib.rs             #   per-project CA + device-cert issuance + revocation: Paths (${HOTSHEET_HOME}/tls/<project-id>/), init_ca, issue_device (DeviceCert{cert,key,ca,fingerprint}), revoke_device + load_revoked, fingerprint_of_der; project_id matches lifecycle's path-hash
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
      src/lib.rs             #   Index (SCHEMA_VERSION 3): open_reconciled/reconcile (git-diff fast path)/rebuild/upsert/delete/query + hash_bytes; facets: tags + assignees + reviews tables; filters incl. assignee/review_requested (facet joins) + claimed + blocked/unblocked (json_each over blocked_by vs done set) + created/updated date-range + moved-tombstones-hidden-by-default; keyset page_after paging (HS2-89/HS2-T84F9F/HS2-TCDTCH)
    hotsheet-aitools/        # AI-tool host (behavioral half): the drive/transport interface
      src/drive.rs           #   Drive trait (+ service() -> BackingService accessor) + BackingService trait + Transport/Target/DriveCtx/TurnHandle/DoneReason
      src/host.rs            #   drive_for(plugin) + trigger(): plugins[drive] -> Drive -> registry glue
      src/appserver.rs       #   AppServerDrive (Codex persistent daemon: turn on a resumed thread; with_daemon() exposes its CodexDaemonService via Drive::service) + AppServerClient port
      src/codex.rs           #   CodexAppServer: real AppServerClient (codex 0.148 JSON-RPC engine) + StdioTransport (live-verified) / UdsWsTransport (shared daemon: WebSocket over the control socket, HS2-115) + codex_control_socket_path + ensure_codex_daemon + CodexDaemonService (BackingService prestart, tool-id-free, HS2-V5Z2EY) + turn_usage (parse token usage from turn/completed → drive::Usage, the codex-usage metrics mapper, HS2-8PSAFE); loopback + scripted-WS daemon tests
      src/claude.rs          #   ClaudeChannelDrive + ClaudeChannel: turn injected into a running `claude` stream-json session, async TurnEvent stream; ClaudeStreamTransport + claude_result_usage (parse token usage from the `result` event → drive::Usage, the claude-usage metrics mapper, HS2-TJ8FGR); scripted-claude tests
      src/procio.rs          #   StreamChild: shared piped-stdio plumbing (spawn -> RpcWriter/RpcReader) for the stream transports
      src/live.rs            #   run_trigger → TurnDone{reason, session_id}: spawn a REAL tool per its [drive] transport (codex app-server: StdioTransport, or shared-daemon UdsWsTransport when --shared-daemon), build DriveCtx, pump_turn one turn (behind `hotsheet-cli trigger`); pump_turn heartbeats ConnectionRegistry busy at a LIVE clock per streamed event + emits TurnEvent::Usage + idle on Done (HS2-34X6BW); surfaces the tool session/thread id for cross-turn resume (HS2-3C1XK3)
      src/launch_safety.rs   #   HS2-103 safety: hotsheet->hotsheet-cli PATH shim, assert_no_hs1, absolute hotsheet-mcp path, IsolatedCodexHome (auto MCP-free CODEX_HOME, HS2-YRDQNX) — moved here so CLI + server share it (HS2-1TY7GC)
      src/safe_trigger.rs    #   SafeTrigger + prepare_trigger: resolve a tool + assemble launch safety once, run_turn(on_event sink, conn_id) per turn; shared by `hotsheet-cli trigger`/`work` and the server driving loop (HS2-1TY7GC)
      src/spawn.rs           #   SpawnDrive (spawn-per-run, Codex `exec` shape) + SpawnDrive::codex()
      src/ports.rs           #   ProcessSpawner/SpawnedProcess + AppServerClient/Turn + RpcTransport/Reader/Writer (injected) + SpawnSpec
      src/system.rs          #   SystemSpawner (real std::process adapter)
      src/registry.rs        #   ConnectionRegistry: live connections + sliding-window busy tracking
      src/permission.rs      #   PermissionBridge: FIFO request queue (concurrent-preserving, fixes HS1 overwrite) + allow-rules (once/session/always) + route-back (docs/05 §5.7, HS2-11)
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
  (incl. `--blocked-by`/`--clear-blocked-by`), `close`, `setup` (AI-tool setup, headless),
  `plugin` (list/install/remove external plugins), `settings` (get/set/list, shared|local),
  `import`, `doctor`, `claim-next`, `release`, `renew`, `trigger` (the headless "play":
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

- **Ticket file:** `tickets/<2-char shard>/<ULID>.md` — YAML frontmatter + Markdown
  body (`details`) + optional `## Notes`. Schema: [17](17-ticket-file-format.md).
- **Store metadata:** `hotsheet-store.json` (camelCase: `schemaVersion`,
  `ticketPrefix`, `idStrategy`, `shard`). See `store.rs::StoreMetadata`.
- **Settings:** `hotsheet-settings.json` (shared, committed) + `hotsheet-settings.local.json`
  (local, gitignored), plus **global** `${HOTSHEET_HOME}/settings.json` (machine-wide) —
  flat key→JSON maps, effective precedence global<shared<local. See `settings.rs::Settings`.
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
- **Lint (must pass before push):** `cargo fmt --all --check` + `cargo clippy
  --all-targets --all-features -- -D warnings`

## Where do I look for X?

| X | Where |
|---|---|
| Ticket fields / enums | `hotsheet-model/src/{ticket,enums}.rs`, [17](17-ticket-file-format.md) |
| Reading/writing a ticket file | `hotsheet-model/src/format.rs` |
| Slug derivation | `hotsheet-model/src/ids.rs` |
| Store layout / sharding | `hotsheet-ticketing/src/store.rs`, [02](02-ticket-storage.md) §2.3 |
| HS1 → git migration | `migrator/` + `hotsheet-cli/src/import.rs`, [07](07-migration.md) |
| Adapter seams (Clock/Rng/…) | `hotsheet-ticketing/src/ports.rs`, [12](12-code-organization-and-testing.md) §12.1 |
| AI-tool plugins (loader + first-party) | `hotsheet-plugins/src/lib.rs`, `plugins/`, [05](05-ai-tool-plugins.md) §5.11 |
| Wire DTOs (server + MCP JSON shape) | `hotsheet-ticketing/src/wire.rs` |
| Requirements status | [README.md](README.md) |
| This project's own tickets (dogfood) | A **standalone HS2 store** in its own git repo (separate from this code repo, so ticket churn stays out of code history — docs/02 §2.8 option 2). Migrated from HS1. Read with `hotsheet-cli -C <store> ls`; the store path is per-machine (not committed here — `.mcp.json` is gitignored). |
