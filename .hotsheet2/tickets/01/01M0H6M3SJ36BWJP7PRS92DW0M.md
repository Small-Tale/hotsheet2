---
id: 01M0H6M3SJ36BWJP7PRS92DW0M
slug: HS2-NF2NHG
title: 'Migrator: cross-major PGLite datadirs via pglite-migrate (PG16/PG18) + fetch-missing-engine'
category: feature
priority: default
status: completed
created_at: 2026-08-20T01:09:33.778Z
updated_at: 2026-08-20T02:21:04.171Z
completed_at: 2026-08-20T02:21:04.171Z
closed_at: 2026-08-20T02:21:04.171Z
close_reason: completed
legacy_number: HS2-82
schema: 1
---

The bundled engine is PGLite 0.4.x (PG17); it physically cannot open a datadir written by a different PG major (PG16 ≈ PGLite 0.3.x, PG18 ≈ 0.5.x) — a WASM abort. The exporter now DETECTS this and fails with a clear message pointing here (migrator/src/export.mjs openWithTickets). To actually handle it, integrate `pglite-migrate` (already a dependency): use its `openDataDir(dir, alias, { fetchMissingEngine: true })` / cross-major COPY flow to logically migrate the source datadir up to the bundled major before export, verifying against the package's hashed engine checksums. Also thread `pgliteOptions.database` for the template1/postgres split. Only meaningfully testable with a real PG16 or PG18 cluster (none on hand now). See migrator README notes + docs/07. Follow-up of HS2-14 / HS2-77.

## Notes

<!-- note: 01M0H6M3T3HE1B9ETGT8QDH88G -->
2026-08-20T01:31:59.688Z — **Scope narrowed (2026-08-20).** The immediate cross-major need — PG16 (HS v0.17.x) alongside PG17 (v0.18.0+) — is now SOLVED and tested: the exporter bundles one engine per major (`pglite-pg16` + `@electric-sql/pglite`) and selects by `PG_VERSION`, with real on-disk PG16 and PG17 cluster tests. So this ticket now covers only **future/unbundled majors** (e.g. PG18 / PGLite 0.5.x).

The pglite-migrate fallback is **already wired** (`openViaPgliteMigrate` in migrator/src/export.mjs uses `openDataDir(..., {fetchMissingEngine:true})`), but it downloads an engine at runtime and there's no PG18 cluster to test against offline, so it's best-effort. Remaining work: add a bundled engine (or validate the fetch path) once Hot Sheet officially ships a release on a new PG major.

<!-- note: 01M0H6M3T3TEWY5H29E41DJ7G6 -->
2026-08-20T01:40:43.988Z — **Design changed (2026-08-20, maintainer):** don't bundle an engine per major. The migrator now bundles **only the latest** PGLite (`@electric-sql/pglite`, PG17) and routes any datadir of a different major through `pglite-migrate` (fetch-missing-engine). So PG16 (v0.17.x, a supported release) now uses the fetch fallback too — the cross-major *read* is offline-tested with a locally-installed PG16 engine (devDependency), but the runtime network **fetch** for a non-bundled major is not offline-tested.

Remaining on this ticket: cover/validate the pglite-migrate fetch acquisition (needs network, or a seeded engine cache in CI), and confirm the fallback end-to-end against a real PG16 project via `exportDatadir` (not just the read path). Scope now = "the network fetch for non-bundled majors," not the earlier per-major bundling.

<!-- note: 01M0H6M3T3JD0GYM1Z2VAE9SRM -->
2026-08-20T02:21:04.171Z — **TL;DR:** The pglite-migrate fetch fallback is wired **and validated end-to-end** against a real PG18 datadir. Along the way I corrected a wrong mental model (it's PGLite forward-compat, not PG major).

### Correction (important)
Empirically, **both PGLite 0.3.x and 0.4.x write `PG_VERSION 17`** — all Postgres 17. The earlier "PG16 vs PG17" framing was wrong. The real property is datadir **forward-compatibility**:

| engine ↓ opens datadir → | 0.3.16 | 0.4.6 | 0.5.5 |
|---|---|---|---|
| 0.4.6 (bundled) | ✅ | ✅ | ❌ |
| 0.5.5 (latest) | ❌ | ❌ | ✅ |

A newer PGLite reads older datadirs, not the reverse. So bundling the **absolute latest (0.5.x) would break** reading all current HS data; the correct bundle is the **0.4.x** line HS ships, which opens every supported datadir (0.3.x + 0.4.x) directly — no fetch for any supported version.

### Fetch validated
A datadir written by a PGLite **newer than the bundle** (PGLite 0.5.5 = **PG18**) — which the bundled 0.4.6 engine genuinely cannot open — was exported successfully: `openViaPgliteMigrate` → `openDataDir(..., {fetchMissingEngine:true})` **downloaded a matching PG18 engine** and read the cluster. Full fallback path proven on real data.

### Left as-is (honestly)
The fetch downloads an engine, so it's **not in the offline CI suite** (would need network or a seeded cache). The forward-compat *read* for old + current datadirs IS covered offline with real on-disk clusters. If we want CI to exercise the fetch, that's a small follow-up (seed pglite-migrate's engine cache) — noting it here rather than filing a separate ticket since it's minor.

Comments/docs/README (docs/07, migrator/README) corrected to the forward-compat model. 11 vitest + 41 Rust tests pass.
