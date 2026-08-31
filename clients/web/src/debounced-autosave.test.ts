import { afterEach, describe, expect, it, vi } from 'vitest';

import { AUTOSAVE_DELAY_MS, createDebouncedAutosave } from './debounced-autosave';

afterEach(() => { vi.useRealTimers(); });

describe('debounced autosave', () => {
  it('coalesces rapid edits and saves the latest value after 150 ms', async () => {
    vi.useFakeTimers();
    const save = vi.fn(async () => true);
    const autosave = createDebouncedAutosave(save);
    autosave.schedule('a'); autosave.schedule('ab'); autosave.schedule('abc');
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS - 1);
    expect(save).not.toHaveBeenCalled();
    await vi.advanceTimersByTimeAsync(1);
    expect(save).toHaveBeenCalledOnce();
    expect(save).toHaveBeenCalledWith('abc');
  });

  it('flushes the latest edit immediately and supports cancelling a pending edit', async () => {
    vi.useFakeTimers();
    const save = vi.fn(async () => true);
    const autosave = createDebouncedAutosave(save);
    autosave.schedule('flush me');
    expect(await autosave.flush()).toBe(true);
    expect(save).toHaveBeenCalledWith('flush me');
    autosave.schedule('discard me');
    autosave.cancel();
    await vi.runAllTimersAsync();
    expect(save).toHaveBeenCalledOnce();
  });
});
