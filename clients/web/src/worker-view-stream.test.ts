import { describe, expect, it, vi } from 'vitest';

import type { WorkerView } from './worker-view-model';
import { startWorkerViewStream } from './worker-view-stream';

const snapshot = (cursor: string): WorkerView => ({
  cursor,
  project_id: 'demo',
  source_id: 'git',
  workers: [],
  uncovered_sources: [],
  connection_coverage: true,
});
const tick = () =>
  new Promise<void>((resolve) => {
    queueMicrotask(resolve);
  });

describe('HS2-E9SR7R worker cursor stream', () => {
  it('replays the server cursor in a real long poll and aborts on navigation', async () => {
    let release: ((value: WorkerView) => void) | undefined;
    const checkoutWorkers = vi
      .fn()
      .mockResolvedValueOnce(snapshot('one'))
      .mockImplementationOnce(
        (_checkout: string, _source: string, _cursor: string, signal: AbortSignal) =>
          new Promise<WorkerView>((resolve) => {
            release = resolve;
            signal.addEventListener('abort', () => {
              resolve(snapshot('late'));
            });
          }),
      );
    const onSnapshot = vi.fn();
    const stop = startWorkerViewStream({
      client: { checkoutWorkers },
      checkout: 'demo',
      sources: ['git'],
      onSnapshot,
      onError: vi.fn(),
    });
    await tick();
    await tick();
    expect(checkoutWorkers).toHaveBeenNthCalledWith(2, 'demo', 'git', 'one', expect.any(AbortSignal));
    stop();
    release?.(snapshot('late'));
    await tick();
    expect(onSnapshot).toHaveBeenCalledTimes(1);
  });

  it('backs off after a failure and resumes with a fresh snapshot instead of spinning', async () => {
    const checkoutWorkers = vi
      .fn()
      .mockRejectedValueOnce(new Error('offline'))
      .mockResolvedValueOnce(snapshot('recovered'))
      .mockImplementation(() => new Promise(() => {}));
    const delays: number[] = [];
    let releaseDelay: (() => void) | undefined;
    const onSnapshot = vi.fn(),
      onError = vi.fn();
    const stop = startWorkerViewStream({
      client: { checkoutWorkers },
      checkout: 'demo',
      sources: ['git'],
      onSnapshot,
      onError,
      delay: (ms) =>
        new Promise<void>((resolve) => {
          delays.push(ms);
          releaseDelay = resolve;
        }),
    });
    await tick();
    await tick();
    expect(delays).toEqual([1000]);
    expect(checkoutWorkers).toHaveBeenCalledTimes(1);
    releaseDelay?.();
    await tick();
    await tick();
    expect(checkoutWorkers).toHaveBeenNthCalledWith(2, 'demo', 'git', undefined, expect.any(AbortSignal));
    expect(onSnapshot).toHaveBeenCalledTimes(1);
    stop();
  });
});
