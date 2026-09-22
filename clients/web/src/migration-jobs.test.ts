import { mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { describe, expect, it, vi } from 'vitest';

import { MigrationJobs, type MigrationOwner } from './migration-jobs';
import {
  acceptMigrationJob,
  migrationCounter,
  type MigrationJob,
  migrationPercent,
  type MigrationProgress,
} from './migration-progress';
import { lineDecoder, parseGitProgress, parseMigrationProgress } from './migration-stream';

function memoryLocks() {
  const held = new Set<string>();
  return {
    crash: () => {
      held.clear();
    },
    lock: async (paths: string[]) => {
      if (paths.some((path) => held.has(path))) throw new Error('Another project already owns this repository.');
      paths.forEach((path) => held.add(path));
      return async () => {
        paths.forEach((path) => held.delete(path));
      };
    },
  };
}
const owner: MigrationOwner = {
  projectId: 'project-a',
  root: '/work/a',
  sourceIdentity: 'db-a',
  store: '/work/a.hs2',
  kind: 'import',
};

describe('durable migration owner transitions', () => {
  it('deduplicates starts, rejects competing destinations, replays revisions, and survives subscriber disconnect', async () => {
    const directory = await mkdtemp(resolve(tmpdir(), 'migration-jobs-'));
    let progress: (event: MigrationProgress) => void = () => {},
      finish = () => {};
    const runner = vi.fn(async (_job, publish) => {
      progress = publish;
      await new Promise<void>((resolveDone) => {
        finish = resolveDone;
      });
      return undefined;
    });
    const locks = memoryLocks(),
      jobs = new MigrationJobs(directory, runner, undefined, locks.lock);
    try {
      const first = await jobs.start(owner),
        duplicate = await jobs.start(owner);
      expect(duplicate.attempt).toBe(first.attempt);
      expect(runner).toHaveBeenCalledTimes(1);
      await expect(jobs.start({ ...owner, store: '/other.hs2' })).rejects.toThrow(/already/);
      await expect(jobs.start({ ...owner, root: '/work/b', projectId: 'b' })).rejects.toThrow(/Another project/);
      const abort = new AbortController(),
        waiting = jobs.watch(owner.root, first.revision, abort.signal);
      abort.abort();
      expect((await waiting)?.status).toBe('running');
      const next = jobs.watch(owner.root, first.revision);
      progress({ version: 1, phase: 'copy_database', completed: 50, total: 100, unit: 'bytes' });
      const measured = await next;
      expect(measured?.revision).toBeGreaterThan(first.revision);
      expect(measured?.progress.completed).toBe(50);
      finish();
      let terminal = await jobs.watch(owner.root, measured!.revision);
      while (terminal?.status === 'running') terminal = await jobs.watch(owner.root, terminal.revision);
      expect(terminal?.status).toBe('succeeded');
      const durable = JSON.parse(await readFile(resolve(directory, `${first.id}.json`), 'utf8')) as MigrationJob;
      // Terminal status is durable before subscribers observe it.
      expect(durable.attempt).toBe(first.attempt);
      expect(durable.status).toBe('succeeded');
      const late = acceptMigrationJob(terminal, first);
      expect(late).toEqual(terminal);
      expect(migrationPercent(measured!.progress)).toBe(50);
      expect(migrationCounter({ version: 1, phase: 'empty', completed: 0, total: 0, unit: 'tickets' })).toBe(
        '0 of 0 tickets',
      );
      expect(migrationPercent({ version: 1, phase: 'unknown' })).toBeUndefined();
    } finally {
      finish();
      await new Promise((resolveDone) => setTimeout(resolveDone, 20));
      await rm(directory, { recursive: true, force: true });
    }
  });

  it('retains failures, rejects stale retries, and reports a stopped owner as interrupted after restart', async () => {
    const directory = await mkdtemp(resolve(tmpdir(), 'migration-restart-'));
    try {
      const locks = memoryLocks(),
        jobs = new MigrationJobs(
          directory,
          async () => {
            throw new Error('Disk full');
          },
          undefined,
          locks.lock,
        );
      const started = await jobs.start(owner);
      let failed = await jobs.watch(owner.root, started.revision);
      while (failed?.status === 'running') failed = await jobs.watch(owner.root, failed.revision);
      expect(failed?.error).toBe('Disk full');
      expect((await jobs.start(owner)).attempt).toBe(started.attempt);
      await expect(jobs.start(owner, 'old-attempt')).rejects.toThrow(/older/);
      const retried = await jobs.start(owner, started.attempt);
      expect(retried.attempt).not.toBe(started.attempt);
      expect(retried.revision).toBeGreaterThan(failed!.revision);
      await new Promise((resolveDone) => setTimeout(resolveDone, 20));
      const another = new MigrationJobs(directory, async () => new Promise(() => {}), undefined, locks.lock);
      const running = await another.start(owner, retried.attempt);
      locks.crash();
      const restarted = new MigrationJobs(
        directory,
        async () => undefined,
        () => false,
        locks.lock,
      );
      const interrupted = await restarted.snapshot(owner.root);
      expect(interrupted?.status).toBe('interrupted');
      expect(interrupted?.revision).toBeGreaterThan(running.revision);
      const recovered = await restarted.start(owner, running.attempt);
      expect(recovered.attempt).not.toBe(running.attempt);
    } finally {
      await new Promise((resolveDone) => setTimeout(resolveDone, 20));
      await rm(directory, { recursive: true, force: true });
    }
  });
});

describe('migration ownership adversaries', () => {
  it('arbitrates independent owners loaded before either start and replays foreign revisions', async () => {
    const directory = await mkdtemp(resolve(tmpdir(), 'migration-two-owners-'));
    const locks = memoryLocks();
    let finish = () => {};
    const runner = vi.fn(
      async () =>
        new Promise<undefined>((done) => {
          finish = () => {
            done(undefined);
          };
        }),
    );
    const a = new MigrationJobs(directory, runner, undefined, locks.lock);
    const b = new MigrationJobs(directory, runner, undefined, locks.lock);
    try {
      await Promise.all([a.snapshot(owner.root), b.snapshot(owner.root)]);
      const starts = await Promise.allSettled([a.start(owner), b.start(owner)]);
      expect(starts.filter((start) => start.status === 'fulfilled')).toHaveLength(1);
      expect(runner).toHaveBeenCalledTimes(1);
      const current = (await a.snapshot(owner.root))!;
      const changed = b.watch(owner.root, current.revision);
      finish();
      let terminal = await changed;
      while (terminal?.status === 'running') terminal = await b.watch(owner.root, terminal.revision);
      expect(terminal?.status).toBe('succeeded');
      expect((await b.start(owner)).attempt).toBe(current.attempt);
    } finally {
      finish();
      await a.flush();
      await b.flush();
      await rm(directory, { recursive: true, force: true });
    }
  });

  it('never publishes success before the terminal checkpoint is durable', async () => {
    const directory = await mkdtemp(resolve(tmpdir(), 'migration-persist-'));
    const locks = memoryLocks();
    const jobs = new MigrationJobs(
      directory,
      async () => undefined,
      undefined,
      locks.lock,
      async (job) => {
        if (job.status === 'succeeded') throw new Error('Disk full saving result');
      },
    );
    try {
      const started = await jobs.start(owner);
      let terminal = await jobs.watch(owner.root, started.revision);
      while (terminal?.status === 'running') terminal = await jobs.watch(owner.root, terminal.revision);
      expect(terminal?.status).toBe('failed');
      expect(terminal?.error).toContain('Could not persist migration result');
      const durable = JSON.parse(await readFile(resolve(directory, `${started.id}.json`), 'utf8')) as MigrationJob;
      expect(durable.status).toBe('failed');
    } finally {
      await jobs.flush();
      await rm(directory, { recursive: true, force: true });
    }
  });

  it('blocks another project sharing a store while an interrupted foreign owner is alive', async () => {
    const directory = await mkdtemp(resolve(tmpdir(), 'migration-foreign-orphan-'));
    const locks = memoryLocks();
    const initial = new MigrationJobs(directory, async () => undefined, undefined, locks.lock);
    try {
      const started = await initial.start(owner);
      await initial.flush();
      // Conservatively handle an interrupted checkpoint even if its owner PID was reused.
      await writeFile(
        resolve(directory, `${started.id}.json`),
        JSON.stringify({ ...started, status: 'interrupted', ownerPid: 987654 }),
      );
      const runner = vi.fn(async () => undefined);
      const restarted = new MigrationJobs(directory, runner, (pid) => pid === 987654, locks.lock);
      await expect(restarted.start({ ...owner, root: '/work/another', projectId: 'another' })).rejects.toThrow(
        /Another project/,
      );
      expect(runner).not.toHaveBeenCalled();
    } finally {
      await initial.flush();
      await rm(directory, { recursive: true, force: true });
    }
  });

  it('refuses retry while an interrupted owner still has a live child', async () => {
    const directory = await mkdtemp(resolve(tmpdir(), 'migration-orphan-'));
    const locks = memoryLocks();
    let checkpointed = () => {};
    const saved = new Promise<void>((done) => {
      checkpointed = done;
    });
    const original = new MigrationJobs(
      directory,
      async (_job, progress) => {
        await progress({ version: 1, phase: 'process_owner', childPid: 987654 });
        checkpointed();
        return new Promise(() => {});
      },
      undefined,
      locks.lock,
    );
    try {
      const started = await original.start(owner);
      await saved;
      locks.crash();
      const restarted = new MigrationJobs(
        directory,
        async () => undefined,
        (pid) => pid === 987654,
        locks.lock,
      );
      expect((await restarted.snapshot(owner.root))?.status).toBe('interrupted');
      await expect(restarted.start(owner, started.attempt)).rejects.toThrow(
        /previous migration process is still running/,
      );
    } finally {
      await original.flush();
      await rm(directory, { recursive: true, force: true });
    }
  });
});

describe('migration stream protocol', () => {
  it('decodes split UTF-8 and CRLF records without inventing unknown denominators', () => {
    const lines: string[] = [],
      decoder = lineDecoder((line) => lines.push(line)),
      bytes = Buffer.from('é\r\nnext\rthird');
    decoder.push(bytes.subarray(0, 1));
    decoder.push(bytes.subarray(1, 4));
    decoder.push(bytes.subarray(4));
    decoder.end();
    expect(lines).toEqual(['é', 'next', 'third']);
    expect(() => {
      lineDecoder(() => {}).push(Buffer.from(`${'x'.repeat(1024 * 1024 + 1)}\n`));
    }).toThrow(/exceeds 1 MB/);
    expect(parseGitProgress('Compressing objects: 50% (2/4)')).toMatchObject({
      phase: 'compress_objects',
      completed: 2,
      total: 4,
    });
    expect(parseGitProgress('Enumerating objects: 8, done.')).toEqual({ version: 1, phase: 'enumerate_objects' });
    expect(parseGitProgress('remote: accepted')).toBeUndefined();
    expect(migrationCounter({ version: 1, phase: 'write_objects', transferredBytes: 1536 })).toBe('1.5 KB sent');
    expect(parseGitProgress('Writing objects: 50% (2/4), 1.25 MiB | 512.00 KiB/s')).toMatchObject({
      completed: 2,
      total: 4,
      transferredBytes: 1310720,
      bytesPerSecond: 524288,
    });
    expect(parseGitProgress('Writing objects: 2, 5 bytes')).toEqual({
      version: 1,
      phase: 'write_objects',
      transferredBytes: 5,
    });
    expect(parseMigrationProgress('{"version":1,"phase":"empty","completed":0,"total":0}').total).toBe(0);
    expect(() => parseMigrationProgress('{"version":2,"phase":"copy"}')).toThrow(/Unsupported/);
    expect(() => parseMigrationProgress('{"version":1,"phase":"copy","completed":-1}')).toThrow(/Unsupported/);
  });
});

it('keeps an idle revision watch pending until its bounded timeout', async () => {
  const directory = await mkdtemp(resolve(tmpdir(), 'migration-idle-'));
  const locks = memoryLocks();
  let finish = () => {};
  const jobs = new MigrationJobs(
    directory,
    async () =>
      new Promise<undefined>((done) => {
        finish = () => {
          done(undefined);
        };
      }),
    undefined,
    locks.lock,
  );
  try {
    const job = await jobs.start(owner),
      started = Date.now();
    expect((await jobs.watch(owner.root, job.revision, undefined, 60))?.revision).toBe(job.revision);
    expect(Date.now() - started).toBeGreaterThanOrEqual(50);
    finish();
    let terminal = await jobs.watch(owner.root, job.revision);
    while (terminal?.status === 'running') terminal = await jobs.watch(owner.root, terminal.revision);
    expect(terminal?.status).toBe('succeeded');
  } finally {
    finish();
    await jobs.flush();
    await rm(directory, { recursive: true, force: true });
  }
});
