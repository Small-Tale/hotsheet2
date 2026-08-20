# Codebase Map

The AI-read orientation doc: where things are, how to build/test, and where to look
for X. Keep in sync in the same change that adds a file/dir, a command, a schema
field, or a setting. Requirements status lives in [README.md](README.md); the field
schema in [17-ticket-file-format.md](17-ticket-file-format.md).

> **Status:** early implementation. The Rust core model + file format, a filesystem
> store, the `hotsheet` CLI (init/new/ls/show/import), and the Node HS1 exporter
> exist. Server, indexing, sync, clients, and plugins are still design-only.

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
      src/ports.rs           #   Clock, Rng (FileSystem/GitLocal/... to come)
      src/store.rs           #   FsStore: init/open/read/write/list + StoreMetadata
    hotsheet-cli/            # `hotsheet` binary
      src/main.rs            #   clap: init/new/ls/show/import
      src/import.rs          #   hotsheet-export.json -> store (two-pass, idempotent)
  migrator/                  # disposable Node HS1 exporter (docs/07)
    src/export.mjs           #   exportFromDb(db, project) + CLI (opens a datadir copy)
    src/introspect.mjs       #   schema-dump helper
    test/export.test.mjs     #   vitest: synthetic HS1 DB + cross-language conformance
  docs/                      # design docs 00–17 (+ this map)
  .github/workflows/ci.yml   # fmt --check · clippy -D warnings · nextest
```

## Entry points

- **CLI:** `crates/hotsheet-cli/src/main.rs` → binary `hotsheet`. Global `-C/--path`
  selects the store dir. Subcommands: `init`, `new`, `ls`, `show`, `import`.
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
