# hotsheet-migrator

Disposable, **read-only** exporter that converts a Hot Sheet 1 project's PGLite
database into a portable `hotsheet-export.json`, which the Rust `hotsheet import`
turns into a git store (Hot Sheet 2). See [`docs/07-migration.md`](../docs/07-migration.md).

It opens a **copy** of the datadir — the source cluster is never modified.

## Usage

```bash
cd migrator && npm install
node src/export.mjs <path-to-a-project/.hotsheet> --out hotsheet-export.json
# then, from the repo root:
target/debug/hotsheet-cli -C ./my-store import hotsheet-export.json
```

## Supported Hot Sheet versions

Targets the **5 most recent production releases + the current beta**. These use two
PGLite lines (both Postgres 17):

| Hot Sheet | PGLite | Working DB |
|---|---|---|
| v0.17.2, v0.17.3 | 0.3.x | `template1` |
| v0.18.0, v0.19.0, v0.20.0 | 0.4.x | `postgres` |
| v0.21.0-beta (current beta) | 0.4.x | `postgres` |

### Engine strategy — bundle one, fetch only what's newer

A newer PGLite **reads older datadirs** but not the reverse (a 0.4.x engine opens
0.3.x *and* 0.4.x datadirs; a 0.5.x engine opens neither). So the exporter bundles
**one** engine — the line Hot Sheet ships, `@electric-sql/pglite` **0.4.x** — and
tries it first. It opens every supported HS datadir directly.

Only a datadir written by a PGLite **newer than the bundle** (e.g. **PGLite 0.5.x =
PG18**, a future Hot Sheet) can't be opened; those fall back to
[`pglite-migrate`](https://www.npmjs.com/package/pglite-migrate), which fetches a
pinned, hash-verified matching engine on demand (cached). Bundling the absolute-latest
0.5.x would be *wrong* — it can't read today's 0.3.x/0.4.x data.

- **Old datadirs.** v0.17.x (0.3.x) clusters open directly with the bundled 0.4.x
  engine — no fetch.
- **Working database.** PGLite 0.4.0 moved the default working DB from `template1` to
  `postgres`, so a 0.3.x cluster keeps its tables in `template1`. The opener probes
  both.
- **Column tolerance.** It reads only the ticket columns that exist, so schema
  differences across releases (e.g. `ticket_blocked_by` didn't exist at v0.17.x)
  degrade gracefully instead of erroring.

The newer-than-bundle **fetch** path is validated end-to-end against a real PG18
(0.5.x) datadir, but isn't in the offline test suite because it downloads an engine
(tracked as **HS2-82**). Old + current datadirs are covered offline with real on-disk
clusters (the old one written by a devDependency 0.3.x engine).

## Tests

```bash
npm test   # vitest
```

Covers: the export shape, field/notes/tag/timestamp mapping, `blocked_by` edge
resolution, soft-delete, column tolerance, **real on-disk old (0.3.x) and current
(0.4.x) clusters** read by the bundled engine, and a **cross-language conformance** test that
feeds the exporter's JSON to the real Rust `hotsheet import` (skips if the CLI isn't
built — run `cargo build` first).
