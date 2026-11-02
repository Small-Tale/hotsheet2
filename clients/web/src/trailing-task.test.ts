import { afterEach, describe, expect, it, vi } from 'vitest';

import { createTrailingTask, type TrailingTask } from './trailing-task';

afterEach(() => vi.useRealTimers());

describe('trailing task', () => {
  it('starts only the latest operation after rapid intent changes settle', () => {
    vi.useFakeTimers();
    const run = vi.fn();
    const task = createTrailingTask(250, run);
    task.schedule('backlog');
    vi.advanceTimersByTime(100);
    task.schedule('archive');
    vi.advanceTimersByTime(100);
    task.schedule('queue');
    vi.advanceTimersByTime(249);
    expect(run).not.toHaveBeenCalled();
    vi.advanceTimersByTime(1);
    expect(run).toHaveBeenCalledOnce();
    expect(run).toHaveBeenCalledWith('queue');
  });

  it('cancels scheduled work', () => {
    vi.useFakeTimers();
    const run = vi.fn();
    const task = createTrailingTask(250, run);
    task.schedule('backlog');
    task.cancel();
    vi.runAllTimers();
    expect(run).not.toHaveBeenCalled();
  });

  it('HS2-K5E2KF preserves the public delay, reset and further-schedule contract', () => {
    vi.useFakeTimers();
    const run = vi.fn<(value: string) => void>();
    const task: TrailingTask<string> = createTrailingTask(250, run);
    task.schedule('first');
    vi.advanceTimersByTime(249);
    expect(run).not.toHaveBeenCalled();
    task.cancel();
    vi.advanceTimersByTime(1);
    expect(run).not.toHaveBeenCalled();
    task.schedule('second');
    vi.advanceTimersByTime(250);
    expect(run).toHaveBeenCalledExactlyOnceWith('second');
    task.schedule('third');
    vi.advanceTimersByTime(250);
    expect(run).toHaveBeenNthCalledWith(2, 'third');
  });
});
