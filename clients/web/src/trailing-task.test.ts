import { afterEach, describe, expect, it, vi } from 'vitest';

import { createTrailingTask } from './trailing-task';

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
});
