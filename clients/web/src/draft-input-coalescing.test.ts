import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { coalescedDraft, coalescedKeyedDraft, flushDraftInputs } from './draft-input-coalescing';

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

  it('keeps linked reader frames apart: switching frame or note publishes the previous draft (HS2-WV0VPE)', () => {
    const published: string[] = [];
    const input = coalescedKeyedDraft<{ frame: string; note: string; value: string }>(
      ({ frame, note }) => `${frame}/${note}`,
      ({ frame, note, value }) => published.push(`${frame}/${note}=${value}`),
    );
    input.schedule({ frame: 'a', note: '1', value: 'he' });
    input.schedule({ frame: 'a', note: '1', value: 'hello' });
    expect(published).toEqual([]);
    // Typing into another note of the same frame, then another frame, never drops text.
    input.schedule({ frame: 'a', note: '2', value: 'x' });
    expect(published).toEqual(['a/1=hello']);
    input.schedule({ frame: 'b', note: '2', value: 'y' });
    expect(published).toEqual(['a/1=hello', 'a/2=x']);
    // The shared flush (frame close, save, refresh merge) publishes the latest pending draft.
    flushDraftInputs();
    expect(published).toEqual(['a/1=hello', 'a/2=x', 'b/2=y']);
    input.schedule({ frame: 'b', note: '2', value: 'yz' });
    vi.advanceTimersByTime(150);
    expect(published.at(-1)).toBe('b/2=yz');
    flushDraftInputs();
    expect(published).toHaveLength(4);
  });
});
