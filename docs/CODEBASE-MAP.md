# Codebase Map

The AI-read orientation doc: where things are, how to build/test, and where to look
for X. Keep in sync in the same change that adds a file/dir, a command, a schema
field, or a setting. Requirements status lives in [README.md](README.md); the field
schema in [17-ticket-file-format.md](17-ticket-file-format.md).

> **Status:** early implementation. Built: the Rust core model + file format, a
> filesystem store, the shared engine `ops`, the SQLite/FTS **index** + the server's
> **filesystem watcher** (live reindex), the `hotsheet` CLI + `hotsheet-migrate`,
> the Node HS1 exporter, the **`hotsheet-server`** (index-backed HTTP REST + WS,
> loopback auth; **file-backed index restored + reconciled on launch**), and the
> **`hotsheet-mcp`** shim. Still design-only: server lifecycle/auto-start (HS2-59),
> the `hotsheet reindex` CLI + no-server index maintenance, git-aware fast-path
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
    hotsheet-cli/            # two binaries + a shared lib
      src/main.rs            #   `hotsheet`: init/new/ls/show/edit/close/import/doctor/claim-next/release/renew
      src/bin/hotsheet-migrate.rs #   `hotsheet-migrate`: standalone HS1 migrator (spawns Node exporter + imports)
      src/lib.rs             #   shared: run_import / run_migrate / git helpers (pglite-free)
      src/import.rs          #   hotsheet-export.json -> store (two-pass, idempotent)
      tests/cli.rs, tests/migrate.rs #  E2E for each binary (assert_cmd)
    hotsheet-server/         # `hotsheet-server` binary (axum HTTP + WS)
      src/lib.rs             #   app() router, handlers over ops, ApiTicket DTO, auth, /ws/sync
      src/main.rs            #   bind (loopback only) + serve; prints port + secret
      tests/http.rs          #   in-process HTTP E2E (tower::oneshot)
    hotsheet-mcp/            # `hotsheet-mcp` binary (MCP shim)
      src/lib.rs             #   JSON-RPC handle_message + hotsheet_* tools -> HttpBackend
      src/main.rs            #   stdio JSON-RPC loop; --server + --secret
    hotsheet-index/          # disposable SQLite + FTS5 index (cache over the store)
      src/lib.rs             #   Index: open_reconciled/reconcile/rebuild/upsert/delete/query + hash_bytes
  migrator/                  # disposable Node HS1 exporter (docs/07)
    src/export.mjs           #   exportFromDb(db, project) + CLI (opens a datadir copy)
    src/introspect.mjs       #   schema-dump helper
    test/export.test.mjs     #   vitest: synthetic HS1 DB + cross-language conformance
  docs/                      # design docs 00–17 (+ this map)
  .github/workflows/ci.yml   # fmt --check · clippy -D warnings · nextest
```

## Entry points

- **CLI:** `crates/hotsheet-cli/src/main.rs` → binary `hotsheet` (live ticket ops).
  Global `-C/--path` selects the store dir. Subcommands: `init`, `new`, `ls`
  (filters/sort/text), `show`, `edit`, `close`, `import`, `doctor`, `claim-next`,
  `release`, `renew`.
- **Migrator CLI:** `src/bin/hotsheet-migrate.rs` → **separate** binary
  `hotsheet-migrate` (rarely-used, one-time, needs Node). `hotsheet-migrate
  <old/.hotsheet> -C <store>` spawns the Node exporter against a *copy* of the old
  database, then imports. Then `hotsheet`'s `ls`/`show`/`edit`/`new` operate on the
  store. Kept out of `hotsheet` so the live CLI carries no Node/migrator runtime dep.
- **Server:** `hotsheet-server -C <store> [--bind 127.0.0.1:8787] [--secret …]` — HTTP
  REST (`/health`, `/tickets`…) + `/ws/sync`, `X-Hotsheet-Secret` auth (Tier 0,
  loopback only). Over `ops`; in-memory scan (index is HS2-5).
- **MCP shim:** `hotsheet-mcp --server <url> --secret <s>` — stdio JSON-RPC exposing
  the `hotsheet_*` tools, proxying the server. An AI tool spawns it per project.
- **Migrator:** `migrator/src/export.mjs` → `node src/export.mjs <.hotsheet> [--out …]`.
- **Library:** `hotsheet_model::{parse_file, to_file_string, Ticket}` is the format
  SSOT; `hotsheet_ticketing::FsStore` is the on-disk store.

## Data / formats

- **Ticket file:** `tickets/<2-char shard>/<ULID>.md` — YAML frontmatter + Markdown
  body (`details`) + optional `## Notes`. Schema: [17](17-ticket-file-format.md).
- **Store metadata:** `hotsheet-store.json` (camelCase: `schemaVersion`,
  `ticketPrefix`, `idStrategy`, `shard`). See `store.rs::StoreMetadata`.
- **Export interchange:** `hotsheet-export.json` (`exportVersion`, `project`,
  `tickets[]`) — [07](07-migration.md) §7.2.1; produced by the migrator, consumed by
  `hotsheet import` (`import.rs::ExportFile`).

## Build / test

- **Build:** `cargo build`
- **Rust tests:** `cargo nextest run` (fallback `cargo test`)
- **Migrator tests:** `cd migrator && npm install && npx vitest run` (the conformance
  test needs `target/debug/hotsheet` built first; it skips otherwise)
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
| Requirements status | [README.md](README.md) |
