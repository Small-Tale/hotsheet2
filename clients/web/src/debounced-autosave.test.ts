import { afterEach, describe, expect, it, vi } from 'vitest';

import { AUTOSAVE_DELAY_MS, createDebouncedAutosave, createFocusLossAutosave } from './debounced-autosave';

afterEach(() => {
  vi.useRealTimers();
});

describe('debounced autosave', () => {
  it('coalesces rapid edits and saves the latest value after 150 ms', async () => {
    vi.useFakeTimers();
    const save = vi.fn(async () => true);
    const autosave = createDebouncedAutosave(save);
    autosave.schedule('a');
    autosave.schedule('ab');
    autosave.schedule('abc');
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

  it('serializes a newer edit behind an active save', async () => {
    vi.useFakeTimers();
    let resolveFirst: (saved: boolean) => void = () => undefined;
    const first = new Promise<boolean>((resolve) => {
      resolveFirst = resolve;
    });
    const save = vi
      .fn<(value: string) => Promise<boolean>>()
      .mockImplementationOnce(() => first)
      .mockResolvedValue(true);
    const autosave = createDebouncedAutosave(save);
    autosave.schedule('partial');
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS);
    autosave.schedule('complete');
    await vi.advanceTimersByTimeAsync(AUTOSAVE_DELAY_MS);
    expect(save).toHaveBeenCalledOnce();
    expect(autosave.pending()).toBe(true);
    resolveFirst(true);
    await vi.runAllTimersAsync();
    expect(save).toHaveBeenCalledTimes(2);
    expect(save).toHaveBeenLastCalledWith('complete');
    expect(autosave.pending()).toBe(false);
  });
});

describe('createFocusLossAutosave (HS2-RE1PS6)', () => {
  it('persists locally while typing and saves to the server only on flush', async () => {
    vi.useFakeTimers();
    const persisted: string[] = [],
      saved: string[] = [];
    const autosave = createFocusLossAutosave(
      async (value: string) => {
        saved.push(value);
        return true;
      },
      { persist: (value) => persisted.push(value), delay: 150 },
    );
    autosave.schedule('a');
    autosave.schedule('ab');
    await vi.advanceTimersByTimeAsync(149);
    expect(persisted).toEqual([]);
    await vi.advanceTimersByTimeAsync(1);
    expect(persisted).toEqual(['ab']);
    autosave.schedule('abc');
    await vi.advanceTimersByTimeAsync(5_000);
    expect(persisted).toEqual(['ab', 'abc']);
    expect(saved).toEqual([]);
    expect(autosave.pending()).toBe(true);
    // Focus leaves: the latest value is persisted once more and sent exactly once.
    autosave.schedule('abcd');
    await expect(autosave.flush()).resolves.toBe(true);
    expect(persisted).toEqual(['ab', 'abc', 'abcd']);
    expect(saved).toEqual(['abcd']);
    expect(autosave.pending()).toBe(false);
    await expect(autosave.flush()).resolves.toBe(true);
    expect(saved).toEqual(['abcd']);
    vi.useRealTimers();
  });

  it('serializes a flush behind an in-flight save and cancels a queued value', async () => {
    let release: (value: boolean) => void = () => {};
    const saved: string[] = [];
    const autosave = createFocusLossAutosave(
      (value: string) =>
        new Promise<boolean>((resolve) => {
          saved.push(value);
          release = resolve;
        }),
      { delay: 10 },
    );
    autosave.schedule('one');
    const first = autosave.flush();
    expect(saved).toEqual(['one']);
    autosave.schedule('two');
    const second = autosave.flush();
    expect(saved).toEqual(['one']);
    release(true);
    await expect(first).resolves.toBe(true);
    // The queued flush waited for the first save and then sent the newer value.
    await vi.waitFor(() => {
      expect(saved).toEqual(['one', 'two']);
    });
    release(true);
    await expect(second).resolves.toBe(true);
    autosave.schedule('three');
    autosave.cancel();
    expect(autosave.pending()).toBe(false);
    await expect(autosave.flush()).resolves.toBe(true);
    expect(saved).toEqual(['one', 'two']);
  });
});
