import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { PGlite } from '@electric-sql/pglite';
import { PGlite as PGlite16 } from 'pglite-pg16';
import { execFileSync } from 'node:child_process';
import { mkdtempSync, rmSync, writeFileSync, existsSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join, dirname } from 'node:path';
import { fileURLToPath } from 'node:url';

import { exportFromDb, exportDatadir } from '../src/export.mjs';

const HERE = dirname(fileURLToPath(import.meta.url));
const REPO_ROOT = join(HERE, '..', '..');

// A minimal stand-in for the HS1 schema (columns observed from the live API).
async function makeHs1Db() {
  const db = new PGlite();
  await db.exec(`
    create table tickets (
      id serial primary key,
      ticket_number text,
      title text not null,
      details text,
      category text,
      priority text,
      status text,
      up_next boolean default false,
      tags jsonb default '[]',
      notes jsonb default '[]',
      created_at timestamptz,
      updated_at timestamptz,
      completed_at timestamptz,
      verified_at timestamptz,
      deleted_at timestamptz
    );
    -- Real HS1 column names (src/db/connection.ts): ticket_id + blocks_on_ticket_id.
    create table ticket_blocked_by (ticket_id int, blocks_on_ticket_id int);

    insert into tickets
      (ticket_number, title, details, category, priority, status, up_next, tags, notes,
       created_at, updated_at, completed_at)
    values
      ('HS2-1', 'Root cause', 'the body', 'bug', 'high', 'completed', false,
       '["ui"]',
       '[{"id":"n_abc","text":"fixed","created_at":"2026-08-01T00:00:00Z"}]',
       '2026-08-01T00:00:00Z', '2026-08-02T00:00:00Z', '2026-08-02T00:00:00Z'),
      ('HS2-2', 'Blocked feature', null, 'feature', 'default', 'started', true,
       '[]', '[]', '2026-08-03T00:00:00Z', '2026-08-03T00:00:00Z', null);

    insert into tickets (ticket_number, title, status, deleted_at, created_at, updated_at)
    values ('HS2-3', 'Trashed', 'started', '2026-08-04T00:00:00Z',
            '2026-08-04T00:00:00Z', '2026-08-04T00:00:00Z');

    -- HS2-2 is blocked by HS2-1 (ids 1 and 2 from the serial).
    insert into ticket_blocked_by (ticket_id, blocks_on_ticket_id) values (2, 1);
  `);
  return db;
}

describe('exportFromDb', () => {
  let db;
  beforeAll(async () => {
    db = await makeHs1Db();
  });
  afterAll(async () => {
    await db.close();
  });

  it('produces the docs/07 export shape', async () => {
    const out = await exportFromDb(db, { name: 'Hot Sheet 2', ticketPrefix: 'HS2' });
    expect(out.exportVersion).toBe(1);
    expect(out.project).toEqual({ name: 'Hot Sheet 2', ticketPrefix: 'HS2' });
    expect(out.tickets).toHaveLength(3);
  });

  it('maps fields, notes, tags, and timestamps', async () => {
    const out = await exportFromDb(db, { ticketPrefix: 'HS2' });
    const root = out.tickets.find((t) => t.ticket_number === 'HS2-1');
    expect(root.status).toBe('completed');
    expect(root.priority).toBe('high');
    expect(root.tags).toEqual(['ui']);
    // Note timestamps come from the JSON column, so they pass through verbatim.
    expect(root.notes).toEqual([
      { id: 'n_abc', text: 'fixed', created_at: '2026-08-01T00:00:00Z' },
    ]);
    expect(root.created_at).toBe('2026-08-01T00:00:00.000Z');
    expect(root.completed_at).toBe('2026-08-02T00:00:00.000Z');
    expect(root.verified_at).toBeNull();
  });

  it('resolves blocked_by edges to ticket numbers', async () => {
    const out = await exportFromDb(db, {});
    const dep = out.tickets.find((t) => t.ticket_number === 'HS2-2');
    expect(dep.blocked_by).toEqual(['HS2-1']);
    expect(dep.up_next).toBe(true);
  });

  it('migrates a soft-deleted ticket as status deleted', async () => {
    const out = await exportFromDb(db, {});
    const trashed = out.tickets.find((t) => t.ticket_number === 'HS2-3');
    expect(trashed.status).toBe('deleted');
  });

  it('degrades gracefully when the blocked_by table is absent', async () => {
    const bare = new PGlite();
    await bare.exec(`create table tickets (
      id serial primary key, ticket_number text, title text not null,
      details text, category text, priority text, status text, up_next boolean,
      tags jsonb, notes jsonb, created_at timestamptz, updated_at timestamptz,
      completed_at timestamptz, verified_at timestamptz, deleted_at timestamptz
    );
    insert into tickets (ticket_number, title) values ('HS2-9', 'lonely');`);
    const out = await exportFromDb(bare, {});
    expect(out.tickets[0].blocked_by).toEqual([]);
    await bare.close();
  });
});

// Cross-language conformance (docs/07 §7.2.1, docs/12 §12.7): the real Rust importer
// must ingest what the exporter produced. Skips when the CLI hasn't been built.
describe('conformance: Rust hotsheet import parses the export', () => {
  const bin = join(REPO_ROOT, 'target', 'debug', 'hotsheet');
  const run = existsSync(bin) ? it : it.skip;

  run('imports the exported JSON without drift', async () => {
    const db = await makeHs1Db();
    const out = await exportFromDb(db, { name: 'Hot Sheet 2', ticketPrefix: 'HS2' });
    await db.close();

    const work = mkdtempSync(join(tmpdir(), 'hs2-conformance-'));
    const exportFile = join(work, 'export.json');
    const store = join(work, 'store');
    writeFileSync(exportFile, JSON.stringify(out));

    try {
      const stdout = execFileSync(bin, ['-C', store, 'import', exportFile], { encoding: 'utf8' });
      expect(stdout).toContain('Imported 3 ticket(s)');

      // The Rust side can list what it wrote (proves every file parses).
      const listed = execFileSync(bin, ['-C', store, 'ls'], { encoding: 'utf8' });
      expect(listed).toContain('Root cause');
      expect(listed).toContain('Blocked feature');
    } finally {
      rmSync(work, { recursive: true, force: true });
    }
  });
});

// Multi-major support: the migrator must open every datadir across the 5 latest
// production releases + the current beta, which span PG16 (v0.17.x, PGLite 0.3.x)
// and PG17 (v0.18.0+, PGLite 0.4.x). We create a REAL on-disk cluster with each
// engine and export it through the version-selecting opener.

// The pre-v0.18 tickets schema: no claim columns, no ticket_blocked_by table.
const OLD_TICKETS_DDL = `
  create table tickets (
    id serial primary key, ticket_number text unique not null,
    title text not null default '', details text not null default '',
    category text not null default 'issue', priority text not null default 'default',
    status text not null default 'not_started', up_next boolean not null default false,
    created_at timestamptz not null default now(), updated_at timestamptz not null default now(),
    completed_at timestamptz, deleted_at timestamptz,
    notes text not null default '', verified_at timestamptz, tags text not null default '[]'
  );`;

async function makeDatadir(EngineClass, ddl, seed) {
  const hs = mkdtempSync(join(tmpdir(), 'hs1-cluster-'));
  const db = new EngineClass(join(hs, 'db'));
  await db.exec(ddl);
  await db.exec(seed);
  await db.close();
  writeFileSync(join(hs, 'settings.json'), JSON.stringify({ appName: 'HS', ticketPrefix: 'HS' }));
  return hs;
}

describe('multi-major on-disk export', () => {
  it(
    'reads a PG16 (v0.17.x) datadir — older schema, tables in template1',
    async () => {
      const hs = await makeDatadir(
        PGlite16,
        OLD_TICKETS_DDL,
        `insert into tickets (ticket_number, title, status, notes)
         values ('HS-1', 'old one', 'completed',
                 '[{"id":"n_1","text":"hi","created_at":"2026-01-01T00:00:00Z"}]'),
                ('HS-2', 'old two', 'not_started', '');`,
      );
      try {
        const out = await exportDatadir(hs, null);
        expect(out.tickets).toHaveLength(2);
        const t1 = out.tickets.find((t) => t.ticket_number === 'HS-1');
        expect(t1.status).toBe('completed');
        expect(t1.notes[0].text).toBe('hi');
        // No ticket_blocked_by table in this era → degrades to empty, no throw.
        expect(t1.blocked_by).toEqual([]);
      } finally {
        rmSync(hs, { recursive: true, force: true });
      }
    },
    30000,
  );

  it(
    'reads a PG17 (v0.18.0+) datadir — tables in postgres, with edges',
    async () => {
      const hs = await makeDatadir(
        PGlite,
        `${OLD_TICKETS_DDL}
         create table ticket_blocked_by (ticket_id int, blocks_on_ticket_id int);`,
        `insert into tickets (ticket_number, title, status)
         values ('HS-1', 'blocker', 'completed'), ('HS-2', 'blocked', 'started');
         insert into ticket_blocked_by (ticket_id, blocks_on_ticket_id) values (2, 1);`,
      );
      try {
        const out = await exportDatadir(hs, null);
        expect(out.tickets).toHaveLength(2);
        const dep = out.tickets.find((t) => t.ticket_number === 'HS-2');
        expect(dep.blocked_by).toEqual(['HS-1']);
      } finally {
        rmSync(hs, { recursive: true, force: true });
      }
    },
    30000,
  );
});

describe('column tolerance', () => {
  it('exports even when optional columns are absent', async () => {
    const db = new PGlite();
    await db.exec(`create table tickets (
      id serial primary key, ticket_number text, title text not null default ''
    );
    insert into tickets (ticket_number, title) values ('HS-1', 'minimal');`);
    const out = await exportFromDb(db, {});
    expect(out.tickets).toHaveLength(1);
    expect(out.tickets[0].verified_at).toBeNull();
    expect(out.tickets[0].status).toBeNull();
    expect(out.tickets[0].tags).toEqual([]);
    await db.close();
  });
});
