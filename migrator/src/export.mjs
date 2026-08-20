#!/usr/bin/env node
// HS1 (PGLite/Postgres) -> hotsheet-export.json exporter (docs/07 §7.2.1, shape B).
//
// Disposable and read-only: it opens a COPY of the datadir so the source cluster is
// never touched. Supports every HS1 datadir across the last 5 production releases +
// the current beta, which span TWO Postgres majors:
//
//   Hot Sheet v0.17.x  -> PGLite 0.3.x = PG16   (tables live in `template1`)
//   Hot Sheet v0.18.0+ -> PGLite 0.4.x = PG17   (tables live in `postgres`)
//   (v0.21.0-beta is still 0.4.x / PG17)
//
// A PG17 engine physically cannot open a PG16 datadir, so we bundle one engine per
// major and pick by the datadir's PG_VERSION. Unbundled majors (e.g. a future PG18)
// fail with a clear message pointing at `pglite-migrate` (HS2-82).
//
// Usage: node src/export.mjs <path-to-.hotsheet> [--out hotsheet-export.json]

/** The export-file version the Rust importer understands (`docs/07`). */
export const EXPORT_VERSION = 1;

/** Bundled PGLite engine per Postgres major (module specifier). */
export const ENGINE_BY_MAJOR = {
  16: 'pglite-pg16', //         PGLite 0.3.x — Hot Sheet v0.17.x
  17: '@electric-sql/pglite', // PGLite 0.4.x — Hot Sheet v0.18.0 .. v0.21.0-beta
};

/** Ticket columns we read, in output order. Absent columns are skipped (older
 * schemas lack some), so the SELECT never references a column that isn't there. */
const TICKET_COLUMNS = [
  'id', 'ticket_number', 'title', 'details', 'category', 'priority', 'status',
  'up_next', 'tags', 'notes', 'created_at', 'updated_at', 'completed_at',
  'verified_at', 'deleted_at',
];

/**
 * Build the export object from an open PGLite/pg database + project metadata.
 * Column-tolerant: it reads only the ticket columns that exist in this schema.
 * @param {{query: (sql: string, params?: unknown[]) => Promise<{rows: any[]}>}} db
 * @param {{name?: string|null, ticketPrefix?: string|null}} project
 */
export async function exportFromDb(db, project = {}) {
  const present = new Set(
    (
      await db.query(
        `select column_name from information_schema.columns
         where table_schema = 'public' and table_name = 'tickets'`,
      )
    ).rows.map((r) => r.column_name),
  );
  if (!present.has('id') || !present.has('ticket_number')) {
    throw new Error('the `tickets` table is missing id/ticket_number — not an HS1 cluster?');
  }
  const cols = TICKET_COLUMNS.filter((c) => present.has(c));
  const { rows: ticketRows } = await db.query(`select ${cols.join(', ')} from tickets order by id`);

  const numberById = new Map();
  for (const r of ticketRows) numberById.set(r.id, r.ticket_number);

  // Dependency edges (HS1 `ticket_blocked_by`, added after v0.17.x). Read
  // best-effort and warn — rather than abort — if the table/column is absent.
  const blockedBy = new Map(); // ticket_number -> [blocked-on numbers]
  try {
    const { rows } = await db.query(`select ticket_id, blocks_on_ticket_id from ticket_blocked_by`);
    for (const e of rows) {
      const from = numberById.get(e.ticket_id);
      const to = numberById.get(e.blocks_on_ticket_id);
      if (from && to) {
        if (!blockedBy.has(from)) blockedBy.set(from, []);
        blockedBy.get(from).push(to);
      }
    }
  } catch (err) {
    console.warn(
      `warning: could not read ticket_blocked_by (${err.message}); dependency edges not exported`,
    );
  }

  const tickets = ticketRows.map((r) => ({
    ticket_number: r.ticket_number ?? null,
    title: r.title ?? '',
    details: r.details ?? null,
    category: r.category ?? null,
    priority: r.priority ?? null,
    // A soft-deleted ticket migrates as status `deleted` (docs/07 §7.4).
    status: r.deleted_at ? 'deleted' : (r.status ?? null),
    up_next: Boolean(r.up_next),
    tags: asArray(r.tags),
    notes: asArray(r.notes).map((n) => ({
      id: n.id ?? null,
      text: n.text ?? '',
      created_at: iso(n.created_at),
    })),
    blocked_by: blockedBy.get(r.ticket_number) ?? [],
    created_at: iso(r.created_at),
    updated_at: iso(r.updated_at),
    completed_at: iso(r.completed_at),
    verified_at: iso(r.verified_at),
  }));

  return {
    exportVersion: EXPORT_VERSION,
    project: {
      name: project.name ?? null,
      ticketPrefix: project.ticketPrefix ?? 'HS',
    },
    tickets,
  };
}

/**
 * Export one HS1 project's `.hotsheet/` to an export object (and, if `outPath` is
 * given, to that file). Opens a COPY of `<hotsheetDir>/db` with the engine matching
 * its Postgres major, at whichever database holds the tables. Read-only w.r.t. the
 * source. Returns the export object.
 */
export async function exportDatadir(hotsheetDir, outPath) {
  const fs = await import('node:fs');
  const os = await import('node:os');
  const { join } = await import('node:path');

  let project = {};
  const settingsPath = join(hotsheetDir, 'settings.json');
  if (fs.existsSync(settingsPath)) {
    try {
      const s = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
      project = { name: s.appName ?? null, ticketPrefix: s.ticketPrefix ?? null };
    } catch (err) {
      console.warn(`warning: could not read settings.json (${err.message})`);
    }
  }

  const work = fs.mkdtempSync(join(os.tmpdir(), 'hs1-export-'));
  fs.cpSync(join(hotsheetDir, 'db'), work, { recursive: true });
  try {
    const major = readPgMajor(fs, join, work);
    const { db, database } = await openCluster(work, major);
    if (database !== 'postgres') {
      console.log(`(reading from the '${database}' database — cluster predates PGLite 0.4.0)`);
    }
    try {
      const exportObj = await exportFromDb(db, project);
      if (outPath) fs.writeFileSync(outPath, `${JSON.stringify(exportObj, null, 2)}\n`);
      return exportObj;
    } finally {
      await db.close();
    }
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
}

// ---- engine + database selection -------------------------------------------------

function readPgMajor(fs, join, work) {
  try {
    return parseInt(fs.readFileSync(join(work, 'PG_VERSION'), 'utf8').trim(), 10);
  } catch {
    return NaN;
  }
}

/**
 * Open the datadir at whichever database holds the `tickets` table, using the right
 * engine for its Postgres major. A bundled engine (deterministic, offline) is used
 * for a supported major; an unbundled major (e.g. a future PG18) falls back to
 * pglite-migrate fetching a matching engine on demand.
 */
async function openCluster(work, major) {
  const mod = ENGINE_BY_MAJOR[major];
  if (mod) {
    const { PGlite } = await import(mod);
    return openBundled(PGlite, work, major);
  }
  return openViaPgliteMigrate(work, major);
}

/**
 * Probe `postgres` then `template1` with a bundled engine. PGLite 0.4.0 moved the
 * default working database from `template1` to `postgres`, so a PG16-era cluster
 * keeps its tables in `template1` where a plain open never looks.
 */
async function openBundled(PGlite, work, major) {
  let engineError = null;
  for (const database of ['postgres', 'template1']) {
    const db = new PGlite(work, { database });
    try {
      if (await hasTicketsTable(db)) return { db, database };
    } catch (err) {
      if (/abort|initialize|wasm/i.test(String(err && err.message))) engineError = err;
    }
    await db.close().catch(() => {});
  }
  if (engineError) {
    throw new Error(
      `failed to open this PG${major} datadir with its bundled engine ` +
        `(${engineError.message}). The datadir may be corrupt or from an unexpected build.`,
    );
  }
  throw new Error('no `tickets` table found in this cluster (looked in postgres + template1)');
}

/**
 * Fallback for a Postgres major we don't bundle an engine for: let pglite-migrate
 * fetch a pinned, hash-verified engine matching the datadir on demand (opt-in,
 * downloads ~9 MB). Best-effort — exercised only for future majors and not covered
 * by the offline test suite (HS2-82).
 */
async function openViaPgliteMigrate(work, major) {
  const { openDataDir } = await import('pglite-migrate');
  for (const database of ['postgres', 'template1']) {
    let cluster = null;
    try {
      cluster = await openDataDir(work, 'pglite-source', {
        fetchMissingEngine: true,
        pgliteOptions: { database },
      });
      if (await hasTicketsTable(cluster)) return { db: cluster, database };
    } catch {
      // try the next database, then fail with guidance below
    }
    if (cluster) await cluster.close().catch(() => {});
  }
  const supported = Object.keys(ENGINE_BY_MAJOR).join(', ');
  throw new Error(
    `could not open this PostgreSQL major ${major} datadir. The migrator bundles ` +
      `engines for majors ${supported}; pglite-migrate's fetch-missing-engine did not ` +
      `yield a working engine either. See docs/07 / HS2-82.`,
  );
}

async function hasTicketsTable(db) {
  const { rows } = await db.query(
    `select 1 from information_schema.tables
     where table_schema = 'public' and table_name = 'tickets' limit 1`,
  );
  return rows.length > 0;
}

// ---- value coercion --------------------------------------------------------------

/** Coerce a JSON column (already-parsed array, JSON string, or null) to an array. */
function asArray(value) {
  if (value == null) return [];
  if (Array.isArray(value)) return value;
  if (typeof value === 'string') {
    try {
      const parsed = JSON.parse(value);
      return Array.isArray(parsed) ? parsed : [];
    } catch {
      return [];
    }
  }
  return [];
}

/** Normalize a timestamp (JS Date from pg, or a string, or null) to RFC3339/null. */
function iso(value) {
  if (value == null) return null;
  if (value instanceof Date) return value.toISOString();
  return String(value);
}

// ---- CLI -------------------------------------------------------------------------

async function main(argv) {
  const args = argv.slice(2);
  const hotsheetDir = args.find((a) => !a.startsWith('--'));
  const outIdx = args.indexOf('--out');
  const outPath = outIdx >= 0 ? args[outIdx + 1] : 'hotsheet-export.json';

  if (!hotsheetDir) {
    console.error('usage: node src/export.mjs <path-to-.hotsheet> [--out hotsheet-export.json]');
    process.exitCode = 2;
    return;
  }
  const exportObj = await exportDatadir(hotsheetDir, outPath);
  console.log(`Wrote ${exportObj.tickets.length} ticket(s) to ${outPath}`);
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main(process.argv).catch((err) => {
    console.error(err.message);
    process.exitCode = 1;
  });
}
