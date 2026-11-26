#!/usr/bin/env node
// HS1 (PGLite/Postgres) -> hotsheet-export.json exporter (docs/07 §7.2.1, shape B).
//
// Disposable and read-only: it opens a COPY of the datadir so the source cluster is
// never touched. Supports every HS1 datadir across the last 5 production releases +
// the current beta:
//
//   Hot Sheet v0.17.x  -> PGLite 0.3.x   (tables live in `template1`)
//   Hot Sheet v0.18.0+ -> PGLite 0.4.x   (tables live in `postgres`)
//   (all of the above are Postgres 17; v0.21.0-beta is still 0.4.x)
//
// Engine strategy — bundle ONE engine (the PGLite line Hot Sheet ships,
// `@electric-sql/pglite` 0.4.x) and try it first. A newer PGLite reads older
// datadirs, so 0.4.x opens every supported HS datadir (both 0.3.x and 0.4.x). Only a
// datadir written by a PGLite *newer* than the bundle — e.g. PGLite 0.5.x (PG18), a
// future Hot Sheet — can't be opened; those fall back to `pglite-migrate`, which
// fetches a pinned, hash-verified matching engine on demand. (The 0.5.x engine, by
// contrast, cannot read 0.3.x/0.4.x datadirs, which is why we bundle 0.4.x, not the
// absolute latest.)
//
// Usage: node src/export.mjs <path-to-.hotsheet> [--out hotsheet-export.json] [--progress-json]

import nodeFs from 'node:fs';
import nodeOs from 'node:os';
import nodePath from 'node:path';
import { fileURLToPath } from 'node:url';
import { parseArgs } from 'node:util';

/** The export-file version the Rust importer understands (`docs/07`). */
export const EXPORT_VERSION = 1;

/** The `--progress-json` event schema version `hotsheet-migrate` reads (`docs/07`). */
export const PROGRESS_VERSION = 1;

/** The Postgres major of the bundled `@electric-sql/pglite` 0.4.x engine. A test pins
 * it to the installed engine's `server_version` (HS2-D0V2NB). */
export const BUNDLED_PG_MAJOR = 17;

/** Command-line usage, printed on every usage error and for `--help` (HS2-6JNDRY). */
export const USAGE = 'usage: node src/export.mjs <path-to-.hotsheet> [--out hotsheet-export.json] [--progress-json]';

function reportWarning(observe, warning) {
  console.warn(warning);
  observe({ version: PROGRESS_VERSION, phase: 'warning', warning });
}

/** Ticket columns we read, in output order. Absent columns are skipped (older
 * schemas lack some), so the SELECT never references a column that isn't there. */
const TICKET_COLUMNS = [
  'id',
  'ticket_number',
  'title',
  'details',
  'category',
  'priority',
  'status',
  'up_next',
  'tags',
  'notes',
  'created_at',
  'updated_at',
  'completed_at',
  'verified_at',
  'deleted_at',
];

/**
 * Build the export object from an open PGLite/pg database + project metadata.
 * Column-tolerant: it reads only the ticket columns that exist in this schema. Rows
 * are untrusted: `checkTicketRow` and `iso` reject shapes the importer cannot store.
 * @param {{query: (sql: string, params?: unknown[]) => Promise<{rows: Record<string, unknown>[]}>}} db
 * @param {{name?: string|null, ticketPrefix?: string|null}} project
 */
export async function exportFromDb(db, project = {}, settings = {}, observe = () => {}) {
  observe({ version: PROGRESS_VERSION, phase: 'read_database' });
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
    reportWarning(observe, `warning: could not read ticket_blocked_by (${err.message}); dependency edges not exported`);
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
    reportWarning(observe, `warning: could not read attachments (${err.message}); attachments not exported`);
  }

  observe({
    version: PROGRESS_VERSION,
    phase: 'export_tickets',
    completed: 0,
    total: ticketRows.length,
    unit: 'tickets',
  });
  const tickets = ticketRows.map((r, index) => {
    // HS2-D0V2NB: reject rows the importer cannot represent instead of guessing.
    checkTicketRow(r);
    const label = rowLabel(r);
    const at = (column) => iso(r[column], `${label}: ${column}`);
    const ticket = {
      ticket_number: r.ticket_number,
      title: r.title,
      details: r.details ?? null,
      category: r.category ?? null,
      priority: r.priority ?? null,
      // A soft-deleted ticket migrates as status `deleted` (docs/07 §7.4).
      status: r.deleted_at ? 'deleted' : (r.status ?? null),
      up_next: Boolean(r.up_next),
      tags: asArray(r.tags),
      notes: asArray(r.notes).map((note, noteIndex) => exportNote(note, noteIndex, label)),
      blocked_by: blockedBy.get(r.ticket_number) ?? [],
      attachments: attByTicket.get(r.ticket_number) ?? [],
      created_at: at('created_at'),
      updated_at: at('updated_at'),
      completed_at: at('completed_at'),
      verified_at: at('verified_at'),
      deleted_at: at('deleted_at'),
    };
    observe({
      version: PROGRESS_VERSION,
      phase: 'export_tickets',
      completed: index + 1,
      total: ticketRows.length,
      unit: 'tickets',
    });
    return ticket;
  });

  return {
    exportVersion: EXPORT_VERSION,
    project: {
      name: project.name ?? null,
      ticketPrefix: project.ticketPrefix ?? 'HS',
      ...(project.sourceRoot ? { sourceRoot: project.sourceRoot } : {}),
    },
    settings,
    tickets,
  };
}

/**
 * Export one HS1 project's `.hotsheet/` to an export object (and, if `outPath` is
 * given, to that file). Opens a COPY of `<hotsheetDir>/db` with the engine matching
 * its Postgres major, at whichever database holds the tables. Read-only w.r.t. the
 * source. Returns the export object.
 */
export async function exportDatadir(hotsheetDir, outPath, observe = () => {}) {
  const fs = nodeFs;
  const path = nodePath;
  const { join } = path;
  const datadir = join(hotsheetDir, 'db');
  if (!fs.existsSync(datadir) || !fs.statSync(datadir).isDirectory()) {
    // HS2-6JNDRY: name the expected layout instead of a bare ENOENT from the copy.
    throw new Error(`no HS1 database directory at ${datadir}; pass the project's .hotsheet directory`);
  }

  let project = { sourceRoot: path.resolve(hotsheetDir, '..') };
  let settings = {};
  const settingsPath = join(hotsheetDir, 'settings.json');
  if (fs.existsSync(settingsPath)) {
    try {
      settings = JSON.parse(fs.readFileSync(settingsPath, 'utf8'));
      project = {
        name: settings.appName ?? null,
        ticketPrefix: settings.ticketPrefix ?? null,
        sourceRoot: path.resolve(hotsheetDir, '..'),
      };
    } catch (err) {
      reportWarning(observe, `warning: could not read settings.json (${err.message})`);
    }
  }

  // HS1's effective custom-command tree can include a machine-local replacement or
  // tree delta. Export only that effective key from settings.local.json: the other
  // local values are machine/runtime details that have never been part of the
  // portable project export.
  const localSettingsPath = join(hotsheetDir, 'settings.local.json');
  if (fs.existsSync(localSettingsPath)) {
    try {
      const local = JSON.parse(fs.readFileSync(localSettingsPath, 'utf8'));
      if (Object.hasOwn(local, 'custom_commands')) {
        settings.custom_commands = resolveCustomCommands(settings.custom_commands, local.custom_commands);
      }
    } catch (err) {
      reportWarning(observe, `warning: could not read settings.local.json (${err.message})`);
    }
  }

  const work = fs.mkdtempSync(join(nodeOs.tmpdir(), 'hs1-export-'));
  try {
    copyDatabase(fs, path, datadir, work, observe);
    observe({ version: PROGRESS_VERSION, phase: 'open_database' });
    const { db, database } = await openCluster(work);
    if (database !== 'postgres') {
      console.error(`(reading from the '${database}' database — cluster predates PGLite 0.4.0)`);
    }
    try {
      const exportObj = await exportFromDb(db, project, settings, observe);
      if (outPath) {
        stageAttachments(fs, path, hotsheetDir, outPath, exportObj, observe);
        observe({ version: PROGRESS_VERSION, phase: 'write_export' });
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

// ---- HS1 custom-command settings ------------------------------------------------

/** Coerce HS1's native or legacy-JSON-string command tree to an array. */
function asCommandTree(value) {
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

function commandId(item) {
  return typeof item?.id === 'string' ? item.id : '';
}

function commandGroup(item) {
  return typeof item === 'object' && item !== null && 'type' in item;
}

function cloneCommandItem(item) {
  if (commandGroup(item)) {
    return {
      ...item,
      children: Array.isArray(item.children) ? item.children.map((child) => ({ ...child })) : [],
    };
  }
  return { ...item };
}

/**
 * Resolve the effective HS1 custom-command tree exactly like HS1's settings layer:
 * a local array replaces shared wholesale; a local object is a tree delta (including
 * empty `{}`); absent local data leaves the shared tree unchanged.
 */
export function resolveCustomCommands(sharedValue, localValue) {
  const shared = asCommandTree(sharedValue);
  if (localValue === undefined) return shared.map(cloneCommandItem);
  if (Array.isArray(localValue) || typeof localValue === 'string') {
    return asCommandTree(localValue).map(cloneCommandItem);
  }
  if (typeof localValue !== 'object' || localValue === null) return [];

  const hidden = new Set(Array.isArray(localValue.hidden) ? localValue.hidden : []);
  const overrides =
    typeof localValue.overrides === 'object' && localValue.overrides !== null ? localValue.overrides : {};
  const childAdded =
    typeof localValue.childAdded === 'object' && localValue.childAdded !== null ? localValue.childAdded : {};
  const out = [];
  const seenGroups = new Set();

  for (const item of shared) {
    const id = commandId(item);
    if (hidden.has(id)) continue;
    const override = typeof overrides[id] === 'object' && overrides[id] !== null ? overrides[id] : {};
    if (commandGroup(item)) {
      seenGroups.add(id);
      const children = [];
      for (const child of Array.isArray(item.children) ? item.children : []) {
        const childId = commandId(child);
        if (hidden.has(childId)) continue;
        const childOverride =
          typeof overrides[childId] === 'object' && overrides[childId] !== null ? overrides[childId] : {};
        children.push({ ...child, ...childOverride });
      }
      const addition = childAdded[id];
      if (Array.isArray(addition?.children)) {
        children.push(...addition.children.map((child) => ({ ...child })));
      }
      out.push({ ...item, ...override, children });
    } else {
      out.push({ ...item, ...override });
    }
  }

  if (Array.isArray(localValue.added)) {
    out.push(...localValue.added.map(cloneCommandItem));
  }
  for (const [id, addition] of Object.entries(childAdded)) {
    if (seenGroups.has(id) || !Array.isArray(addition?.children)) continue;
    out.push({
      type: 'group',
      id: typeof addition.group?.id === 'string' ? addition.group.id : id,
      name: typeof addition.group?.name === 'string' ? addition.group.name : '',
      children: addition.children.map((child) => ({ ...child })),
    });
  }
  return out;
}

// ---- attachment staging ----------------------------------------------------------

/**
 * Copy each ticket's attachment files next to the export JSON and rewrite their
 * `stored_path` to that staged, JSON-relative location, so the importer can find and
 * copy them into the store. Missing referenced files fail before export mutation.
 * `exportObj` is mutated in place only after complete source discovery.
 */
export function stageAttachments(fs, path, hotsheetDir, outPath, exportObj, observe = () => {}) {
  const outDir = path.dirname(outPath);
  const plan = [];
  // Discover the complete source set before mutating the export. Missing payloads
  // must not disappear from evidence and accidentally authorize source cleanup.
  for (const ticket of exportObj.tickets) {
    for (const attachment of ticket.attachments ?? []) {
      const { src, outside } = resolveAttachmentSource(fs, path, hotsheetDir, attachment.stored_path);
      if (outside) {
        throw new Error(
          `Referenced HS1 attachment is outside ${path.join(hotsheetDir, 'attachments')}: ${attachment.stored_path}. Move the payload into that directory before importing; no completion or cleanup proof was recorded.`,
        );
      }
      if (!src || !fs.statSync(src).isFile()) {
        throw new Error(
          `Referenced HS1 attachment is missing: ${attachment.stored_path}. Restore the source payload before importing; no completion or cleanup proof was recorded.`,
        );
      }
      plan.push({ attachment, src, size: fs.statSync(src).size });
    }
  }
  let completed = 0;
  let total = plan.reduce((sum, entry) => sum + entry.size, 0);
  observe({ version: PROGRESS_VERSION, phase: 'stage_attachments', completed, total, unit: 'bytes' });
  for (const [index, { attachment, src, size }] of plan.entries()) {
    const name = path.basename(attachment.original_filename || attachment.stored_path);
    const rel = ['attachments', String(index), name].join('/');
    const dest = path.join(outDir, 'attachments', String(index), name);
    fs.mkdirSync(path.dirname(dest), { recursive: true });
    fs.copyFileSync(src, dest);
    const copied = fs.statSync(dest).size;
    total += copied - size;
    completed += copied;
    attachment.original_filename ??= name;
    attachment.stored_path = rel;
    observe({ version: PROGRESS_VERSION, phase: 'stage_attachments', completed, total, unit: 'bytes' });
  }
  if (plan.length) console.error(`Staged ${plan.length} attachment file(s) beside ${path.basename(outPath)}`);
}

/** Inventory then copy the database without opening or modifying its source. Byte
 * counters advance at successful file boundaries; symlinks retain cpSync semantics. */
export function copyDatabase(fs, path, source, destination, observe = () => {}) {
  observe({ version: PROGRESS_VERSION, phase: 'discover_database' });
  const entries = [];
  const visit = (relative) => {
    const from = path.join(source, relative);
    const info = fs.lstatSync(from);
    entries.push({ relative, info });
    if (info.isDirectory()) for (const name of fs.readdirSync(from).sort()) visit(path.join(relative, name));
  };
  visit('');
  let completed = 0;
  let total = entries.reduce((sum, entry) => sum + (entry.info.isFile() ? entry.info.size : 0), 0);
  observe({ version: PROGRESS_VERSION, phase: 'copy_database', completed, total, unit: 'bytes' });
  for (const { relative, info } of entries) {
    const from = path.join(source, relative),
      to = path.join(destination, relative);
    if (info.isDirectory()) fs.mkdirSync(to, { recursive: true, mode: info.mode });
    else {
      fs.cpSync(from, to);
      if (info.isFile()) {
        const copied = fs.statSync(to).size;
        total += copied - info.size;
        completed += copied;
        observe({ version: PROGRESS_VERSION, phase: 'copy_database', completed, total, unit: 'bytes' });
      }
    }
  }
}

/**
 * HS1 `stored_path` is an absolute path under `<.hotsheet>/attachments/`. Resolve by
 * basename against this project's attachments dir first (so a project copied to
 * another machine still works), then fall back to the recorded path only when it
 * resolves (after symbolic links, when the filesystem can say) inside that same
 * directory (HS2-N250VD). A crafted HS1 database must never stage an arbitrary local
 * file into a Git-pushed ticket store. Returns `{ src }` or `{ outside: true }`.
 */
export function resolveAttachmentSource(fs, path, hotsheetDir, storedPath) {
  const root = path.resolve(hotsheetDir, 'attachments');
  const realRoot = realpathOr(fs, root);
  const inside = (candidate) => {
    const relative = path.relative(realRoot, realpathOr(fs, candidate));
    return relative !== '' && !relative.startsWith('..') && !path.isAbsolute(relative);
  };
  for (const candidate of [path.join(root, path.basename(storedPath)), path.resolve(root, storedPath)]) {
    if (fs.existsSync(candidate)) return inside(candidate) ? { src: candidate } : { outside: true };
  }
  return {};
}

/** `fs.realpathSync(file)` when the filesystem provides it, else the lexical path. */
function realpathOr(fs, file) {
  if (typeof fs.realpathSync !== 'function') return file;
  try {
    return fs.realpathSync(file);
  } catch {
    return file;
  }
}

// ---- engine + database selection -------------------------------------------------

/**
 * Open the datadir at whichever database holds the `tickets` table. Try the bundled
 * engine first (it reads its own + older datadirs). If the datadir was written by a
 * newer PGLite than the bundle, the bundled engine can't initialize (a WASM abort),
 * so fall back to pglite-migrate, which fetches a matching engine on demand.
 */
async function openCluster(work) {
  const bundled = await tryBundled(work);
  if (bundled.db) return bundled;
  if (bundled.tooNew) return openViaPgliteMigrate(work);
  throw new Error('no `tickets` table found in this cluster (looked in postgres + template1)');
}

/**
 * Probe `postgres` then `template1` with the bundled engine. PGLite 0.4.0 moved the
 * default working database from `template1` to `postgres`, so an older (0.3.x)
 * cluster keeps its tables in `template1` where a plain open never looks. Returns
 * `{tooNew:true}` only when the bundled engine cannot initialize a datadir stamped
 * with another Postgres major; a same-major failure is reported as damage
 * (HS2-D0V2NB).
 */
async function tryBundled(work) {
  const { PGlite } = await import('@electric-sql/pglite');
  const major = readDatadirMajor(nodeFs, nodePath, work);
  for (const database of ['postgres', 'template1']) {
    const db = new PGlite(work, { database });
    try {
      if (await hasTicketsTable(db)) return { db, database };
    } catch (err) {
      await db.close().catch(() => {});
      // A second database would fail identically, so stop: hand off a different
      // major to pglite-migrate, and report anything else without a download.
      if (shouldFetchEngine(err, major)) return { tooNew: true };
      if (isBundledInitFailure(err)) {
        const stamp = major === null ? 'no readable PG_VERSION' : `PG_VERSION ${major}`;
        throw new Error(
          `could not open the HS1 database (${stamp}) with the bundled PostgreSQL ${BUNDLED_PG_MAJOR} ` +
            `engine: ${errorText(err)}. The database looks damaged; check that Hot Sheet 1 still opens ` +
            'this project, or restore .hotsheet/db from a backup. No engine was downloaded.',
          { cause: err },
        );
      }
      throw err;
    }
    await db.close().catch(() => {});
  }
  return {}; // opened cleanly, but neither database has a `tickets` table
}

/**
 * Fallback for a datadir written by a PGLite newer than the bundle (e.g. PGLite
 * 0.5.x / PG18): let pglite-migrate fetch a pinned, hash-verified engine matching the
 * datadir on demand (opt-in; downloads ~9 MB, cached). Validated end-to-end against a
 * real PG18 datadir; the network fetch isn't exercised by the offline suite (HS2-82).
 */
async function openViaPgliteMigrate(work) {
  const { openDataDir } = await import('pglite-migrate');
  const failures = [];
  for (const database of ['postgres', 'template1']) {
    let cluster = null;
    try {
      // A non-resolving specifier forces acquisition by the datadir's own major.
      cluster = await openDataDir(work, 'pglite-source', {
        fetchMissingEngine: true,
        pgliteOptions: { database },
      });
      if (await hasTicketsTable(cluster)) return { db: cluster, database };
    } catch (err) {
      // Try the next database; keep the cause for the guidance below (HS2-6JNDRY).
      failures.push(`${database}: ${errorText(err)}`);
    }
    if (cluster) await cluster.close().catch(() => {});
  }
  const causes = failures.length ? ` Causes: ${failures.join('; ')}.` : '';
  throw new Error(
    'could not open this datadir: the bundled engine is a different Postgres major and ' +
      "pglite-migrate's fetch-missing-engine did not yield a working engine. See docs/07 / HS2-82." +
      causes,
  );
}

/**
 * Whether `err` is one of the bundled engine's measured initialization failures
 * (HS2-D0V2NB): `PGlite failed to initialize properly`, or a WASM `RuntimeError:
 * Aborted()`. PGLite 0.4.x throws the same text for a datadir of another Postgres
 * major as for a damaged one, so this alone never authorizes an engine download.
 */
export function isBundledInitFailure(err) {
  const message = err && typeof err.message === 'string' ? err.message : '';
  if (message.startsWith('PGlite failed to initialize')) return true;
  return err?.name === 'RuntimeError' && message.startsWith('Aborted(');
}

/**
 * Whether a bundled-engine failure should hand off to pglite-migrate's on-demand
 * engine (HS2-D0V2NB): only an initialization failure on a datadir whose own
 * `PG_VERSION` names a Postgres major other than the bundled one. A same-major or
 * unstamped datadir that fails to open is damaged, and fetching an engine cannot help.
 */
export function shouldFetchEngine(err, datadirMajor) {
  return isBundledInitFailure(err) && Number.isInteger(datadirMajor) && datadirMajor !== BUNDLED_PG_MAJOR;
}

/** The Postgres major stamped in `<datadir>/PG_VERSION`, or null when absent/unparseable. */
export function readDatadirMajor(fs, path, datadir) {
  let raw;
  try {
    raw = fs.readFileSync(path.join(datadir, 'PG_VERSION'), 'utf8').trim();
  } catch {
    return null;
  }
  return /^[1-9]\d{0,2}$/.test(raw) ? Number(raw) : null;
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

/** A value's JSON-ish kind for shape errors; never echoes ticket content. */
function kindOf(value) {
  if (value === null) return 'null';
  if (Array.isArray(value)) return 'array';
  if (value instanceof Date) return 'Date';
  return typeof value;
}

/** The label HS1 shape errors use for a row: its ticket number, else its row id. */
function rowLabel(row) {
  return typeof row.ticket_number === 'string' && row.ticket_number.trim()
    ? `HS1 ticket ${row.ticket_number}`
    : `HS1 ticket row id ${JSON.stringify(row.id ?? null)}`;
}

/**
 * Reject an HS1 ticket row whose shape the importer cannot represent (HS2-D0V2NB):
 * a missing/blank `ticket_number`, a non-text `title`, or a non-text optional text
 * column. Messages name the row and the offending kind, never the content.
 */
export function checkTicketRow(row) {
  if (typeof row.ticket_number !== 'string' || !row.ticket_number.trim()) {
    throw new Error(`${rowLabel(row)}: ticket_number must be non-empty text, got ${kindOf(row.ticket_number)}`);
  }
  if (typeof row.title !== 'string') {
    throw new Error(`${rowLabel(row)}: title must be text, got ${kindOf(row.title)}`);
  }
  for (const column of ['details', 'category', 'priority', 'status']) {
    if (row[column] != null && typeof row[column] !== 'string') {
      throw new Error(`${rowLabel(row)}: ${column} must be text or null, got ${kindOf(row[column])}`);
    }
  }
}

/** One HS1 note entry, shape-checked: an object whose text is text or absent. */
function exportNote(note, index, label) {
  const where = `${label}: note ${index + 1}`;
  if (typeof note !== 'object' || note === null || Array.isArray(note)) {
    throw new Error(`${where} must be an object, got ${kindOf(note)}`);
  }
  if (note.text != null && typeof note.text !== 'string') {
    throw new Error(`${where} text must be text, got ${kindOf(note.text)}`);
  }
  return {
    id: note.id ?? null,
    text: note.text ?? '',
    created_at: iso(note.created_at, `${where} created_at`),
  };
}

// RFC 3339 / Postgres `timestamptz` text: a date, `T` or space, a time, and a zone.
const TIMESTAMP_TEXT =
  /^(\d{4})-(\d{2})-(\d{2})[Tt ](\d{2}):(\d{2}):(\d{2})(\.\d{1,9})?(?:([Zz])|([+-])(\d{2})(?::?(\d{2}))?)$/;
const RFC3339_TEXT = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(\.\d{1,9})?(Z|[+-]\d{2}:\d{2})$/;

/**
 * Normalize an HS1 timestamp to RFC 3339 or null (HS2-D0V2NB). A valid `Date` (pg's
 * `timestamptz`) becomes ISO; an RFC 3339 string (HS1 note `toISOString()`) stays
 * verbatim; Postgres text form (`2026-08-01 00:00:00+00`) normalizes to ISO. Anything
 * else — an invalid `Date`, a zone-less or out-of-range string, a number — rejects the
 * export: the importer stores timestamp text verbatim, so passing it through would
 * corrupt the ticket store.
 * @param {unknown} value
 * @param {string} [label] where the value came from, for the error
 */
export function iso(value, label = 'timestamp') {
  if (value == null) return null;
  if (value instanceof Date) {
    if (!Number.isNaN(value.getTime())) return value.toISOString();
    throw new Error(`${label} is an invalid date`);
  }
  if (typeof value === 'string') {
    const text = value.trim();
    const match = TIMESTAMP_TEXT.exec(text);
    if (match && validTimestampFields(match)) {
      if (RFC3339_TEXT.test(text)) return text;
      const [, year, month, day, hh, mm, ss, fraction = '', z, sign, zh, zm = '00'] = match;
      const zone = z ? 'Z' : `${sign}${zh}:${zm}`;
      return new Date(`${year}-${month}-${day}T${hh}:${mm}:${ss}${fraction}${zone}`).toISOString();
    }
    throw new Error(`${label} is not an RFC 3339 timestamp: ${JSON.stringify(text.slice(0, 40))}`);
  }
  throw new Error(`${label} must be a timestamp, got ${kindOf(value)}`);
}

/** Calendar/clock range checks `Date.parse` skips (it accepts Feb 30 and 24:00). */
function validTimestampFields(match) {
  const [year, month, day, hour, minute, second] = match.slice(1, 7).map(Number);
  const zoneHour = match[10] === undefined ? 0 : Number(match[10]);
  const zoneMinute = match[11] === undefined ? 0 : Number(match[11]);
  const daysInMonth = new Date(Date.UTC(year, month, 0)).getUTCDate();
  return (
    month >= 1 &&
    month <= 12 &&
    day >= 1 &&
    day <= daysInMonth &&
    hour <= 23 &&
    minute <= 59 &&
    second <= 59 &&
    zoneHour <= 23 &&
    zoneMinute <= 59
  );
}

// ---- CLI -------------------------------------------------------------------------

/** A non-empty, human-readable message for any thrown value (HS2-6JNDRY). */
export function errorText(err) {
  const message = err && typeof err.message === 'string' ? err.message.trim() : '';
  if (message) return message;
  const text = String(err ?? '').trim();
  return text && text !== '[object Object]' ? text : 'export failed with an unknown error';
}

/**
 * Parse the exporter's arguments strictly (HS2-6JNDRY). Returns
 * `{ hotsheetDir, outPath, progressJson }`, `{ help: true }`, or `{ error }` for a usage
 * error: an unknown flag, `--out` without a value (or followed by another flag), an
 * empty or repeated `--out`, a value on `--progress-json`, or zero or several datadirs.
 * Options may appear before or after the datadir; `--` ends option parsing.
 * @param {string[]} args the arguments after the script path
 */
export function parseCliArgs(args) {
  let parsed;
  try {
    parsed = parseArgs({
      args,
      options: {
        out: { type: 'string' },
        'progress-json': { type: 'boolean' },
        help: { type: 'boolean', short: 'h' },
      },
      allowPositionals: true,
      strict: true,
      tokens: true,
    });
  } catch (err) {
    return { error: errorText(err).split('\n')[0] };
  }
  const { values, positionals, tokens } = parsed;
  if (values.help) return { help: true };
  const outs = tokens.filter((token) => token.kind === 'option' && token.name === 'out');
  if (outs.length > 1) return { error: "Option '--out' was given more than once" };
  if (values.out === '') return { error: "Option '--out' needs a non-empty file path" };
  if (positionals.length === 0) return { error: 'Missing the path to an HS1 .hotsheet directory' };
  if (positionals.length > 1) {
    return { error: `Expected one .hotsheet directory, got ${positionals.length}: ${positionals.join(', ')}` };
  }
  return {
    hotsheetDir: positionals[0],
    outPath: values.out ?? 'hotsheet-export.json',
    progressJson: values['progress-json'] === true,
  };
}

async function main(argv) {
  const cli = parseCliArgs(argv.slice(2));
  if (cli.help) {
    console.log(USAGE);
    return;
  }
  if (cli.error) {
    console.error(`hotsheet-export: ${cli.error}`);
    console.error(USAGE);
    process.exitCode = 2;
    return;
  }
  const { hotsheetDir, outPath, progressJson: machine } = cli;
  let previousPhase = '',
    previousTime = 0;
  const observe = (event) => {
    if (!machine) return;
    const now = Date.now();
    if (
      event.phase !== previousPhase ||
      event.completed === event.total ||
      event.warning ||
      now - previousTime >= 100
    ) {
      process.stdout.write(`${JSON.stringify(event)}\n`);
      previousPhase = event.phase;
      previousTime = now;
    }
  };
  try {
    const exportObj = await exportDatadir(hotsheetDir, outPath, observe);
    console.error(`Wrote ${exportObj.tickets.length} ticket(s) to ${outPath}`);
  } catch (error) {
    if (machine) {
      process.stdout.write(
        `${JSON.stringify({ version: PROGRESS_VERSION, phase: 'failed', error: errorText(error) })}\n`,
      );
    }
    throw error;
  }
}

/**
 * Whether Node is running `moduleUrl` as its entry script `argv1` (HS2-WE1AW4). A hand-built
 * `file://${argv1}` never equals the module URL on Windows (`file:///C:/…` against
 * `C:\…`), so compare canonical native paths, which also resolves an npm bin symlink.
 */
export function isDirectEntry(
  moduleUrl,
  argv1,
  { realpath = nodeFs.realpathSync.native, toPath = fileURLToPath } = {},
) {
  if (typeof argv1 !== 'string' || argv1 === '') return false;
  const canonical = (file) => {
    try {
      return realpath(file);
    } catch {
      return nodePath.resolve(file);
    }
  };
  return canonical(toPath(moduleUrl)) === canonical(argv1);
}

if (isDirectEntry(import.meta.url, process.argv[1])) {
  main(process.argv).catch((err) => {
    console.error(`hotsheet-export: ${errorText(err)}`);
    process.exitCode = 1;
  });
}
