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
// Engine strategy: bundle only the LATEST PGLite (`@electric-sql/pglite`) and try it
// first. A datadir from a different Postgres major can't be opened by it (a WASM
// abort), so those fall back to `pglite-migrate`, which fetches a pinned,
// hash-verified engine matching the datadir on demand. This keeps the bundle to one
// engine while supporting arbitrarily old (or new) majors.
//
// Usage: node src/export.mjs <path-to-.hotsheet> [--out hotsheet-export.json]

/** The export-file version the Rust importer understands (`docs/07`). */
export const EXPORT_VERSION = 1;

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

  // Promoted attachments (draft_id IS NULL, or no draft_id column on old schemas).
  // Best-effort: the table didn't exist on the very earliest schemas.
  const attByTicket = new Map(); // ticket_number -> [{original_filename, stored_path}]
  try {
    const cols = new Set(
      (
        await db.query(
          `select column_name from information_schema.columns
           where table_schema = 'public' and table_name = 'attachments'`,
        )
      ).rows.map((r) => r.column_name),
    );
    if (cols.has('ticket_id') && cols.has('original_filename') && cols.has('stored_path')) {
      const where = cols.has('draft_id') ? 'where draft_id is null' : '';
      const { rows } = await db.query(
        `select ticket_id, original_filename, stored_path from attachments ${where} order by id`,
      );
      for (const a of rows) {
        const num = numberById.get(a.ticket_id);
        if (!num) continue;
        if (!attByTicket.has(num)) attByTicket.set(num, []);
        attByTicket.get(num).push({
          original_filename: a.original_filename ?? null,
          stored_path: a.stored_path,
        });
      }
    }
  } catch (err) {
    console.warn(`warning: could not read attachments (${err.message}); attachments not exported`);
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
    attachments: attByTicket.get(r.ticket_number) ?? [],
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
  const path = await import('node:path');
  const { join } = path;

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
    const { db, database } = await openCluster(work);
    if (database !== 'postgres') {
      console.log(`(reading from the '${database}' database — cluster predates PGLite 0.4.0)`);
    }
    try {
      const exportObj = await exportFromDb(db, project);
      if (outPath) {
        stageAttachments(fs, path, hotsheetDir, outPath, exportObj);
        fs.writeFileSync(outPath, `${JSON.stringify(exportObj, null, 2)}\n`);
      }
      return exportObj;
    } finally {
      await db.close();
    }
  } finally {
    fs.rmSync(work, { recursive: true, force: true });
  }
}

// ---- attachment staging ----------------------------------------------------------

/**
 * Copy each ticket's attachment files next to the export JSON and rewrite their
 * `stored_path` to that staged, JSON-relative location, so the importer can find and
 * copy them into the store. Files that can't be found on disk are dropped with a
 * warning. `exportObj` is mutated in place.
 */
function stageAttachments(fs, path, hotsheetDir, outPath, exportObj) {
  const outDir = path.dirname(outPath);
  let idx = 0;
  let staged = 0;
  let missing = 0;
  for (const t of exportObj.tickets) {
    const kept = [];
    for (const att of t.attachments ?? []) {
      const src = resolveAttachmentSource(fs, path, hotsheetDir, att.stored_path);
      if (!src) {
        missing += 1;
        continue;
      }
      const name = path.basename(att.original_filename || att.stored_path);
      const rel = path.join('attachments', String(idx), name);
      const dest = path.join(outDir, rel);
      fs.mkdirSync(path.dirname(dest), { recursive: true });
      fs.copyFileSync(src, dest);
      kept.push({ original_filename: att.original_filename ?? name, stored_path: rel });
      idx += 1;
      staged += 1;
    }
    t.attachments = kept;
  }
  if (staged) console.log(`Staged ${staged} attachment file(s) beside ${path.basename(outPath)}`);
  if (missing) console.warn(`warning: ${missing} attachment file(s) not found on disk; skipped`);
}

/**
 * HS1 `stored_path` is an absolute path under `<.hotsheet>/attachments/`. Resolve by
 * basename against this project's attachments dir first (so a project copied to
 * another machine still works), then fall back to the raw stored path.
 */
function resolveAttachmentSource(fs, path, hotsheetDir, storedPath) {
  const byBasename = path.join(hotsheetDir, 'attachments', path.basename(storedPath));
  if (fs.existsSync(byBasename)) return byBasename;
  if (fs.existsSync(storedPath)) return storedPath;
  return null;
}

// ---- engine + database selection -------------------------------------------------

/**
 * Open the datadir at whichever database holds the `tickets` table. Try the bundled
 * (latest) PGLite engine first; if the datadir is a different Postgres major (a WASM
 * abort), fall back to pglite-migrate, which fetches a matching engine on demand.
 */
async function openCluster(work) {
  const bundled = await tryBundled(work);
  if (bundled.db) return bundled;
  if (bundled.majorMismatch) return openViaPgliteMigrate(work);
  throw new Error('no `tickets` table found in this cluster (looked in postgres + template1)');
}

/**
 * Probe `postgres` then `template1` with the bundled engine. PGLite 0.4.0 moved the
 * default working database from `template1` to `postgres`, so a PG16-era cluster
 * keeps its tables in `template1` where a plain open never looks. Returns
 * `{majorMismatch:true}` if the engine can't open the datadir (wrong major).
 */
async function tryBundled(work) {
  const { PGlite } = await import('@electric-sql/pglite');
  for (const database of ['postgres', 'template1']) {
    const db = new PGlite(work, { database });
    try {
      if (await hasTicketsTable(db)) return { db, database };
    } catch (err) {
      await db.close().catch(() => {});
      // A WASM "Aborted()" means the on-disk major ≠ the bundled engine's major;
      // a second database would abort identically, so stop and hand off.
      if (isEngineMajorMismatch(err)) return { majorMismatch: true };
      throw err;
    }
    await db.close().catch(() => {});
  }
  return {}; // opened cleanly, but neither database has a `tickets` table
}

/**
 * Fallback for a datadir whose Postgres major differs from the bundled engine: let
 * pglite-migrate fetch a pinned, hash-verified engine matching the datadir on demand
 * (opt-in; downloads ~9 MB, cached). This is what supports older/newer majors without
 * bundling extra engines. Best-effort — the network-fetch acquisition itself isn't
 * covered by the offline test suite (HS2-82).
 */
async function openViaPgliteMigrate(work) {
  const { openDataDir } = await import('pglite-migrate');
  for (const database of ['postgres', 'template1']) {
    let cluster = null;
    try {
      // A non-resolving specifier forces acquisition by the datadir's own major.
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
  throw new Error(
    'could not open this datadir: the bundled engine is a different Postgres major and ' +
      "pglite-migrate's fetch-missing-engine did not yield a working engine. See docs/07 / HS2-82.",
  );
}

function isEngineMajorMismatch(err) {
  return /abort|initialize|wasm|EngineMismatch/i.test(String(err && err.message));
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
