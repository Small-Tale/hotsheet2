// HS2-Q77A53: the introspection helper uses the exporter's database probe (so an old
// 0.3.x cluster with its tables in `template1` is listed) and never leaks its temp copy.
// All clusters are synthetic, written by the bundled and the 0.3.x dev engines.
import { describe, it, expect, beforeAll, afterAll } from 'vitest';
import { PGlite } from '@electric-sql/pglite';
import { PGlite as PGliteOld } from 'pglite-old';
import { spawnSync } from 'node:child_process';
import { mkdirSync, mkdtempSync, readdirSync, rmSync, writeFileSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

import { INTROSPECT_USAGE, introspectDatadir } from '../src/introspect.mjs';

const SCRIPT = join(dirname(fileURLToPath(import.meta.url)), '..', 'src', 'introspect.mjs');
const DDL = `create table tickets (id serial primary key, ticket_number text, title text not null);
  insert into tickets (ticket_number, title) values ('SYN-1', 'synthetic'), ('SYN-2', 'synthetic');`;

describe('introspectDatadir (HS2-Q77A53)', () => {
  let root, oldDb, currentDb;

  beforeAll(async () => {
    root = mkdtempSync(join(tmpdir(), 'hs2-introspect-'));
    oldDb = join(root, 'old', 'db');
    currentDb = join(root, 'current', 'db');
    for (const [Engine, dir] of [
      [PGliteOld, oldDb],
      [PGlite, currentDb],
    ]) {
      mkdirSync(dirname(dir), { recursive: true });
      const db = new Engine(dir);
      await db.exec(DDL);
      await db.close();
    }
  }, 60000);
  afterAll(() => rmSync(root, { recursive: true, force: true }));

  /** A fresh, empty temp root so leaked `hs1-introspect-*` directories are visible. */
  const freshTmp = () => mkdtempSync(join(root, 'tmp-'));

  it.each([
    ['an old 0.3.x cluster from template1', () => oldDb, 'template1'],
    ['a current 0.4.x cluster from postgres', () => currentDb, 'postgres'],
  ])(
    'lists %s and removes its copy',
    async (_name, source, database) => {
      const tmpRoot = freshTmp();
      const lines = [];
      const result = await introspectDatadir(source(), { log: (line) => lines.push(line), tmpRoot });
      expect(result.database).toBe(database);
      expect(result.tables).toEqual([{ name: 'tickets', rows: 2, columns: ['id', 'ticket_number', 'title'] }]);
      expect(lines.join('\n')).toContain('== tickets (2 rows) ==');
      expect(readdirSync(tmpRoot)).toEqual([]);
    },
    30000,
  );

  it('removes its temp directory when the copy fails before opening', async () => {
    const tmpRoot = freshTmp();
    await expect(introspectDatadir(join(root, 'missing', 'db'), { log: () => {}, tmpRoot })).rejects.toThrow(/ENOENT/);
    expect(readdirSync(tmpRoot)).toEqual([]);
  });

  it('removes its temp directory when a damaged cluster cannot be opened', async () => {
    const damaged = join(root, 'damaged', 'db');
    mkdirSync(dirname(damaged), { recursive: true });
    const db = new PGlite(damaged);
    await db.exec(DDL);
    await db.close();
    writeFileSync(join(damaged, 'global', 'pg_control'), Buffer.alloc(8192, 7));
    const tmpRoot = freshTmp();
    await expect(introspectDatadir(damaged, { log: () => {}, tmpRoot })).rejects.toThrow(/looks damaged/);
    expect(readdirSync(tmpRoot)).toEqual([]);
  }, 30000);

  it('exits 2 with usage for bad arguments and 1 for a failure, and only runs as the entry script', () => {
    const run = (args) => spawnSync(process.execPath, [SCRIPT, ...args], { encoding: 'utf8', timeout: 60000 });
    for (const args of [[], ['a', 'b'], ['--help']]) {
      const result = run(args);
      expect(result.status).toBe(2);
      expect(result.stderr.trim()).toBe(INTROSPECT_USAGE);
    }
    const missing = run([join(root, 'missing', 'db')]);
    expect(missing.status).toBe(1);
    expect(missing.stderr).toMatch(/^hotsheet-introspect: \S/);
  }, 60000);
});
