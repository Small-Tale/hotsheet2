import { afterEach, describe, expect, it, vi } from 'vitest';

import { createProjectChangeRefresh } from './project-change-refresh';

afterEach(() => vi.useRealTimers());

describe('project change refresh batching', () => {
  it('batches interleaved projects and bounds a continuous burst', async () => {
    vi.useFakeTimers();
    const refresh = vi.fn(async (target: { id: string }) => target.id);
    const errors = vi.fn();
    const owner = createProjectChangeRefresh(refresh, errors, { now: () => Date.now() });
    owner.schedule({ id: 'a' });
    owner.schedule({ id: 'b' });
    for (let i = 0; i < 12; i += 1) {
      await vi.advanceTimersByTimeAsync(75);
      owner.schedule({ id: 'a' });
    }
    expect(refresh.mock.calls.map(([target]) => target.id)).toEqual(['b', 'a']);
    await vi.advanceTimersByTimeAsync(180);
    expect(refresh.mock.calls.map(([target]) => target.id)).toEqual(['b', 'a', 'a']);
    expect(errors).not.toHaveBeenCalled();
    owner.dispose();
  });

  it('runs one trailing refresh after an in-flight read and cancels closed projects', async () => {
    vi.useFakeTimers();
    let finish!: () => void;
    const first = new Promise<void>((resolve) => {
      finish = resolve;
    });
    const refresh = vi
      .fn()
      .mockImplementationOnce(() => first)
      .mockResolvedValue(undefined);
    const owner = createProjectChangeRefresh(refresh, vi.fn(), { now: () => Date.now() });
    owner.schedule({ id: 'a' });
    await vi.advanceTimersByTimeAsync(180);
    owner.schedule({ id: 'a' });
    owner.schedule({ id: 'a' });
    expect(refresh).toHaveBeenCalledTimes(1);
    finish();
    await Promise.resolve();
    await vi.advanceTimersByTimeAsync(180);
    expect(refresh).toHaveBeenCalledTimes(2);
    owner.schedule({ id: 'b' });
    owner.cancel('b');
    await vi.advanceTimersByTimeAsync(1000);
    expect(refresh).toHaveBeenCalledTimes(2);
    owner.dispose();
  });

  it('does not revive a removed project after a pending refresh settles', async () => {
    vi.useFakeTimers();
    let finish!: () => void;
    const refresh = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          finish = resolve;
        }),
    );
    const owner = createProjectChangeRefresh(refresh, vi.fn(), { now: () => Date.now() });
    owner.schedule({ id: 'a' });
    await vi.advanceTimersByTimeAsync(180);
    owner.schedule({ id: 'a' });
    owner.cancel('a');
    finish();
    await vi.advanceTimersByTimeAsync(1000);
    expect(refresh).toHaveBeenCalledTimes(1);
    owner.dispose();
  });
});
