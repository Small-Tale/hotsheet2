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
> **`hotsheet-mcp`** shim. Still design-only: server lifecycle/auto-start (HS2-59),
> the `hotsheet-cli reindex` CLI + no-server index maintenance, git-aware fast-path
> reindex, mTLS, terminals, and clients.

## Directory tree

```
hot-sheet2/
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
      src/ops.rs             #   query/create/update/close/claim — the one op impl (CLI+server+MCP)
      src/ports.rs           #   Clock, Rng (FileSystem/GitLocal/... to come)
      src/store.rs           #   FsStore: init/open/read/write/list + StoreMetadata
      src/settings.rs        #   Settings: shared (committed) / local (gitignored) scopes; effective = local over shared
      src/wire.rs            #   wire SSOT: ApiTicket/ApiNote/TicketRow + From<&Ticket> (shared by server + MCP)
    hotsheet-cli/            # two binaries + a shared lib
      src/main.rs            #   `hotsheet-cli`: init/new/ls/show/edit/close/setup/import/doctor/claim-next/release/renew
      src/bin/hotsheet-migrate.rs #   `hotsheet-migrate`: standalone HS1 migrator (spawns Node exporter + imports)
      src/lib.rs             #   shared: run_import / run_migrate / git helpers (pglite-free)
      src/setup.rs           #   `hotsheet-cli setup <tool>`: write a plugin's instructions/skill/MCP config (merge-safe)
      src/plugin.rs          #   `hotsheet-cli plugin list|info|install|verify|remove`: manage + trust-gate external plugins
      src/import.rs          #   hotsheet-export.json -> store (two-pass, idempotent)
      tests/cli.rs, tests/migrate.rs #  E2E for each binary (assert_cmd)
    hotsheet-server/         # `hotsheet-server` binary (axum HTTP + WS)
      src/lib.rs             #   app() router, handlers over ops, ApiTicket DTO, auth, /ws/sync
      src/main.rs            #   bind (loopback only) + serve; prints port + secret
      tests/http.rs          #   in-process HTTP E2E (tower::oneshot)
    hotsheet-mcp/            # `hotsheet-mcp` binary (MCP shim)
      src/lib.rs             #   JSON-RPC handle_message + hotsheet_* tools over a Backend:
                             #     CoreBackend (direct-to-disk, serverless) | HttpBackend (proxy a server)
      src/main.rs            #   stdio JSON-RPC loop; --path <store> (serverless) | --server <url> --secret
    hotsheet-index/          # disposable SQLite + FTS5 index (cache over the store)
      src/lib.rs             #   Index: open_reconciled/reconcile/rebuild/upsert/delete/query + hash_bytes
    hotsheet-aitools/        # AI-tool host (behavioral half): the drive/transport interface
      src/drive.rs           #   Drive trait + Transport/Target/DriveCtx/TurnHandle/DoneReason
      src/host.rs            #   drive_for(plugin) + trigger(): plugins[drive] -> Drive -> registry glue
      src/spawn.rs           #   SpawnDrive (spawn-per-run, Codex `exec` shape) + SpawnDrive::codex()
      src/ports.rs           #   ProcessSpawner/SpawnedProcess (injected) + SpawnSpec
      src/system.rs          #   SystemSpawner (real std::process adapter)
      src/registry.rs        #   ConnectionRegistry: live connections + sliding-window busy tracking
    hotsheet-plugins/        # AI-tool plugin loader + registry (core `plugins` module)
      src/lib.rs             #   Manifest/Plugin, from_dir (bundled) + from_fs_dir (on-disk),
                             #     all_plugins(search_dirs)/find_in/builtin_plugins; ${HOTSHEET_HOME:-~/.hotsheet2}/plugins
      src/tests.rs           #   built-in + on-disk loading, first-party-wins-collision
  plugins/                   # first-party AI-tool plugin dirs, bundled into the binary (docs/05 §5.11)
    claude/                  #   manifest.toml + instructions.md (CLAUDE.md section) + SKILL.md (worklist skill)
    codex/                   #   manifest.toml + instructions.md (AGENTS.md section); no skill, codex-toml MCP
  migrator/                  # disposable Node HS1 exporter (docs/07)
    src/export.mjs           #   exportFromDb(db, project) + CLI (opens a datadir copy)
    src/introspect.mjs       #   schema-dump helper
    test/export.test.mjs     #   vitest: synthetic HS1 DB + cross-language conformance
  test-projects/             # full-binary E2E harnesses (not in-process unit tests)
    e2e-headless-claude.sh   #   headless loop: setup + drive hotsheet-mcp (serverless + server) [HS2-99]
  docs/                      # design docs 00–17 (+ this map)
  .github/workflows/ci.yml   # fmt --check · clippy -D warnings · nextest
```

## Entry points

- **CLI:** `crates/hotsheet-cli/src/main.rs` → binary `hotsheet-cli` (live ticket ops).
  Global `-C/--path` selects the store dir. Subcommands: `init`, `new`, `ls`
  (filters/sort/text), `show`, `edit`, `close`, `setup` (AI-tool setup, headless),
  `plugin` (list/install/remove external plugins), `settings` (get/set/list, shared|local),
  `import`, `doctor`, `claim-next`, `release`, `renew`.
- **Migrator CLI:** `src/bin/hotsheet-migrate.rs` → **separate** binary
  `hotsheet-migrate` (rarely-used, one-time, needs Node). `hotsheet-migrate
  <old/.hotsheet> -C <store>` spawns the Node exporter against a *copy* of the old
  database, then imports. Then `hotsheet-cli`'s `ls`/`show`/`edit`/`new` operate on the
  store. Kept out of `hotsheet-cli` so the live CLI carries no Node/migrator runtime dep.
- **Server:** `hotsheet-server -C <store> [--bind 127.0.0.1:8787] [--secret …]` — HTTP
  REST (`/health`, `/tickets`…) + `/ws/sync`, `X-Hotsheet-Secret` auth (Tier 0,
  loopback only). Over `ops`; in-memory scan (index is HS2-5).
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
  (local, gitignored) — flat key→JSON maps. See `settings.rs::Settings`.
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
