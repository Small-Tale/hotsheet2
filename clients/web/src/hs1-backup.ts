import { execFile } from 'node:child_process';
import { randomUUID } from 'node:crypto';
import { readdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';
import { promisify } from 'node:util';

const execute = promisify(execFile);
export type BackupGit = (store: string, args: string[]) => Promise<string>;
export const backupGit: BackupGit = async (store, args) => {
  const { stdout } = await execute('git', ['-C', store, ...args], {
    env: { ...process.env, GIT_TERMINAL_PROMPT: '0' },
    timeout: 30_000,
    maxBuffer: 16 * 1024 * 1024,
  });
  return stdout.trim();
};

const completionName = 'hotsheet-hs1-import-completed.json',
  backupName = 'hotsheet-hs1-backup.json';
export interface ImportCompletion {
  version: 1;
  sourceProject: string;
  revision: string;
}
interface BackupProof extends ImportCompletion {
  remote: string;
  ref: string;
  remoteRevision: string;
}
async function localPath(store: string, name: string, git: BackupGit): Promise<string> {
  return resolve(store, await git(store, ['rev-parse', '--git-path', name]));
}
async function readJson(path: string): Promise<unknown> {
  try {
    return JSON.parse(await readFile(path, 'utf8')) as unknown;
  } catch (error) {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return undefined;
    throw error;
  }
}
function validCompletion(value: unknown): value is ImportCompletion {
  if (!value || typeof value !== 'object') return false;
  const proof = value as Partial<ImportCompletion>;
  return (
    proof.version === 1 && typeof proof.sourceProject === 'string' && /^[a-f0-9]{40,64}$/.test(proof.revision ?? '')
  );
}

async function loadImportCompletion(store: string, git: BackupGit): Promise<ImportCompletion | undefined> {
  const receipt = await readJson(resolve(store, 'hotsheet-hs1-import.json'));
  if (!receipt) return undefined;
  const proof = await readJson(await localPath(store, completionName, git));
  if (!validCompletion(proof) || (receipt as { sourceProject?: unknown }).sourceProject !== proof.sourceProject)
    throw new Error('Retry the Hot Sheet 1 import to verify its committed ticket and attachment files before backup.');
  await git(store, ['merge-base', '--is-ancestor', proof.revision, 'HEAD']);
  return proof;
}

/** Keep completed-import presentation stable while later ticket edits are uncommitted. */
export async function hasCompletedHs1Import(store: string | undefined, root: string): Promise<boolean> {
  if (!store) return false;
  try {
    return (await loadImportCompletion(store, backupGit))?.sourceProject === root;
  } catch {
    return false;
  }
}

/** Validate the migrator's exact committed revision before any backup attempt. */
export async function readImportCompletion(
  store: string,
  git: BackupGit = backupGit,
): Promise<ImportCompletion | undefined> {
  const proof = await loadImportCompletion(store, git);
  if (!proof) return undefined;
  if (await git(store, ['status', '--porcelain=v1', '--untracked-files=all']))
    throw new Error('Commit all imported ticket-store changes before backup or Hot Sheet 1 cleanup.');
  const pending = await readdir(resolve(store, 'hotsheet-hs1-import-pending')).catch((error: unknown) => {
    if ((error as NodeJS.ErrnoException).code === 'ENOENT') return [];
    throw error;
  });
  if (pending.length) throw new Error('Finish the pending Hot Sheet 1 attachment import before backup or cleanup.');
  return proof;
}

/** Invalidate before starting a push so a reopened project cannot reuse stale proof. */
export async function invalidateHs1Backup(store: string, git: BackupGit = backupGit): Promise<void> {
  await rm(await localPath(store, backupName, git), { force: true });
}

async function remoteRevision(store: string, ref: string, git: BackupGit): Promise<string> {
  if (!ref.startsWith('refs/heads/') || /[\r\n]/.test(ref)) throw new Error('The backup branch is invalid.');
  const output = await git(store, ['ls-remote', '--exit-code', 'origin', ref]),
    line = output.split('\n').find((entry) => entry.split('\t')[1] === ref),
    revision = line?.split('\t')[0];
  if (!revision || !/^[a-f0-9]{40,64}$/.test(revision)) throw new Error('The backup branch is not present on origin.');
  return revision;
}

/** Persist backup proof only after push exits successfully and origin proves ancestry. */
export async function recordHs1Backup(
  store: string,
  expected: ImportCompletion,
  git: BackupGit = backupGit,
): Promise<void> {
  const current = await readImportCompletion(store, git);
  if (!current || current.revision !== expected.revision || current.sourceProject !== expected.sourceProject)
    throw new Error('The Hot Sheet 1 import changed during backup. Retry the backup.');
  const remote = await git(store, ['remote', 'get-url', 'origin']),
    ref = await git(store, ['symbolic-ref', 'HEAD']),
    revision = await remoteRevision(store, ref, git);
  await git(store, ['merge-base', '--is-ancestor', current.revision, revision]);
  const latest = await readImportCompletion(store, git);
  if (
    JSON.stringify(latest) !== JSON.stringify(current) ||
    (await git(store, ['remote', 'get-url', 'origin'])) !== remote
  )
    throw new Error('The Hot Sheet 1 import or origin changed during backup verification. Retry the backup.');
  const proof: BackupProof = { ...current, remote, ref, remoteRevision: revision },
    path = await localPath(store, backupName, git),
    temporary = `${path}.${randomUUID()}.tmp`;
  try {
    await writeFile(temporary, `${JSON.stringify(proof, null, 2)}\n`, { flag: 'wx' });
    await rename(temporary, path);
  } finally {
    await rm(temporary, { force: true });
  }
}

/** Recheck durable import/push evidence and current origin before deleting source data. */
export async function requireHs1Backup(
  store: string,
  root: string,
  git: BackupGit = backupGit,
  checkRemote = true,
): Promise<void> {
  const completion = await readImportCompletion(store, git),
    value = await readJson(await localPath(store, backupName, git));
  if (!completion || completion.sourceProject !== root || !validCompletion(value))
    throw new Error('Hot Sheet 1 cleanup requires a verified backup of this project’s completed import.');
  const proof = value as BackupProof;
  if (
    proof.sourceProject !== root ||
    proof.revision !== completion.revision ||
    typeof proof.remote !== 'string' ||
    typeof proof.ref !== 'string' ||
    !/^[a-f0-9]{40,64}$/.test(proof.remoteRevision) ||
    (await git(store, ['remote', 'get-url', 'origin'])) !== proof.remote
  )
    throw new Error('The Hot Sheet 1 backup proof is stale. Retry the backup before cleanup.');
  if (checkRemote) {
    const revision = await remoteRevision(store, proof.ref, git);
    await git(store, ['merge-base', '--is-ancestor', completion.revision, revision]);
  }
  const latest = await readImportCompletion(store, git),
    latestBackup = await readJson(await localPath(store, backupName, git));
  if (
    JSON.stringify(latest) !== JSON.stringify(completion) ||
    JSON.stringify(latestBackup) !== JSON.stringify(value) ||
    (await git(store, ['remote', 'get-url', 'origin'])) !== proof.remote
  )
    throw new Error('The Hot Sheet 1 import or backup changed during verification. Retry after it finishes.');
}

export async function hasHs1Backup(
  store: string | undefined,
  root: string,
  git: BackupGit = backupGit,
): Promise<boolean> {
  if (!store) return false;
  try {
    await requireHs1Backup(store, root, git, false);
    return true;
  } catch {
    return false;
  }
}
