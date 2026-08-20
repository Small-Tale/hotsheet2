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
PostgreSQL majors, so the exporter **bundles one PGLite engine per major** and selects
by the datadir's `PG_VERSION`:

| Hot Sheet | PGLite | PG major | Engine (bundled) | Default DB |
|---|---|---|---|---|
| v0.17.2, v0.17.3 | 0.3.x | **PG16** | `pglite-pg16` (npm alias) | `template1` |
| v0.18.0, v0.19.0, v0.20.0 | 0.4.x | **PG17** | `@electric-sql/pglite` | `postgres` |
| v0.21.0-beta (current beta) | 0.4.x | **PG17** | `@electric-sql/pglite` | `postgres` |

Two details this handles automatically:

- **Cross-major open.** A PG17 engine physically cannot open a PG16 datadir; picking
  the engine by `PG_VERSION` is what makes the older releases work.
- **Working database.** PGLite 0.4.0 moved the default working database from
  `template1` to `postgres`, so an older cluster keeps its tables in `template1`. The
  opener probes both.

The **column set is version-tolerant**: it reads only the ticket columns that exist,
so schema differences across releases (e.g. `ticket_blocked_by` didn't exist at
v0.17.x) degrade gracefully instead of erroring.

### Newer/unknown majors

A datadir from a major we don't bundle an engine for (e.g. a future PG18) falls back
to [`pglite-migrate`](https://www.npmjs.com/package/pglite-migrate), which fetches a
pinned, hash-verified engine on demand. That path is best-effort and not covered by
the offline test suite (tracked as **HS2-82**); add a bundled engine for a major once
it's an officially supported Hot Sheet release.

## Tests

```bash
npm test   # vitest
```

Covers: the export shape, field/notes/tag/timestamp mapping, `blocked_by` edge
resolution, soft-delete, column tolerance, **real on-disk PG16 and PG17 clusters**
(created with each bundled engine), and a **cross-language conformance** test that
feeds the exporter's JSON to the real Rust `hotsheet import` (skips if the CLI isn't
built — run `cargo build` first).
