import { createHash, randomUUID } from 'node:crypto';
import { watch as watchFiles } from 'node:fs';
import { mkdir, readdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { resolve } from 'node:path';

import { acquireMigrationLock, type MigrationLock } from './migration-lock';
import type { MigrationJob, MigrationProgress, MigrationResult } from './migration-progress';

export type MigrationOwner = Pick<MigrationJob, 'projectId' | 'root' | 'sourceIdentity' | 'store' | 'kind' | 'remote'>;
export type MigrationJobRunner = (
  job: MigrationJob,
  progress: (event: MigrationProgress) => void | Promise<void>,
) => Promise<MigrationResult | undefined>;

/** One durable owner per canonical project/store. Request cancellation only removes a watcher. */
export class MigrationJobs {
  private jobs = new Map<string, MigrationJob>();
  private listeners = new Map<string, Set<() => void>>();
  private writes = Promise.resolve();
  private starts = Promise.resolve();
  private active = new Set<string>();
  private loaded = false;
  private loading?: Promise<void>;
  constructor(
    private directory: string,
    private runner: MigrationJobRunner,
    private alive = (pid: number) => {
      try {
        process.kill(pid, 0);
        return true;
      } catch {
        return false;
      }
    },
    private lock: MigrationLock = acquireMigrationLock,
    private beforeSave: (job: MigrationJob) => Promise<void> = async () => {},
  ) {}

  private async load() {
    if (this.loaded) return;
    await (this.loading ??= this.loadFiles());
  }
  private async loadFiles() {
    await mkdir(this.directory, { recursive: true });
    for (const name of await readdir(this.directory)) {
      if (!/^[a-f0-9]{64}\.json$/.test(name)) continue;
      const job = JSON.parse(await readFile(resolve(this.directory, name), 'utf8')) as MigrationJob;
      if (!job.id || !job.attempt || !job.root || !Number.isSafeInteger(job.revision))
        throw new Error('Invalid migration job checkpoint. Keep the checkpoint for recovery.');
      if (job.status === 'running' && !this.alive(job.ownerPid)) {
        job.status = 'interrupted';
        job.error =
          'The migration owner changed before completion. Retry after its previous processes have ended to resume verified import work.';
        job.revision++;
        job.updatedAt = new Date().toISOString();
      }
      const known = this.jobs.get(job.id);
      if (!this.active.has(job.id) && (!known || job.revision >= known.revision)) this.jobs.set(job.id, job);
    }
    this.loaded = true;
  }
  private copy(job: MigrationJob): MigrationJob {
    return structuredClone(job);
  }
  private save(job: MigrationJob): Promise<void> {
    const snapshot = this.copy(job);
    const writing = this.writes
      .catch(() => {})
      .then(async () => {
        await this.beforeSave(snapshot);
        const path = resolve(this.directory, `${snapshot.id}.json`),
          temporary = `${path}.${randomUUID()}.tmp`;
        try {
          await writeFile(temporary, `${JSON.stringify(snapshot)}\n`, { flag: 'wx' });
          await rename(temporary, path);
        } finally {
          await rm(temporary, { force: true });
        }
      });
    this.writes = writing;
    return writing;
  }
  private changed(job: MigrationJob) {
    job.revision++;
    job.updatedAt = new Date().toISOString();
    for (const listener of this.listeners.get(job.id) ?? []) listener();
  }
  async snapshot(root: string): Promise<MigrationJob | undefined> {
    await this.load();
    const id = createHash('sha256').update(root).digest('hex');
    if (!this.active.has(id)) await this.loadFiles();
    const job = [...this.jobs.values()].find((value) => value.root === root);
    return job ? this.copy(job) : undefined;
  }
  async start(owner: MigrationOwner, retryAttempt?: string): Promise<MigrationJob> {
    let snapshot: MigrationJob | undefined;
    const starting = this.starts
      .catch(() => {})
      .then(async () => {
        await this.load();
        const id = createHash('sha256').update(owner.root).digest('hex');
        const matches = (value: MigrationJob) =>
          value.kind === owner.kind &&
          value.store === owner.store &&
          value.remote === owner.remote &&
          value.sourceIdentity === owner.sourceIdentity;
        const current = this.jobs.get(id);
        if (current?.status === 'running' && this.active.has(id)) {
          if (!matches(current)) throw new Error('This project already has migration or backup work in progress.');
          snapshot = this.copy(current);
          return;
        }
        const storeId = createHash('sha256').update(owner.store).digest('hex');
        const release = await this.lock([
          resolve(this.directory, 'locks', `project-${id}`),
          resolve(this.directory, 'locks', `store-${storeId}`),
        ]);
        let transferred = false;
        try {
          // Refresh under the native locks: another bridge may have completed work
          // since this instance first read its registry.
          await this.loadFiles();
          const previous = this.jobs.get(id),
            same = previous && matches(previous);
          if (
            previous?.status === 'interrupted' &&
            ((previous.ownerPid !== process.pid && this.alive(previous.ownerPid)) ||
              (previous.childPid && this.alive(previous.childPid)))
          )
            throw new Error(
              'The previous migration process is still running. Reconnect after it ends before retrying.',
            );
          if (previous?.status === 'running') {
            if (!same) throw new Error('This project already has migration or backup work in progress.');
            snapshot = this.copy(previous);
            return;
          }
          if (
            previous?.kind === 'backup' &&
            owner.kind === 'import' &&
            previous.store === owner.store &&
            previous.sourceIdentity === owner.sourceIdentity &&
            !retryAttempt
          ) {
            snapshot = this.copy(previous);
            return;
          }
          if (retryAttempt && !same)
            throw new Error('Retry inputs changed. Reopen the owning project before starting a different operation.');
          if (same && !retryAttempt) {
            snapshot = this.copy(previous);
            return;
          }
          if (retryAttempt && previous?.attempt !== retryAttempt)
            throw new Error('This retry belongs to an older migration attempt. Reopen its current status.');
          if (
            [...this.jobs.values()].some(
              (job) =>
                (job.status === 'running' ||
                  (job.status === 'interrupted' && job.ownerPid !== process.pid && this.alive(job.ownerPid)) ||
                  (job.childPid && this.alive(job.childPid))) &&
                job.store === owner.store,
            )
          )
            throw new Error('Another project is already importing or backing up this ticket repository.');
          const job: MigrationJob = {
            ...owner,
            id,
            attempt: randomUUID(),
            revision: (previous?.revision ?? 0) + 1,
            status: 'running',
            progress: { version: 1, phase: owner.kind === 'import' ? 'starting' : 'push_start' },
            warnings: [],
            ownerPid: process.pid,
            updatedAt: new Date().toISOString(),
          };
          await this.save(job);
          this.jobs.set(id, job);
          this.active.add(id);
          snapshot = this.copy(job);
          transferred = true;
          void this.execute(job, release);
        } finally {
          if (!transferred) await release();
        }
      });
    this.starts = starting;
    await starting;
    return snapshot!;
  }
  private async execute(job: MigrationJob, release: () => Promise<void>) {
    let lastProgress = 0;
    const terminal = this.copy(job);
    try {
      terminal.result = await this.runner(this.copy(job), (event) => {
        if (event.childPid !== undefined) {
          job.childPid = event.childPid || undefined;
          job.revision++;
          return this.save(job).then(() => {
            for (const listener of this.listeners.get(job.id) ?? []) listener();
          });
        }
        if (event.warning && !job.warnings.includes(event.warning))
          job.warnings = [...job.warnings.slice(-19), event.warning];
        if (
          !event.warning &&
          event.phase === job.progress.phase &&
          event.completed !== event.total &&
          Date.now() - lastProgress < 100
        )
          return;
        lastProgress = Date.now();
        job.progress = event;
        this.changed(job);
        void this.save(job).catch(() => {});
      });
      await this.writes;
      terminal.status = 'succeeded';
      terminal.progress = { version: 1, phase: 'complete' };
    } catch (error) {
      terminal.status = 'failed';
      terminal.progress = job.progress;
      terminal.error = error instanceof Error ? error.message : String(error);
    }
    terminal.childPid = job.childPid;
    terminal.warnings = job.warnings;
    terminal.revision = job.revision + 1;
    terminal.updatedAt = new Date().toISOString();
    try {
      await this.save(terminal);
    } catch (error) {
      terminal.status = 'failed';
      terminal.error = `Could not persist migration result: ${error instanceof Error ? error.message : String(error)}`;
      terminal.revision++;
      await this.save(terminal).catch(() => {});
    }
    await release();
    this.active.delete(job.id);
    this.jobs.set(job.id, terminal);
    for (const listener of this.listeners.get(job.id) ?? []) listener();
  }
  /** Flush checkpoints before a graceful shutdown or a persistence assertion. */
  async flush(): Promise<void> {
    await this.writes;
  }
  async watch(root: string, after: number, signal?: AbortSignal, idleMs = 25_000): Promise<MigrationJob | undefined> {
    const initial = await this.snapshot(root);
    if (!initial || initial.revision > after || initial.status !== 'running') return initial;
    await new Promise<void>((resolveWatch, rejectWatch) => {
      const listeners = this.listeners.get(initial.id) ?? new Set<() => void>();
      this.listeners.set(initial.id, listeners);
      const filesystem = this.active.has(initial.id)
        ? undefined
        : watchFiles(this.directory, (_event, filename) => {
            if (filename === `${initial.id}.json`) finish();
          });
      const finish = () => {
        filesystem?.close();
        clearTimeout(timeout);
        signal?.removeEventListener('abort', finish);
        listeners.delete(finish);
        resolveWatch();
      };
      filesystem?.on('error', (error) => {
        filesystem.close();
        clearTimeout(timeout);
        listeners.delete(finish);
        signal?.removeEventListener('abort', finish);
        rejectWatch(error);
      });
      const timeout = setTimeout(finish, idleMs);
      listeners.add(finish);
      signal?.addEventListener('abort', finish, { once: true });
      if (signal?.aborted || this.jobs.get(initial.id)?.revision !== initial.revision) finish();
      // A foreign owner may have renamed its checkpoint between the initial read
      // and watcher installation. Re-read after subscribing to close that gap.
      if (filesystem)
        void this.snapshot(root)
          .then((latest) => {
            if (!latest || latest.revision !== initial.revision || latest.status !== 'running') finish();
          })
          .catch((error: unknown) => {
            finish();
            rejectWatch(error instanceof Error ? error : new Error(String(error)));
          });
    });
    return this.snapshot(root);
  }
}
