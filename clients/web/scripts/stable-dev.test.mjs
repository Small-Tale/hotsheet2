import { EventEmitter } from 'node:events';
import { lstat, mkdir, mkdtemp, readdir, readFile, stat, utimes, writeFile } from 'node:fs/promises';
import { tmpdir } from 'node:os';
import { resolve } from 'node:path';

import { afterEach, describe, expect, it } from 'vitest';

import {
  createStableSnapshot,
  pruneStaleSnapshots,
  removeStableSnapshot,
  runStableDev,
  stableDevEnvironment,
  stableSnapshotParent,
} from './stable-dev.mjs';

const cleanup = [];
afterEach(async () => {
  await Promise.all(cleanup.splice(0).map((path) => removeStableSnapshot(path)));
});

describe('stable dev snapshot', () => {
  it('copies source once, excludes generated output, and shares installed dependencies', async () => {
    const source = await mkdtemp(resolve(tmpdir(), 'hotsheet-web-source-'));
    cleanup.push(source);
    await mkdir(resolve(source, 'src'));
    await mkdir(resolve(source, 'node_modules'));
    await mkdir(resolve(source, 'dist'));
    await mkdir(resolve(source, 'target'));
    await writeFile(resolve(source, 'src/main.ts'), 'before');
    await writeFile(resolve(source, 'dist/generated.js'), 'excluded');
    await writeFile(resolve(source, 'target/generated'), 'excluded');

    const parent = await mkdtemp(resolve(tmpdir(), 'hotsheet-web-parent-'));
    cleanup.push(parent);
    const snapshot = await createStableSnapshot(source, resolve(parent, 'nested'), 4242);
    cleanup.push(snapshot);
    // The owner is recorded so a later startup can prune this snapshot once its process is gone.
    expect(JSON.parse(await readFile(resolve(snapshot, '.stable-dev-owner.json'), 'utf8'))).toEqual({ pid: 4242 });
    await writeFile(resolve(source, 'src/main.ts'), 'after');

    expect(await readFile(resolve(snapshot, 'src/main.ts'), 'utf8')).toBe('before');
    expect((await lstat(resolve(snapshot, 'node_modules'))).isSymbolicLink()).toBe(true);
    await expect(stat(resolve(snapshot, 'dist'))).rejects.toMatchObject({ code: 'ENOENT' });
    await expect(stat(resolve(snapshot, 'target'))).rejects.toMatchObject({ code: 'ENOENT' });
  });

  it('keeps snapshots out of the swept OS temporary directory (HS2-ZJ6VN3)', () => {
    expect(stableSnapshotParent({}, 'darwin', '/Users/dev')).toBe('/Users/dev/Library/Caches/hotsheet-web-stable');
    expect(stableSnapshotParent({}, 'linux', '/home/dev')).toBe('/home/dev/.cache/hotsheet-web-stable');
    expect(stableSnapshotParent({ XDG_CACHE_HOME: '/cache' }, 'linux', '/home/dev')).toBe('/cache/hotsheet-web-stable');
    expect(stableSnapshotParent({ LOCALAPPDATA: '/local' }, 'win32', '/users/dev')).toBe('/local/hotsheet-web-stable');
    expect(stableSnapshotParent({ HOTSHEET_WEB_STABLE_TEMP_ROOT: '/override' }, 'darwin', '/Users/dev')).toBe(
      '/override',
    );
  });

  it('prunes only snapshots whose owner is gone or that never recorded one', async () => {
    const parent = await mkdtemp(resolve(tmpdir(), 'hotsheet-web-prune-'));
    cleanup.push(parent);
    const make = async (name, owner, ageMs = 0) => {
      const path = resolve(parent, name);
      await mkdir(path);
      if (owner !== undefined) await writeFile(resolve(path, '.stable-dev-owner.json'), JSON.stringify({ pid: owner }));
      const when = new Date(Date.now() - ageMs);
      await utimes(path, when, when);
      return path;
    };
    await make('hotsheet-web-stable-live', 100);
    await make('hotsheet-web-stable-dead', 200);
    await make('hotsheet-web-stable-starting', undefined);
    await make('hotsheet-web-stable-orphan', undefined, 60 * 60 * 1000);
    await make('unrelated-directory', 200);
    const removed = await pruneStaleSnapshots(parent, { isAlive: (pid) => pid === 100 });
    expect(removed.map((path) => path.split('/').at(-1)).sort()).toEqual([
      'hotsheet-web-stable-dead',
      'hotsheet-web-stable-orphan',
    ]);
    expect((await readdir(parent)).sort()).toEqual([
      'hotsheet-web-stable-live',
      'hotsheet-web-stable-starting',
      'unrelated-directory',
    ]);
    // A missing parent is not an error: there is simply nothing to prune yet.
    expect(await pruneStaleSnapshots(resolve(parent, 'missing'))).toEqual([]);
  });

  /** Deterministic timers plus a fake Vite child for the shutdown paths (HS2-4SSWV5). */
  function shutdownHarness({ ppid = 500 } = {}) {
    const intervals = new Map(),
      timeouts = new Map();
    let next = 0;
    const timers = {
      setInterval: (callback) => {
        next += 1;
        intervals.set(next, callback);
        return next;
      },
      clearInterval: (id) => intervals.delete(id),
      setTimeout: (callback) => {
        next += 1;
        timeouts.set(next, callback);
        return next;
      },
      clearTimeout: (id) => timeouts.delete(id),
    };
    const processHost = Object.assign(new EventEmitter(), { ppid, execPath: 'node' });
    const child = Object.assign(new EventEmitter(), { signals: [] });
    child.kill = (signal) => {
      child.signals.push(signal);
      return true;
    };
    let started;
    const childStarted = new Promise((resolveStarted) => {
      started = resolveStarted;
    });
    const running = runStableDev({
      sourceRoot: '/work/web',
      temporaryRoot: '/tmp/runtime',
      environment: {},
      processHost,
      timers,
      createSnapshot: async () => '/tmp/runtime/hotsheet-web-stable-test',
      removeSnapshot: async () => undefined,
      pruneSnapshots: async () => [],
      spawnChild: () => {
        started();
        return child;
      },
      log: () => undefined,
    });
    return { running, childStarted, child, processHost, intervals, timeouts };
  }

  it('shuts Vite down when its parent disappears and force-kills a child that ignores SIGTERM', async () => {
    const harness = shutdownHarness();
    await harness.childStarted;
    expect(harness.intervals.size).toBe(1);
    const [check] = harness.intervals.values();
    check();
    expect(harness.child.signals).toEqual([], 'the parent is still alive');
    harness.processHost.ppid = 1;
    check();
    check();
    expect(harness.child.signals).toEqual(['SIGTERM']);
    const [force] = harness.timeouts.values();
    force();
    expect(harness.child.signals).toEqual(['SIGTERM', 'SIGKILL']);
    harness.child.emit('close', null, 'SIGKILL');
    expect(await harness.running).toBe(143);
    expect(harness.intervals.size).toBe(0);
    expect(harness.timeouts.size).toBe(0);
  });

  it('cancels the force kill when Vite exits promptly after a signal', async () => {
    const harness = shutdownHarness();
    await harness.childStarted;
    harness.processHost.emit('SIGINT');
    expect(harness.child.signals).toEqual(['SIGINT']);
    expect(harness.timeouts.size).toBe(1);
    harness.child.emit('close', null, 'SIGINT');
    expect(await harness.running).toBe(130);
    expect(harness.timeouts.size).toBe(0);
    expect(harness.child.signals).toEqual(['SIGINT']);
  });

  it('does not watch a parent it never had', async () => {
    const harness = shutdownHarness({ ppid: 1 });
    await harness.childStarted;
    expect(harness.intervals.size).toBe(0);
    harness.child.emit('close', 0, null);
    expect(await harness.running).toBe(0);
  });

  it('preserves the original repository root for every snapshot-side bridge', () => {
    const environment = stableDevEnvironment('/work/hotsheet2/clients/web', '/tmp/stable-snapshot', {});
    expect(environment.HOTSHEET_REPO_ROOT).toBe('/work/hotsheet2');
    expect(environment.HOTSHEET_DEV_REVIEW_REPO_ROOT).toBe('/work/hotsheet2');
    expect(environment.HOTSHEET_WEB_STABLE_DEV).toBe('1');
    expect(environment.HOTSHEET_VITE_CACHE_DIR).toBe('/tmp/stable-snapshot/.vite-cache');

    const overridden = stableDevEnvironment('/snapshot/web', '/tmp/other-snapshot', {
      HOTSHEET_REPO_ROOT: '/real/repository',
      HOTSHEET_DEV_REVIEW_REPO_ROOT: '/review/repository',
      HOTSHEET_VITE_CACHE_DIR: '/shared/cache-that-must-not-be-used',
    });
    expect(overridden.HOTSHEET_REPO_ROOT).toBe('/real/repository');
    expect(overridden.HOTSHEET_DEV_REVIEW_REPO_ROOT).toBe('/review/repository');
    expect(overridden.HOTSHEET_VITE_CACHE_DIR).toBe('/tmp/other-snapshot/.vite-cache');
  });

  it('awaits and removes a snapshot when shutdown arrives during startup', async () => {
    const processHost = new EventEmitter();
    processHost.execPath = '/test/node';
    let finishSnapshot;
    const snapshotReady = new Promise((resolveReady) => {
      finishSnapshot = resolveReady;
    });
    let announceRemoval;
    const removalStarted = new Promise((resolveStarted) => {
      announceRemoval = resolveStarted;
    });
    let finishRemoval;
    const removalFinished = new Promise((resolveFinished) => {
      finishRemoval = resolveFinished;
    });
    const removed = [];
    const running = runStableDev({
      sourceRoot: '/work/web',
      temporaryRoot: '/tmp/runtime',
      environment: {},
      processHost,
      createSnapshot: async () => {
        await snapshotReady;
        return '/tmp/runtime/hotsheet-web-stable-test';
      },
      removeSnapshot: async (path) => {
        removed.push(path);
        announceRemoval();
        await removalFinished;
      },
      spawnChild: () => {
        throw new Error('Vite must not start after shutdown begins.');
      },
      log: () => undefined,
    });
    let settled = false;
    void running.finally(() => {
      settled = true;
    });

    processHost.emit('SIGTERM');
    finishSnapshot();
    await removalStarted;

    expect(settled).toBe(false);
    finishRemoval();
    await expect(running).resolves.toBe(143);
    expect(removed).toEqual(['/tmp/runtime/hotsheet-web-stable-test']);
    expect(processHost.listenerCount('SIGINT')).toBe(0);
    expect(processHost.listenerCount('SIGTERM')).toBe(0);
  });
});
