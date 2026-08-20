#!/usr/bin/env node
// HS1 (PGLite/Postgres) -> hotsheet-export.json exporter (docs/07 §7.2.1, shape B).
//
// Disposable and read-only: it opens a COPY of the datadir so the source cluster is
// never touched. The pure `exportFromDb` is unit-tested against a synthetic HS1 DB;
// the CLI wrapper below handles the copy + settings + file output.
//
// Usage: node src/export.mjs <path-to-.hotsheet> [--out hotsheet-export.json]

/** The export-file version the Rust importer understands (`docs/07`). */
export const EXPORT_VERSION = 1;

/**
 * Build the export object from an open PGLite/pg database + project metadata.
 * @param {{query: (sql: string, params?: unknown[]) => Promise<{rows: any[]}>}} db
 * @param {{name?: string|null, ticketPrefix?: string|null}} project
 */
export async function exportFromDb(db, project = {}) {
  const { rows: ticketRows } = await db.query(`
    select id, ticket_number, title, details, category, priority, status,
           up_next, tags, notes, created_at, updated_at, completed_at,
           verified_at, deleted_at
    from tickets
    order by id
  `);

  const numberById = new Map();
  for (const r of ticketRows) numberById.set(r.id, r.ticket_number);

  // Dependency edges live in a join table. Its exact shape varies across HS1
  // versions, so read it best-effort and warn (rather than abort) if it's absent.
  const blockedBy = new Map(); // ticket_number -> [blocked-by numbers]
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

  const { PGlite } = await import('@electric-sql/pglite');
  const { cpSync, mkdtempSync, rmSync, writeFileSync, readFileSync, existsSync } = await import(
    'node:fs'
  );
  const { tmpdir } = await import('node:os');
  const { join } = await import('node:path');

  // Read project settings for the display name + ticket prefix.
  let project = {};
  const settingsPath = join(hotsheetDir, 'settings.json');
  if (existsSync(settingsPath)) {
    try {
      const s = JSON.parse(readFileSync(settingsPath, 'utf8'));
      project = { name: s.appName ?? null, ticketPrefix: s.ticketPrefix ?? null };
    } catch (err) {
      console.warn(`warning: could not read settings.json (${err.message})`);
    }
  }

  // Open a COPY of the datadir so the live cluster is never modified.
  const work = mkdtempSync(join(tmpdir(), 'hs1-export-'));
  cpSync(join(hotsheetDir, 'db'), work, { recursive: true });
  const { db, database } = await openWithTickets(PGlite, work);
  if (database !== 'postgres') {
    console.log(`(reading from the '${database}' database — cluster predates PGLite 0.4.0)`);
  }
  try {
    const exportObj = await exportFromDb(db, project);
    writeFileSync(outPath, `${JSON.stringify(exportObj, null, 2)}\n`);
    console.log(`Wrote ${exportObj.tickets.length} ticket(s) to ${outPath}`);
  } finally {
    await db.close();
    rmSync(work, { recursive: true, force: true });
  }
}

/**
 * Open the cluster at the database that actually holds the `tickets` table. PGLite
 * 0.4.0 moved the default working database from `template1` to `postgres`, so a
 * cluster written by an older PGLite keeps its tables in `template1` where a plain
 * open never looks. Probe both.
 *
 * The bundled PGLite engine is PG17 (0.4.x). A datadir written by a *different* PG
 * major (PG16 ≈ PGLite 0.3.x, PG18 ≈ 0.5.x) physically cannot be opened by this
 * engine — that cross-major case is bridged by `pglite-migrate` (see the
 * cross-major follow-up ticket); here we detect it and fail with a clear message
 * rather than an opaque WASM abort.
 */
async function openWithTickets(PGlite, work) {
  let engineError = null;
  for (const database of ['postgres', 'template1']) {
    const db = new PGlite(work, { database });
    try {
      const { rows } = await db.query(
        `select 1 from information_schema.tables
         where table_schema = 'public' and table_name = 'tickets' limit 1`,
      );
      if (rows.length) return { db, database };
    } catch (err) {
      // A WASM "Aborted()" here means the on-disk PG major ≠ this engine's major.
      if (/abort|initialize|wasm/i.test(String(err && err.message))) engineError = err;
    }
    await db.close().catch(() => {});
  }

  if (engineError) {
    const { readFileSync } = await import('node:fs');
    const { join } = await import('node:path');
    let major = 'unknown';
    try {
      major = readFileSync(join(work, 'PG_VERSION'), 'utf8').trim();
    } catch {
      /* ignore */
    }
    throw new Error(
      `this datadir is PostgreSQL major ${major}, which the bundled PGLite engine ` +
        `(PG17 / 0.4.x) cannot open. Migrate it across majors first with pglite-migrate, ` +
        `then re-run the export. (original: ${engineError.message})`,
    );
  }
  throw new Error('no `tickets` table found in this cluster (looked in postgres + template1)');
}

if (import.meta.url === `file://${process.argv[1]}`) {
  main(process.argv).catch((err) => {
    console.error(err);
    process.exitCode = 1;
  });
}
