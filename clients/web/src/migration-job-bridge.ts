import { readFile, realpath, stat } from 'node:fs/promises';
import { homedir } from 'node:os';
import { basename, dirname, resolve } from 'node:path';

import { MigrationJobs } from './migration-jobs';
import type { MigrationJob, MigrationStart } from './migration-progress';
import { projectSessionRegistry, runHs1MigrationJob } from './project-bridge';

type MigrationHost = typeof process & { __hotsheetMigrationJobs?: MigrationJobs };
export function migrationJobRegistry(host: MigrationHost = process): MigrationJobs {
  return (host.__hotsheetMigrationJobs ??= new MigrationJobs(
    process.env.HOTSHEET_MIGRATION_JOBS ??
      resolve(process.env.HOTSHEET_HOME ?? resolve(homedir(), '.hotsheet2'), 'migration-jobs'),
    runHs1MigrationJob,
  ));
}
export interface MigrationJobApi {
  start(input: MigrationStart): Promise<MigrationJob>;
  watch(root: string, after?: number, signal?: AbortSignal): Promise<MigrationJob | undefined>;
}
function validateMigrationStart(value: unknown): asserts value is MigrationStart {
  const input = value as Partial<MigrationStart> | null;
  if (
    !input ||
    !['import', 'backup'].includes(input.kind ?? '') ||
    typeof input.root !== 'string' ||
    typeof input.location !== 'string' ||
    typeof input.projectId !== 'string'
  )
    throw new Error('Choose a project and ticket repository for this operation.');
}
async function detectedMigrationSource(root: string): Promise<string> {
  const marker = resolve(root, '.hotsheet/db/PG_VERSION'),
    info = await stat(marker);
  return `${marker}\0${(await readFile(marker, 'utf8')).trim()}\0${info.dev}:${info.ino}`;
}
export const migrationJobApi: MigrationJobApi = {
  async start(input) {
    validateMigrationStart(input);
    const root = await realpath(input.root),
      target = projectSessionRegistry().get(input.projectId);
    if (!target || target.root !== root) throw new Error('The owning project session is not open.');
    const location = resolve(input.location),
      store = await realpath(location).catch(async (error: unknown) => {
        if ((error as NodeJS.ErrnoException).code !== 'ENOENT') throw error;
        return resolve(await realpath(dirname(location)), basename(location));
      });
    const sourceIdentity = await detectedMigrationSource(root);
    if (input.kind === 'backup' && (target.ticketStore !== store || !input.remote?.trim()))
      throw new Error('Choose the imported repository and its backup remote.');
    return migrationJobRegistry().start(
      {
        projectId: input.projectId,
        root,
        store,
        sourceIdentity,
        kind: input.kind,
        ...(input.remote ? { remote: input.remote.trim() } : {}),
      },
      input.retryAttempt,
    );
  },
  async watch(root, after, signal) {
    const canonical = await realpath(root);
    const job =
      after === undefined
        ? await migrationJobRegistry().snapshot(canonical)
        : await migrationJobRegistry().watch(canonical, after, signal);
    if (!job) return undefined;
    const currentSource = await detectedMigrationSource(canonical).catch((error: unknown) => {
      if ((error as NodeJS.ErrnoException).code === 'ENOENT') return undefined;
      throw error;
    });
    return currentSource && currentSource !== job.sourceIdentity ? undefined : job;
  },
};
