import { afterEach, describe, expect, it, vi } from 'vitest';

import { DirectPollSlots } from './change-poll-slots';

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
});

describe('direct change-poll slots', () => {
  it('bounds two independent contexts and refills slots after a poll ends', async () => {
    const occupied = new Set<string>();
    const queued = new Map<string, Array<() => void>>();
    const request = <T>(name: string, _options: unknown, callback: () => Promise<T>): Promise<T> =>
      new Promise((resolve, reject) => {
        const start = () => {
          if (occupied.has(name)) {
            queued.set(name, [...(queued.get(name) ?? []), start]);
            return;
          }
          occupied.add(name);
          void callback()
            .then(resolve, reject)
            .finally(() => {
              occupied.delete(name);
              queued.get(name)?.shift()?.();
            });
        };
        start();
      });
    vi.stubGlobal('navigator', { locks: { request } });
    const first = new DirectPollSlots();
    const second = new DirectPollSlots();
    const finish: Array<() => void> = [];
    const poll = vi.fn(
      (timeout: number) =>
        new Promise<number>((resolve) =>
          finish.push(() => {
            resolve(timeout);
          }),
        ),
    );
    const controller = new AbortController();
    const results = [first, second].flatMap((slots) =>
      Array.from({ length: 4 }, () => slots.run(controller.signal, poll)),
    );
    expect(poll).toHaveBeenCalledTimes(4);
    expect(occupied.size).toBe(4);
    finish.shift()?.();
    await Promise.resolve();
    await Promise.resolve();
    expect(poll).toHaveBeenCalledTimes(5);
    while (finish.length) {
      finish.shift()?.();
      await Promise.resolve();
      await Promise.resolve();
    }
    await expect(Promise.all(results)).resolves.toEqual(Array(8).fill(1_000));
    expect(occupied.size).toBe(0);
  });

  it('paces short fallback polls and lets a queued poll abort', async () => {
    vi.useFakeTimers();
    vi.stubGlobal('navigator', {});
    const slots = new DirectPollSlots();
    const first = vi.fn(async (timeout: number) => timeout);
    const second = vi.fn(async (timeout: number) => timeout);
    const canceled = new AbortController();
    const one = slots.run(new AbortController().signal, first);
    const two = slots.run(canceled.signal, second);
    expect(await one).toBe(0);
    expect(first).toHaveBeenCalledTimes(1);
    expect(second).not.toHaveBeenCalled();
    canceled.abort();
    await expect(two).rejects.toHaveProperty('name', 'AbortError');
    await vi.advanceTimersByTimeAsync(250);
    expect(second).not.toHaveBeenCalled();
  });
});
