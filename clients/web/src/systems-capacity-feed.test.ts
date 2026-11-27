import { describe, expect, it, vi } from 'vitest';

import type { OrchestrationCapacity } from './api';
import { createSystemsCapacityFeeds } from './systems-capacity-feed';
import { systemsDemoCapacity } from './ux-demo/systems-fixtures';

const snapshot = (version: string | undefined, free = 1000): OrchestrationCapacity => {
  const capacity = { ...systemsDemoCapacity('local'), memory: { free_mb: free, total_mb: 4000 } };
  if (version === undefined) delete capacity.version;
  else capacity.version = version;
  return capacity;
};

/** A scripted server: each read waits until the test answers it. */
function server() {
  const reads: {
    projectId: string;
    after: string | undefined;
    signal: AbortSignal;
    resolve: (value: OrchestrationCapacity) => void;
    reject: (reason: unknown) => void;
  }[] = [];
  const read = vi.fn(
    (projectId: string, after: string | undefined, signal: AbortSignal) =>
      new Promise<OrchestrationCapacity>((resolve, reject) => {
        reads.push({ projectId, after, signal, resolve, reject });
      }),
  );
  return { reads, read };
}

const settle = () => new Promise((resolve) => setTimeout(resolve, 0));

describe('HS2-DR0ZSQ Systems capacity streams', () => {
  it('long-polls each machine with the version it last saw and reports each new snapshot once', async () => {
    const { reads, read } = server();
    let clock = 0;
    const delay = vi.fn(() => Promise.resolve());
    const feeds = createSystemsCapacityFeeds(read, { delay, now: () => clock });
    feeds.sync([{ machineId: 'local', projectId: 'p1' }]);
    await settle();
    expect(reads.map((item) => [item.projectId, item.after])).toEqual([['p1', undefined]]);
    reads[0].resolve(snapshot('v1'));
    await settle();
    expect(feeds.states.value.get('local')?.capacity?.version).toBe('v1');
    expect(reads[1].after).toBe('v1');
    // The server held the read past the floor and answered unchanged: no new report, no backoff.
    clock += 8000;
    const before = feeds.states.value;
    reads[1].resolve(snapshot('v1'));
    await settle();
    expect(feeds.states.value).toBe(before);
    expect(delay).not.toHaveBeenCalled();
    expect(reads[2].after).toBe('v1');
    reads[2].resolve(snapshot('v2', 500));
    await settle();
    expect(feeds.states.value.get('local')?.capacity?.memory.free_mb).toBe(500);
    feeds.stop();
    expect(reads[3].signal.aborted).toBe(true);
    expect(feeds.running()).toEqual([]);
  });

  it('keeps the last snapshot with the error on a failure, backs off, then recovers', async () => {
    const { reads, read } = server();
    const waits: number[] = [];
    const feeds = createSystemsCapacityFeeds(read, {
      delay: (ms) => {
        waits.push(ms);
        return Promise.resolve();
      },
      now: () => 0,
    });
    feeds.sync([{ machineId: 'local', projectId: 'p1' }]);
    await settle();
    reads[0].resolve(snapshot('v1'));
    await settle();
    reads[1].reject(new TypeError('Failed to fetch'));
    await settle();
    expect(feeds.states.value.get('local')).toEqual({
      capacity: snapshot('v1'),
      error: 'The Hot Sheet server could not be reached.',
    });
    expect(waits).toEqual([1000]);
    // After a failure the stream forgets its version and reads at once again.
    expect(reads[2].after).toBeUndefined();
    reads[2].resolve(snapshot('v1'));
    await settle();
    expect(feeds.states.value.get('local')).toEqual({ capacity: snapshot('v1') });
    feeds.stop();
  });

  it('starts new machines, restarts one whose project changed, and stops the rest', async () => {
    const { reads, read } = server();
    const feeds = createSystemsCapacityFeeds(read, { delay: () => Promise.resolve(), now: () => 0 });
    feeds.sync([
      { machineId: 'local', projectId: 'p1' },
      { machineId: 'remote:a', projectId: 'r1' },
    ]);
    await settle();
    expect(reads.map((item) => item.projectId)).toEqual(['p1', 'r1']);
    // Repeating the same set changes nothing.
    feeds.sync([
      { machineId: 'local', projectId: 'p1' },
      { machineId: 'remote:a', projectId: 'r1' },
    ]);
    await settle();
    expect(reads).toHaveLength(2);
    feeds.sync([{ machineId: 'remote:a', projectId: 'r2' }]);
    await settle();
    expect(reads[0].signal.aborted).toBe(true);
    expect(reads[1].signal.aborted).toBe(true);
    expect(reads[2].projectId).toBe('r2');
    expect(feeds.running()).toEqual(['remote:a']);
    // A late answer from the stopped stream is ignored.
    reads[1].resolve(snapshot('old'));
    await settle();
    expect(feeds.states.value.has('remote:a')).toBe(false);
    reads[2].resolve(snapshot('new'));
    await settle();
    expect(feeds.states.value.get('remote:a')?.capacity?.version).toBe('new');
    // Empty, then refilled: the machine reads again from the start, its last reading kept meanwhile.
    feeds.sync([]);
    expect(feeds.states.value.get('remote:a')?.capacity?.version).toBe('new');
    feeds.sync([{ machineId: 'remote:a', projectId: 'r2' }]);
    await settle();
    expect(reads.at(-1)?.after).toBeUndefined();
    feeds.forget('remote:a');
    feeds.forget('remote:a');
    expect(feeds.states.value.has('remote:a')).toBe(false);
    feeds.stop();
  });

  it('backs off on an older host that sends no version instead of reading in a tight loop', async () => {
    const { reads, read } = server();
    const waits: number[] = [];
    const feeds = createSystemsCapacityFeeds(read, {
      delay: (ms) => {
        waits.push(ms);
        return Promise.resolve();
      },
      now: () => 0,
    });
    feeds.sync([{ machineId: 'local', projectId: 'p1' }]);
    await settle();
    reads[0].resolve(snapshot(undefined));
    await settle();
    expect(feeds.states.value.get('local')?.capacity).toBeDefined();
    expect(reads[1].after).toBeUndefined();
    reads[1].resolve(snapshot(undefined));
    await settle();
    reads[2].resolve(snapshot(undefined));
    await settle();
    expect(waits).toEqual([1000, 2000]);
    feeds.stop();
  });
});
