import { mkdir, mkdtemp, readFile, realpath, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { describe, expect, it, vi } from 'vitest';

import { createDevApp } from './dev-server';
import {
  type BackupGit,
  backupGit,
  hasCompletedHs1Import,
  hasHs1Backup,
  readImportCompletion,
  recordHs1Backup,
  requireHs1Backup,
} from './hs1-backup';
import {
  connectGitTicketStoreRemote,
  developmentSetupAssetsFingerprint,
  migrateHs1Project,
  projectSessionRegistry,
  runGitCommand,
} from './project-bridge';

const revision = 'a'.repeat(40),
  olderRevision = 'b'.repeat(40);

async function unitFixture() {
  const store = await realpath(await mkdtemp(resolve(tmpdir(), 'hs1-backup-unit-'))),
    root = '/work/legacy';
  await mkdir(resolve(store, '.git'));
  await writeFile(resolve(store, 'hotsheet-hs1-import.json'), JSON.stringify({ sourceProject: root }));
  await writeFile(
    resolve(store, '.git/hotsheet-hs1-import-completed.json'),
    JSON.stringify({ version: 1, sourceProject: root, revision }),
  );
  const git: BackupGit = vi.fn(async (_store: string, args: string[]) => {
    if (args[0] === 'rev-parse') return `.git/${args[2]}`;
    if (args[0] === 'remote') return '/backup.git';
    if (args[0] === 'symbolic-ref') return 'refs/heads/main';
    if (args[0] === 'ls-remote') return `${revision}\trefs/heads/main`;
    return '';
  });
  return { store, root, git };
}

describe('HS1 backup proof transitions', () => {
  it('requires completed import and successful remote proof rather than origin existence', async () => {
    const { store, root, git } = await unitFixture();
    try {
      await expect(requireHs1Backup(store, root, git)).rejects.toThrow(/verified backup/);
      const completion = await readImportCompletion(store, git);
      expect(completion).toBeDefined();
      await recordHs1Backup(store, completion!, git);
      const offlineGit: BackupGit = async (path, args) => {
        if (args[0] === 'ls-remote') throw new Error('Startup must not contact origin');
        return git(path, args);
      };
      expect(await hasHs1Backup(store, root, offlineGit)).toBe(true);
      await expect(requireHs1Backup(store, root, git)).resolves.toBeUndefined();
      expect(git).toHaveBeenCalledWith(store, ['merge-base', '--is-ancestor', revision, revision]);
      await expect(requireHs1Backup(store, '/work/other', git)).rejects.toThrow(/verified backup/);
      await writeFile(
        resolve(store, '.git/hotsheet-hs1-import-completed.json'),
        JSON.stringify({ version: 1, sourceProject: root, revision: olderRevision }),
      );
      await expect(requireHs1Backup(store, root, git)).rejects.toThrow(/stale/);
    } finally {
      await rm(store, { recursive: true, force: true });
    }
  });

  it('rejects dirty files, pending copies, an old HEAD, and invalidation during remote verification', async () => {
    const { store, root, git } = await unitFixture();
    try {
      const completion = await readImportCompletion(store, git);
      await recordHs1Backup(store, completion!, git);
      await expect(
        readImportCompletion(store, async (path, args) =>
          args[0] === 'status' ? ' M tickets/partial.md' : git(path, args),
        ),
      ).rejects.toThrow(/Commit all/);
      await mkdir(resolve(store, 'hotsheet-hs1-import-pending'));
      await writeFile(resolve(store, 'hotsheet-hs1-import-pending/ticket.json'), '{}');
      await expect(requireHs1Backup(store, root, git)).rejects.toThrow(/pending/);
      await rm(resolve(store, 'hotsheet-hs1-import-pending'), { recursive: true });
      await expect(
        requireHs1Backup(store, root, async (path, args) => {
          if (args[0] === 'merge-base' && args.at(-1) === 'HEAD') throw new Error('Not an ancestor of old HEAD');
          return git(path, args);
        }),
      ).rejects.toThrow(/old HEAD/);
      for (const verify of [
        (probe: BackupGit) => recordHs1Backup(store, completion!, probe),
        (probe: BackupGit) => requireHs1Backup(store, root, probe),
      ]) {
        let remoteChecked = false;
        await expect(
          verify(async (path, args) => {
            if (args[0] === 'ls-remote') remoteChecked = true;
            if (remoteChecked && args[0] === 'remote') return '/replacement.git';
            return git(path, args);
          }),
        ).rejects.toThrow(/changed during.*verification/);
        remoteChecked = false;
        await expect(
          verify(async (path, args) => {
            if (args[0] === 'ls-remote') remoteChecked = true;
            if (remoteChecked && args[0] === 'status') return ' M tickets/changed-during-backup.md';
            return git(path, args);
          }),
        ).rejects.toThrow(/Commit all/);
      }
      await expect(
        requireHs1Backup(store, root, async (path, args) => {
          if (args[0] === 'ls-remote') await rm(resolve(store, '.git/hotsheet-hs1-backup.json'));
          return git(path, args);
        }),
      ).rejects.toThrow(/changed during verification/);
    } finally {
      await rm(store, { recursive: true, force: true });
    }
  });
});

async function realFixture() {
  const parent = await realpath(await mkdtemp(resolve(tmpdir(), 'hs1-backup-git-'))),
    root = resolve(parent, 'project'),
    store = resolve(parent, 'tickets.hs2'),
    remote = resolve(parent, 'backup.git');
  await mkdir(resolve(root, '.hotsheet/db'), { recursive: true });
  await writeFile(resolve(root, '.hotsheet/db/PG_VERSION'), '17');
  await mkdir(store);
  await backupGit(store, ['init', '-b', 'main']);
  await backupGit(store, ['config', 'user.name', 'Hot Sheet test']);
  await backupGit(store, ['config', 'user.email', 'hotsheet@example.invalid']);
  await backupGit(store, ['commit', '--allow-empty', '-m', 'Before import']);
  const beforeImport = await backupGit(store, ['rev-parse', 'HEAD']);
  await writeFile(resolve(store, 'hotsheet-store.json'), '{}');
  await writeFile(resolve(store, 'hotsheet-hs1-import.json'), JSON.stringify({ sourceProject: root }));
  await mkdir(resolve(store, 'attachments'));
  await writeFile(resolve(store, 'attachments/proof.png'), 'Imported payload');
  await backupGit(store, ['add', '-A']);
  await backupGit(store, ['commit', '-m', 'Imported tickets and payloads']);
  const revision = await backupGit(store, ['rev-parse', 'HEAD']);
  await writeFile(
    resolve(store, '.git/hotsheet-hs1-import-completed.json'),
    JSON.stringify({ version: 1, sourceProject: root, revision }),
  );
  await backupGit(parent, ['init', '--bare', remote]);
  return { parent, root, store, remote, beforeImport, revision };
}

describe('HS1 backup and cleanup through real Git and the local HTTP endpoint', () => {
  it('binds a completed custom-destination import to only its owning open session', async () => {
    const { parent, root, store } = await realFixture(),
      sessions = projectSessionRegistry(),
      fingerprint = await developmentSetupAssetsFingerprint();
    sessions.set('new-import-owner', { root, url: 'http://127.0.0.1:1', secret: 'test' });
    sessions.set('other-import-owner', {
      root: '/work/other',
      url: 'http://127.0.0.1:1',
      secret: 'test',
      ticketStore: '/tickets/other',
    });
    try {
      const runner = async (_command: string, args: string[]) =>
        args.includes('compatibility')
          ? JSON.stringify({
              generation: 'hs2',
              setup_assets_fingerprint: fingerprint,
              store_schema: { min: 1, max: 3, creates: 3 },
            })
          : 'Imported 1 ticket(s) (1 attachment file(s)), skipped 0 already present.';
      await expect(migrateHs1Project(root, store, runner)).resolves.toMatchObject({ ticketStore: store });
      expect(sessions.get('new-import-owner')?.ticketStore).toBe(store);
      expect(sessions.get('other-import-owner')?.ticketStore).toBe('/tickets/other');
    } finally {
      sessions.delete('new-import-owner');
      sessions.delete('other-import-owner');
      await rm(parent, { recursive: true, force: true });
    }
  });

  it('blocks reopening and DELETE during push, rechecks stale remote state, then permits a verified retry', async () => {
    const fixture = await realFixture(),
      { parent, root, store, remote, beforeImport } = fixture;
    let releasePush = () => {},
      enteredPush = () => {};
    const paused = new Promise<void>((resolve) => {
        releasePush = resolve;
      }),
      entered = new Promise<void>((resolve) => {
        enteredPush = resolve;
      });
    projectSessionRegistry().set('backup-gate', {
      root,
      ticketStore: store,
      url: 'http://127.0.0.1:1',
      secret: 'test',
    });
    try {
      const app = createDevApp(),
        deleting = () => app.request('/__hotsheet/projects/backup-gate/hs1-data', { method: 'DELETE' }),
        connecting = connectGitTicketStoreRemote(store, remote, async (command, args) => {
          if (args.includes('push')) {
            enteredPush();
            await paused;
          }
          await runGitCommand(command, args);
        });
      await entered;
      expect(await hasCompletedHs1Import(store, root)).toBe(true);
      expect(await hasHs1Backup(store, root)).toBe(false);
      expect((await deleting()).status).toBe(400);
      expect(await readFile(resolve(root, '.hotsheet/db/PG_VERSION'), 'utf8')).toBe('17');
      releasePush();
      await connecting;
      expect(await hasHs1Backup(store, root)).toBe(true);
      await writeFile(resolve(store, 'uncommitted.txt'), 'Pending changes');
      expect((await deleting()).status).toBe(400);
      await rm(resolve(store, 'uncommitted.txt'));
      await backupGit(remote, ['update-ref', 'refs/heads/main', beforeImport]);
      // Reopen uses local proof; the destructive endpoint always rechecks origin.
      expect(await hasHs1Backup(store, root)).toBe(true);
      expect((await deleting()).status).toBe(400);
      await connectGitTicketStoreRemote(store, remote);
      expect(await hasHs1Backup(store, root)).toBe(true);
      const response = await deleting();
      expect(response.status).toBe(200);
      expect(await response.json()).toEqual({ removed: ['db'] });
      await expect(readFile(resolve(root, '.hotsheet/db/PG_VERSION'))).rejects.toMatchObject({ code: 'ENOENT' });
    } finally {
      releasePush();
      projectSessionRegistry().delete('backup-gate');
      await rm(parent, { recursive: true, force: true });
    }
  }, 20_000);

  it('rolls back only its own origin, preserves existing origins on failure, and rejects legacy receipt-only evidence', async () => {
    const { parent, root, store, remote } = await realFixture();
    const failPush = async (command: string, args: string[]) => {
      if (args.includes('push')) throw new Error('Simulated push failure');
      await runGitCommand(command, args);
    };
    try {
      await expect(connectGitTicketStoreRemote(store, remote, failPush)).rejects.toThrow(/Simulated push failure/);
      await expect(backupGit(store, ['remote', 'get-url', 'origin'])).rejects.toThrow();
      expect(await hasHs1Backup(store, root)).toBe(false);
      await backupGit(store, ['remote', 'add', 'origin', remote]);
      await expect(connectGitTicketStoreRemote(store, remote, failPush)).rejects.toThrow(/Simulated push failure/);
      expect(await backupGit(store, ['remote', 'get-url', 'origin'])).toBe(remote);
      await connectGitTicketStoreRemote(store, remote);
      expect(await hasHs1Backup(store, root)).toBe(true);
      // The headless verifier writes the same proof that the HTTP cleanup gate reads.
      await rm(resolve(store, '.git/hotsheet-hs1-backup.json'));
      await runGitCommand(
        process.env.HOTSHEET_MIGRATE_BIN || resolve(process.cwd(), '../../target/debug/hotsheet-migrate'),
        ['-C', store, '--verify-backup'],
      );
      await expect(requireHs1Backup(store, root)).resolves.toBeUndefined();
      await backupGit(store, ['remote', 'remove', 'origin']);
      await expect(
        connectGitTicketStoreRemote(store, remote, async (command, args) => {
          if (args.includes('push')) {
            await backupGit(store, ['remote', 'set-url', 'origin', `${remote}.changed`]);
            throw new Error('Origin changed during failed push');
          }
          await runGitCommand(command, args);
        }),
      ).rejects.toThrow(/Origin changed/);
      expect(await backupGit(store, ['remote', 'get-url', 'origin'])).toBe(`${remote}.changed`);
      expect(await hasHs1Backup(store, root)).toBe(false);
      await backupGit(store, ['remote', 'set-url', 'origin', remote]);
      await connectGitTicketStoreRemote(store, remote);
      await rm(resolve(store, '.git/hotsheet-hs1-import-completed.json'));
      expect(await hasCompletedHs1Import(store, root)).toBe(false);
      expect(await hasHs1Backup(store, root)).toBe(false);
      await expect(connectGitTicketStoreRemote(store, remote)).rejects.toThrow(/Retry the Hot Sheet 1 import/);
    } finally {
      await rm(parent, { recursive: true, force: true });
    }
  }, 20_000);
});
