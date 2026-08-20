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
target/debug/hotsheet -C ./my-store import hotsheet-export.json
```

## Supported Hot Sheet versions

Targets the **5 most recent production releases + the current beta**. These span two
PostgreSQL majors:

| Hot Sheet | PGLite | PG major | Default DB |
|---|---|---|---|
| v0.17.2, v0.17.3 | 0.3.x | **PG16** | `template1` |
| v0.18.0, v0.19.0, v0.20.0 | 0.4.x | **PG17** | `postgres` |
| v0.21.0-beta (current beta) | 0.4.x | **PG17** | `postgres` |

### Engine strategy — bundle one, fetch the rest

The exporter bundles **only the latest PGLite** (`@electric-sql/pglite`, PG17) and
tries it first. A datadir from a *different* Postgres major can't be opened by it, so
those fall back to [`pglite-migrate`](https://www.npmjs.com/package/pglite-migrate),
which fetches a pinned, hash-verified engine matching the datadir on demand (cached
between runs). This keeps the bundle to **one** engine while supporting arbitrarily
old — or future — majors, instead of shipping an engine per version.

- **Cross-major open.** PG17 datadirs (v0.18.0+) open directly with the bundled
  engine; PG16 datadirs (v0.17.x) use the pglite-migrate fetch path.
- **Working database.** PGLite 0.4.0 moved the default working database from
  `template1` to `postgres`, so an older cluster keeps its tables in `template1`. The
  opener probes both.
- **Column tolerance.** It reads only the ticket columns that exist, so schema
  differences across releases (e.g. `ticket_blocked_by` didn't exist at v0.17.x)
  degrade gracefully instead of erroring.

The pglite-migrate **network fetch** for a non-bundled major is best-effort and not
exercised by the offline test suite (that acquisition is pglite-migrate's own tested
concern; tracked as **HS2-82**). The cross-major *read* path is covered offline using
a locally-installed PG16 engine (a devDependency).

## Tests

```bash
npm test   # vitest
```

Covers: the export shape, field/notes/tag/timestamp mapping, `blocked_by` edge
resolution, soft-delete, column tolerance, **real on-disk PG16 and PG17 clusters**
(created with each bundled engine), and a **cross-language conformance** test that
feeds the exporter's JSON to the real Rust `hotsheet import` (skips if the CLI isn't
built — run `cargo build` first).
