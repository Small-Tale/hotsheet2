import { createHash } from 'node:crypto';
import { spawnSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as path from 'node:path';
import { tmpdir } from 'node:os';
import { fileURLToPath, URL } from 'node:url';

import { PGlite } from '@electric-sql/pglite';
import { describe, expect, it } from 'vitest';

import { copyDatabase, exportFromDb, stageAttachments } from '../src/export.mjs';

function digest(directory) {
  const hash = createHash('sha256');
  const visit = (root) => {
    for (const name of fs.readdirSync(root).sort()) {
      const file = path.join(root, name);
      if (fs.statSync(file).isDirectory()) visit(file);
      else {
        hash.update(path.relative(directory, file));
        hash.update(fs.readFileSync(file));
      }
    }
  };
  visit(directory);
  return hash.digest('hex');
}

describe('measured export progress and complete attachment evidence', () => {
  it('counts copied bytes at file boundaries, retains symlinks, and rejects missing attachments before mutation', () => {
    const root = fs.mkdtempSync(path.join(tmpdir(), 'hs-progress-'));
    try {
      const source = path.join(root, 'source'),
        destination = path.join(root, 'copy');
      fs.mkdirSync(path.join(source, 'nested'), { recursive: true });
      fs.writeFileSync(path.join(source, 'one'), '123');
      fs.writeFileSync(path.join(source, 'nested/two'), '45678');
      fs.symlinkSync(path.join(source, 'one'), path.join(source, 'link'));
      const events = [];
      copyDatabase(fs, path, source, destination, (event) => events.push(event));
      expect(
        events.filter((event) => event.phase === 'copy_database').map((event) => [event.completed, event.total]),
      ).toEqual([
        [0, 8],
        [5, 8],
        [8, 8],
      ]);
      expect(fs.lstatSync(path.join(destination, 'link')).isSymbolicLink()).toBe(true);
      expect(fs.readFileSync(path.join(source, 'one'), 'utf8')).toBe('123');
      const exported = {
        tickets: [
          {
            attachments: [
              { original_filename: 'one', stored_path: path.join(source, 'one') },
              { original_filename: 'absent', stored_path: '/missing/payload' },
            ],
          },
        ],
      };
      const original = JSON.parse(JSON.stringify(exported));
      expect(() => stageAttachments(fs, path, source, path.join(root, 'out.json'), exported)).toThrow(
        /Referenced HS1 attachment is missing/,
      );
      expect(exported).toEqual(original);
      expect(fs.existsSync(path.join(root, 'attachments'))).toBe(false);
      exported.tickets[0].attachments.pop();
      stageAttachments(fs, path, source, path.join(root, 'out.json'), exported, (event) => events.push(event));
      expect(events.at(-1)).toMatchObject({ phase: 'stage_attachments', completed: 3, total: 3, unit: 'bytes' });
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  });

  it('reports actual empty and processed ticket totals', async () => {
    const db = new PGlite(),
      events = [];
    try {
      await db.exec('create table tickets (id serial primary key, ticket_number text, title text)');
      await exportFromDb(db, {}, {}, (event) => events.push(event));
      expect(events.at(-1)).toMatchObject({ phase: 'export_tickets', completed: 0, total: 0, unit: 'tickets' });
      await db.exec("insert into tickets (ticket_number,title) values ('HS-1','One'),('HS-2','Two')");
      events.length = 0;
      await exportFromDb(db, {}, {}, (event) => events.push(event));
      expect(events.filter((event) => event.phase === 'export_tickets').map((event) => event.completed)).toEqual([
        0, 1, 2,
      ]);
      expect(events.at(-1).total).toBe(2);
    } finally {
      await db.close();
    }
  });

  it('real standalone migration fails on a missing referenced payload without recording completion or changing HS1 bytes', async () => {
    const root = fs.mkdtempSync(path.join(tmpdir(), 'hs-missing-payload-'));
    const source = path.join(root, 'project/.hotsheet'),
      store = path.join(root, 'tickets.hs2');
    fs.mkdirSync(source, { recursive: true });
    try {
      const db = new PGlite(path.join(source, 'db'));
      await db.exec(`create table tickets (id serial primary key, ticket_number text, title text);
        create table attachments (id serial primary key, ticket_id int, original_filename text, stored_path text);
        insert into tickets (ticket_number,title) values ('HS-1','Missing evidence');
        insert into attachments (ticket_id,original_filename,stored_path) values (1,'evidence.png','/missing/evidence.png');`);
      await db.close();
      const before = digest(source),
        exporter = fileURLToPath(new URL('../src/export.mjs', import.meta.url));
      const binary =
        process.env.HOTSHEET_MIGRATE_BIN ??
        fileURLToPath(new URL('../../target/debug/hotsheet-migrate', import.meta.url));
      const failed = spawnSync(binary, [source, '-C', store, '--migrator', exporter, '--progress-json'], {
        encoding: 'utf8',
      });
      expect(failed.status, failed.error?.message ?? failed.stderr).not.toBe(0);
      expect(failed.stderr).toContain('Referenced HS1 attachment is missing');
      expect(fs.existsSync(path.join(store, '.git/hotsheet-hs1-import-completed.json'))).toBe(false);
      expect(fs.existsSync(path.join(store, '.git/hotsheet-hs1-backup.json'))).toBe(false);
      expect(digest(source)).toBe(before);
      const events = failed.stdout
        .trim()
        .split('\n')
        .map((line) => JSON.parse(line));
      expect(events.some((event) => event.phase === 'copy_database' && event.total > 0)).toBe(true);
      expect(events.some((event) => event.result)).toBe(false);
    } finally {
      fs.rmSync(root, { recursive: true, force: true });
    }
  }, 60_000);
});
