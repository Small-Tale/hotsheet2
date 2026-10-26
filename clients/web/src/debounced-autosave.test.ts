import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  AUTOSAVE_DELAY_MS,
  createCoalescedInput,
  createDebouncedAutosave,
  createFocusLossAutosave,
  createKeyedCoalescedInput,
} from './debounced-autosave';

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
  it('HS2-E0H7S0 binds predecessor authority to an exact destination even when another owner has the same text', async () => {
    vi.useFakeTimers();
    let owner = 'project-a:git:1:details',
      release!: (saved: boolean) => void;
    const copies: { destination: string; draft: string; predecessor?: string }[] = [],
      autosave = createFocusLossAutosave<string>(
        () =>
          new Promise((resolve) => {
            release = resolve;
          }),
        {
          prepareRecovery: (draft, previous) => {
            const destination = owner;
            return {
              destination,
              capture: () => {
                copies.push({
                  destination,
                  draft,
                  ...(previous?.destination === destination ? { predecessor: previous.value } : {}),
                });
              },
            };
          },
        },
      );
    autosave.schedule('same submitted text');
    const pending = autosave.flush();
    owner = 'project-b:git:1:details';
    autosave.schedule('new draft');
    owner = 'project-a:git:1:details';
    autosave.captureRecovery();
    expect(copies).toEqual([{ destination: 'project-b:git:1:details', draft: 'new draft' }]);
    autosave.schedule('new draft');
    autosave.captureRecovery();
    expect(copies.at(-1)).toEqual({ destination: owner, draft: 'new draft', predecessor: 'same submitted text' });
    release(true);
    await pending;
    autosave.captureRecovery();
    expect(copies.at(-1)?.draft).toBe('new draft');
    autosave.cancel();
  });
  it('HS2-E0H7S0 captures the latest owned copy behind an active save without awaiting the server', async () => {
    vi.useFakeTimers();
    let release!: (saved: boolean) => void;
    const first = new Promise<boolean>((resolve) => {
        release = resolve;
      }),
      save = vi.fn<(value: string) => Promise<boolean>>().mockReturnValueOnce(first).mockResolvedValue(false),
      copies: string[] = [];
    let owner = 'first';
    const autosave = createFocusLossAutosave(save, {
      prepareRecovery: (value) => {
        const capturedOwner = owner;
        return {
          destination: capturedOwner,
          capture: () => {
            copies.push(`${capturedOwner}:${value}`);
          },
        };
      },
    });
    autosave.schedule('old');
    const pending = autosave.flush();
    autosave.schedule('new');
    owner = 'other';
    autosave.captureRecovery();
    expect(copies).toEqual(['first:new']);
    expect(save).toHaveBeenCalledExactlyOnceWith('old');
    release(true);
    await pending;
    autosave.captureRecovery();
    expect(copies).toEqual(['first:new', 'first:new']);
    await autosave.flush(); // failed server save still needs local recovery
    autosave.captureRecovery();
    expect(copies.at(-1)).toBe('first:new');
    autosave.cancel();
    autosave.captureRecovery();
    autosave.schedule('refill');
    autosave.captureRecovery();
    expect(copies.at(-1)).toBe('other:refill');
    autosave.cancel();
  });

  it('HS2-E0H7S0 retries a failed strict copy and fences cancel/replacement from stale server success', async () => {
    vi.useFakeTimers();
    let fail = true,
      release!: (saved: boolean) => void;
    const predecessors: (string | undefined)[] = [],
      copies: string[] = [],
      autosave = createFocusLossAutosave<string>(
        () =>
          new Promise((resolve) => {
            release = resolve;
          }),
        {
          prepareRecovery: (value, predecessor) => {
            predecessors.push(predecessor?.value);
            return {
              destination: 'owned',
              capture: () => {
                if (fail) throw new Error('Synthetic quota');
                copies.push(value);
              },
            };
          },
        },
      );
    autosave.schedule('first');
    const pending = autosave.flush();
    expect(() => {
      autosave.captureRecovery();
    }).toThrow('Synthetic quota');
    autosave.cancel();
    autosave.schedule('replacement');
    expect(predecessors).toEqual([undefined, undefined]);
    release(true);
    await pending;
    fail = false;
    autosave.captureRecovery();
    expect(copies).toEqual(['replacement']);
    autosave.cancel();
    autosave.schedule('saved');
    const success = autosave.flush();
    release(true);
    await success;
    autosave.captureRecovery();
    expect(copies).toEqual(['replacement']);
  });

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

describe('createCoalescedInput (HS2-9CWDSB)', () => {
  it('walks schedule, pause, flush, cancel, and refill transitions with one publish per pause', () => {
    vi.useFakeTimers();
    try {
      const published: string[] = [];
      const input = createCoalescedInput<string>((value) => published.push(value));
      // idle -> flush / cancel: nothing to publish.
      input.flush();
      input.cancel();
      expect([published, input.pending()]).toEqual([[], false]);
      // idle -> pending -> repeated schedule -> pause publishes only the latest value.
      for (const value of ['a', 'ab', 'abc']) input.schedule(value);
      expect(input.pending()).toBe(true);
      vi.advanceTimersByTime(149);
      expect(published).toEqual([]);
      vi.advanceTimersByTime(1);
      expect([published, input.pending()]).toEqual([['abc'], false]);
      // pending -> flush publishes synchronously, exactly once, and cancels the timer.
      input.schedule('abcd');
      input.flush();
      input.flush();
      vi.advanceTimersByTime(500);
      expect(published).toEqual(['abc', 'abcd']);
      // pending -> cancel drops the draft.
      input.schedule('dropped');
      input.cancel();
      vi.advanceTimersByTime(500);
      expect(published).toEqual(['abc', 'abcd']);
      // Empty-then-refill: an empty draft is a real value, then the refill publishes.
      input.schedule('');
      vi.advanceTimersByTime(150);
      input.schedule('x');
      input.flush();
      expect(published).toEqual(['abc', 'abcd', '', 'x']);
    } finally {
      vi.useRealTimers();
    }
  });
});

describe('createKeyedCoalescedInput (HS2-2YW8R3)', () => {
  it('publishes a pending field before another field queues, and coalesces within one field', () => {
    vi.useFakeTimers();
    try {
      const published: string[] = [];
      const input = createKeyedCoalescedInput<{ key: string; text: string }>(
        (value) => value.key,
        (value) => published.push(`${value.key}=${value.text}`),
      );
      // Same field: repeated keys coalesce to one publish per pause.
      input.schedule({ key: 'a', text: 'h' });
      input.schedule({ key: 'a', text: 'hi' });
      vi.advanceTimersByTime(150);
      expect(published).toEqual(['a=hi']);
      // Field switch mid-pause publishes the old field synchronously, then the new one after its pause.
      input.schedule({ key: 'a', text: 'hi!' });
      input.schedule({ key: 'b', text: 'x' });
      expect(published).toEqual(['a=hi', 'a=hi!']);
      vi.advanceTimersByTime(150);
      expect(published).toEqual(['a=hi', 'a=hi!', 'b=x']);
      // Switch after an idle pause publishes nothing extra; cancel drops; refill after cancel publishes.
      input.schedule({ key: 'a', text: 'gone' });
      input.cancel();
      input.schedule({ key: 'b', text: 'y' });
      input.flush();
      input.flush();
      expect([published, input.pending()]).toEqual([['a=hi', 'a=hi!', 'b=x', 'b=y'], false]);
    } finally {
      vi.useRealTimers();
    }
  });
});
