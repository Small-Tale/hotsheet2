import { execFile } from 'node:child_process';
import { mkdir, mkdtemp, readdir, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';
import { promisify } from 'node:util';

import { describe, expect, it } from 'vitest';

import { createDevApp } from './dev-server';
import { backupGit, hasHs1Backup } from './hs1-backup';
import type { MigrationJob, MigrationStart } from './migration-progress';
import { listServerCheckouts, openLocalProject, projectSessionRegistry } from './project-bridge';

/** Real exporter, Rust importer, project server, source registration, job routes, and bare Git remote. */
describe.skipIf(process.env.HOTSHEET_LIVE_SERVER !== '1')('background migration through the real local servers', () => {
  it('keeps ownership across disconnect/rejoin and registers the source before verified backup', async () => {
    const workspace = await realpath(
        await mkdtemp(resolve(process.platform === 'darwin' ? '/tmp' : tmpdir(), 'hs-migration-live-')),
      ),
      root = resolve(workspace, 'project'),
      store = resolve(workspace, 'custom.hs2'),
      home = resolve(workspace, 'home'),
      previousHome = process.env.HOTSHEET_HOME,
      previousJobs = process.env.HOTSHEET_MIGRATION_JOBS;
    process.env.HOTSHEET_HOME = home;
    process.env.HOTSHEET_MIGRATION_JOBS = resolve(workspace, 'jobs');
    await mkdir(root);
    try {
      await promisify(execFile)(
        'node',
        [
          '--input-type=module',
          '-e',
          `
        import { PGlite } from '@electric-sql/pglite';
        import { mkdirSync, writeFileSync } from 'node:fs';
        import { join } from 'node:path';
        const source = join(process.argv[1], '.hotsheet');
        mkdirSync(join(source, 'attachments'), { recursive: true });
        const db = new PGlite(join(source, 'db'));
        await db.exec("create table tickets (id serial primary key, ticket_number text, title text); create table attachments (id serial primary key,ticket_id int,original_filename text,stored_path text); insert into tickets(ticket_number,title) values ('HS-1','Background imported'); insert into attachments(ticket_id,original_filename,stored_path) values (1,'proof.txt','/old/machine/proof.txt');");
        await db.close();
        writeFileSync(join(source,'attachments/proof.txt'),'ACTUAL PAYLOAD');
      `,
          root,
        ],
        { cwd: resolve(process.cwd(), '../../migrator') },
      );
      const session = await openLocalProject(root),
        app = createDevApp();
      const start = async (input: MigrationStart) =>
        app.request('/__hotsheet/projects/migration-jobs', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify(input),
        });
      const input: MigrationStart = { projectId: session.id, root, location: store, kind: 'import' };
      const startedResponse = await start(input);
      expect(startedResponse.status, await startedResponse.clone().text()).toBe(202);
      let job = (await startedResponse.json()) as MigrationJob;
      expect(job.status).toBe('running');
      expect(((await (await start(input)).json()) as MigrationJob).attempt).toBe(job.attempt);
      expect((await start({ ...input, location: resolve(workspace, 'other.hs2') })).status).toBe(409);
      const disconnected = new AbortController();
      disconnected.abort();
      await app.request(`/__hotsheet/projects/migration-jobs?root=${encodeURIComponent(root)}&after=${job.revision}`, {
        signal: disconnected.signal,
      });
      const phases = new Set<string>();
      const waitForTerminal = async () => {
        while (job.status === 'running') {
          const response = await app.request(
            `/__hotsheet/projects/migration-jobs?root=${encodeURIComponent(root)}&after=${job.revision}`,
          );
          job = ((await response.json()) as { job: MigrationJob }).job;
          phases.add(job.progress.phase);
        }
      };
      await waitForTerminal();
      expect(job.status, job.error).toBe('succeeded');
      expect(phases.size).toBeGreaterThan(2);
      expect(job.result).toMatchObject({ ticketStore: store, tickets: 1, attachments: 1, stores: [store] });
      expect((await listServerCheckouts()).find((checkout) => checkout.id === session.id)?.stores).toContain(store);
      expect((await readFile(resolve(root, '.hotsheet2/store'), 'utf8')).trim()).toBe(store);
      // The browser refreshes completion, then restores from the root alone after reload.
      for (const body of [{ root, ticketStore: job.store }, { root }]) {
        const reopened = await app.request('/__hotsheet/projects/open', {
          method: 'POST',
          headers: { 'content-type': 'application/json' },
          body: JSON.stringify(body),
        });
        expect(reopened.status).toBe(201);
        expect(await reopened.json()).toMatchObject({
          stores: [store],
          hs1ImportCompleted: true,
          needsTicketSetup: false,
          needsHs1Migration: false,
        });
        expect(projectSessionRegistry().get(session.id)?.ticketStore).toBe(store);
        expect((await listServerCheckouts()).find((checkout) => checkout.id === session.id)?.stores).toEqual([store]);
      }
      expect(await hasHs1Backup(store, root)).toBe(false);
      expect((await app.request(`/__hotsheet/projects/${session.id}/hs1-data`, { method: 'DELETE' })).status).toBe(400);
      const remote = resolve(workspace, 'backup.git');
      await backupGit(workspace, ['init', '--bare', remote]);
      job = (await (await start({ ...input, kind: 'backup', remote })).json()) as MigrationJob;
      expect(job.status).toBe('running');
      await waitForTerminal();
      expect(job.status, job.error).toBe('succeeded');
      expect(await hasHs1Backup(store, root)).toBe(true);
      expect(await readFile(resolve(root, '.hotsheet/attachments/proof.txt'), 'utf8')).toBe('ACTUAL PAYLOAD');
      expect((await app.request(`/__hotsheet/projects/${session.id}/hs1-data`, { method: 'DELETE' })).status).toBe(200);
      // Replaying terminal history must not undo an intentional later relink.
      const replacement = resolve(workspace, 'replacement.hs2');
      await mkdir(replacement);
      await promisify(execFile)(process.env.HOTSHEET_CLI_BIN ?? 'hotsheet-cli', [
        '-C',
        replacement,
        'init',
        '--prefix',
        'ALT',
      ]);
      await writeFile(resolve(root, '.hotsheet2/store'), `${replacement}\n`);
      const history = await app.request(`/__hotsheet/projects/migration-jobs?root=${encodeURIComponent(root)}`);
      expect(((await history.json()) as { job: MigrationJob }).job.store).toBe(store);
      const refreshed = await app.request('/__hotsheet/projects/open', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ root }),
      });
      expect(await refreshed.json()).toMatchObject({
        stores: [replacement],
        needsTicketSetup: false,
        hs1CleanupEligible: false,
      });
      expect(projectSessionRegistry().get(session.id)?.ticketStore).toBe(replacement);
      projectSessionRegistry().delete(session.id);
    } finally {
      for (const name of await readdir(resolve(home, 'instances')).catch(() => [] as string[])) {
        if (!name.endsWith('.json')) continue;
        const instance = JSON.parse(await readFile(resolve(home, 'instances', name), 'utf8')) as { pid?: number };
        if (instance.pid) {
          try {
            process.kill(instance.pid, 'SIGTERM');
          } catch {
            /* Already stopped. */
          }
        }
      }
      // The fixture opens no terminals. Its detached broker intentionally outlives
      // server shutdown, so stop only brokers whose command owns this unique home.
      if (process.platform !== 'win32') {
        const { stdout } = await promisify(execFile)('ps', ['-eo', 'pid=,args=']);
        for (const line of stdout.split('\n')) {
          const match = line.trim().match(/^(\d+)\s+(.+)$/);
          if (match?.[2].includes(`--terminal-broker-process ${resolve(home, 'broker')}/`)) {
            try {
              process.kill(Number(match[1]), 'SIGTERM');
            } catch {
              /* Already stopped. */
            }
          }
        }
      }
      if (previousHome === undefined) delete process.env.HOTSHEET_HOME;
      else process.env.HOTSHEET_HOME = previousHome;
      if (previousJobs === undefined) delete process.env.HOTSHEET_MIGRATION_JOBS;
      else process.env.HOTSHEET_MIGRATION_JOBS = previousJobs;
      delete (process as typeof process & { __hotsheetMigrationJobs?: unknown }).__hotsheetMigrationJobs;
      await rm(workspace, { recursive: true, force: true });
    }
  }, 120_000);
});
