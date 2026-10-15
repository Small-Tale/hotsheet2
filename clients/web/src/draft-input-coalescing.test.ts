import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { coalescedDraft, flushDraftInputs } from './draft-input-coalescing';

describe('draft input coalescing (HS2-EQTXTJ)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    flushDraftInputs();
    vi.useRealTimers();
  });

  it('publishes once per typing pause with the latest keystroke', () => {
    const published: string[] = [];
    const input = coalescedDraft<string>((value) => published.push(value));
    for (const value of ['a', 'ab', 'abc']) {
      input.schedule(value);
      vi.advanceTimersByTime(100);
    }
    expect(published).toEqual([]);
    vi.advanceTimersByTime(150);
    expect(published).toEqual(['abc']);
    expect(input.pending()).toBe(false);
  });

  it('flushes every pending editor at once, in any order, without republishing idle ones', () => {
    const title: string[] = [],
      note: string[] = [];
    const titleInput = coalescedDraft<string>((value) => title.push(value)),
      noteInput = coalescedDraft<string>((value) => note.push(value));
    titleInput.schedule('Ti');
    noteInput.schedule('No');
    titleInput.schedule('Title');
    flushDraftInputs();
    expect(title).toEqual(['Title']);
    expect(note).toEqual(['No']);
    // Nothing pending: flushing again and the expired timers publish nothing more.
    flushDraftInputs();
    vi.advanceTimersByTime(500);
    expect(title).toEqual(['Title']);
    expect(note).toEqual(['No']);
  });

  it('drops a cancelled draft and keeps later edits flowing (empty-then-refill)', () => {
    const published: string[] = [];
    const input = coalescedDraft<string>((value) => published.push(value));
    input.schedule('discard');
    input.cancel();
    flushDraftInputs();
    expect(published).toEqual([]);
    input.schedule('');
    input.schedule('kept');
    input.flush();
    input.schedule('later');
    vi.advanceTimersByTime(150);
    expect(published).toEqual(['kept', 'later']);
  });
});
