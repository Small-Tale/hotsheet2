// HS2-D0V2NB: the pglite-migrate engine download is reserved for a datadir stamped with
// another Postgres major. The bundled PGLite throws the same initialization error for a
// damaged datadir, so a damaged PG17 copy must fail with guidance and never fetch.
// All clusters here are synthetic, written by the bundled engine in a temp directory.
import { describe, it, expect, beforeAll, afterAll, beforeEach, vi } from 'vitest';
import { PGlite } from '@electric-sql/pglite';
import { cpSync, mkdtempSync, rmSync, writeFileSync } from 'node:fs';
import * as nodeFs from 'node:fs';
import { tmpdir } from 'node:os';
import * as nodePath from 'node:path';
import { join } from 'node:path';

const openDataDir = vi.fn();
vi.mock('pglite-migrate', () => ({ openDataDir }));

const { BUNDLED_PG_MAJOR, exportDatadir, isBundledInitFailure, readDatadirMajor, shouldFetchEngine } =
  await import('../src/export.mjs');

// The bundled engine's measured failures (PGLite 0.4.6): one message for a datadir of
// another major and for a damaged one, and a WASM abort when files are missing.
const INIT_FAILURE = new Error('PGlite failed to initialize properly');
const WASM_ABORT = Object.assign(new Error('Aborted(). Build with -sASSERTIONS for more info.'), {
  name: 'RuntimeError',
});

describe('engine fallback classification (HS2-D0V2NB)', () => {
  it('pins the bundled major to the installed engine', async () => {
    const db = new PGlite();
    try {
      const { rows } = await db.query(`select current_setting('server_version') as v`);
      expect(Number.parseInt(rows[0].v, 10)).toBe(BUNDLED_PG_MAJOR);
    } finally {
      await db.close();
    }
  });

  it('recognizes only the bundled engine initialization signatures', () => {
    expect(isBundledInitFailure(INIT_FAILURE)).toBe(true);
    expect(isBundledInitFailure(WASM_ABORT)).toBe(true);
    for (const unrelated of [
      new Error('could not read file "base/5/1259": wasm memory access out of bounds'),
      new Error('Aborted() by a plain Error, not the WASM runtime'),
      new Error('EngineMismatchError: engine major 18'),
      new Error('relation "tickets" does not exist'),
      Object.assign(new Error('unreachable'), { name: 'RuntimeError' }),
      new Error(''),
      null,
      undefined,
      'PGlite failed to initialize properly',
    ]) {
      expect(isBundledInitFailure(unrelated)).toBe(false);
    }
  });

  it.each([
    ['newer major', INIT_FAILURE, 18, true],
    ['older major', INIT_FAILURE, 16, true],
    ['newer major, WASM abort', WASM_ABORT, 18, true],
    ['bundled major (damaged)', INIT_FAILURE, BUNDLED_PG_MAJOR, false],
    ['bundled major, WASM abort (damaged)', WASM_ABORT, BUNDLED_PG_MAJOR, false],
    ['unreadable PG_VERSION', INIT_FAILURE, null, false],
    ['unrelated error on another major', new Error('disk I/O error'), 18, false],
  ])('%s -> fetch %s', (_name, err, major, expected) => {
    expect(shouldFetchEngine(err, major)).toBe(expected);
  });

  it('reads the PG_VERSION major and rejects absent or malformed stamps', () => {
    const dir = mkdtempSync(join(tmpdir(), 'hs2-pgversion-'));
    try {
      expect(readDatadirMajor(nodeFs, nodePath, dir)).toBeNull();
      for (const [raw, major] of [
        ['17\n', 17],
        [' 18 ', 18],
        ['xx\n', null],
        ['', null],
        ['17.2', null],
        ['0', null],
        ['-1', null],
      ]) {
        writeFileSync(join(dir, 'PG_VERSION'), raw);
        expect(readDatadirMajor(nodeFs, nodePath, dir)).toBe(major);
      }
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});

describe('exportDatadir engine selection (HS2-D0V2NB)', () => {
  let seed;
  const scratch = [];

  beforeAll(async () => {
    seed = mkdtempSync(join(tmpdir(), 'hs2-engine-seed-'));
    const db = new PGlite(join(seed, 'db'));
    await db.exec(`create table tickets (id serial primary key, ticket_number text, title text not null);
      insert into tickets (ticket_number, title) values ('HS-1', 'synthetic');`);
    await db.close();
  });
  afterAll(() => {
    for (const dir of [seed, ...scratch]) rmSync(dir, { recursive: true, force: true });
  });
  beforeEach(() => {
    openDataDir.mockReset();
  });

  /** A copy of the seeded `.hotsheet` with `mutate` applied to its `db/`. */
  function variant(mutate) {
    const hs = mkdtempSync(join(tmpdir(), 'hs2-engine-case-'));
    scratch.push(hs);
    cpSync(seed, hs, { recursive: true });
    mutate(join(hs, 'db'));
    return hs;
  }

  it('opens a healthy bundled-major datadir without touching pglite-migrate', async () => {
    const out = await exportDatadir(
      variant(() => {}),
      null,
    );
    expect(out.tickets.map((t) => t.ticket_number)).toEqual(['HS-1']);
    expect(openDataDir).not.toHaveBeenCalled();
  });

  it.each([
    ['a garbage pg_control', (db) => writeFileSync(join(db, 'global', 'pg_control'), Buffer.alloc(8192, 7))],
    ['a missing pg_control (WASM abort)', (db) => rmSync(join(db, 'global', 'pg_control'))],
    ['a missing base directory', (db) => rmSync(join(db, 'base'), { recursive: true })],
    ['an unparseable PG_VERSION', (db) => writeFileSync(join(db, 'PG_VERSION'), 'xx\n')],
  ])(
    'fails a damaged datadir with %s and never fetches an engine',
    async (_name, mutate) => {
      const hs = variant(mutate);
      await expect(exportDatadir(hs, null)).rejects.toThrow(/looks damaged.*No engine was downloaded/s);
      expect(openDataDir).not.toHaveBeenCalled();
    },
    30000,
  );

  it('hands a datadir stamped with another major to pglite-migrate', async () => {
    const fetched = new PGlite();
    await fetched.exec(`create table tickets (id serial primary key, ticket_number text, title text not null);
      insert into tickets (ticket_number, title) values ('HS-7', 'from fetched engine');`);
    openDataDir.mockResolvedValueOnce(fetched);
    const hs = variant((db) => writeFileSync(join(db, 'PG_VERSION'), '18\n'));
    const out = await exportDatadir(hs, null);
    expect(out.tickets.map((t) => t.ticket_number)).toEqual(['HS-7']);
    expect(openDataDir).toHaveBeenCalledTimes(1);
    expect(openDataDir.mock.calls[0][2]).toMatchObject({
      fetchMissingEngine: true,
      pgliteOptions: { database: 'postgres' },
    });
  }, 30000);

  it('keeps every pglite-migrate cause when the fetched engine cannot open either database', async () => {
    openDataDir
      .mockRejectedValueOnce(new Error('postgres refused'))
      .mockRejectedValueOnce(new Error('template1 refused'));
    const hs = variant((db) => writeFileSync(join(db, 'PG_VERSION'), '18\n'));
    await expect(exportDatadir(hs, null)).rejects.toThrow(/postgres: postgres refused; template1: template1 refused/);
    expect(openDataDir).toHaveBeenCalledTimes(2);
  }, 30000);
});
