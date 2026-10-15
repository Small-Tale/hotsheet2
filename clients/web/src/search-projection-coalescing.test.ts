import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { flushDraftInputs } from './draft-input-coalescing';
import type { InlineSearchToken } from './inline-search';
import {
  createSearchProjection,
  isStructuralSearchChange,
  type SearchProjectionState,
} from './search-projection-coalescing';

const tag = (value: string, offset?: number): InlineSearchToken =>
  ({ kind: 'tag', value, raw: `tag:${value}`, offset }) as InlineSearchToken;

function harness() {
  let current: SearchProjectionState = { query: '', tokens: [] };
  const published: Array<{ query: string; coalesced: boolean }> = [];
  const projection = createSearchProjection(
    () => current,
    (next, coalesced) => {
      current = next;
      published.push({ query: next.query, coalesced });
    },
  );
  return { projection, published, current: () => current };
}

describe('isStructuralSearchChange (HS2-X72PD6)', () => {
  it('treats chips and empty/non-empty transitions as structural, and plain typing as not', () => {
    const state = (query: string, tokens: InlineSearchToken[] = []) => ({ query, tokens });
    expect(isStructuralSearchChange(state(''), state('a'))).toBe(true);
    expect(isStructuralSearchChange(state('a'), state(' '))).toBe(true);
    expect(isStructuralSearchChange(state('a'), state('ab'))).toBe(false);
    expect(isStructuralSearchChange(state('a'), state('a', [tag('x')]))).toBe(true);
    expect(isStructuralSearchChange(state('a', [tag('x')]), state('a', [tag('y')]))).toBe(true);
    // A chip moving because text was typed before it is not structural.
    expect(isStructuralSearchChange(state('a', [tag('x', 1)]), state('ab', [tag('x', 2)]))).toBe(false);
  });
});

describe('createSearchProjection (HS2-X72PD6)', () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });
  afterEach(() => {
    vi.useRealTimers();
  });

  it('publishes the first character at once and the rest of a typing burst at the pause', () => {
    const { projection, published } = harness();
    projection.update({ query: 'f', tokens: [] });
    expect(published).toEqual([{ query: 'f', coalesced: false }]);
    for (const query of ['fo', 'fox', 'foxe']) {
      projection.update({ query, tokens: [] });
      vi.advanceTimersByTime(100);
    }
    expect(published).toHaveLength(1);
    expect(projection.pending()).toBe(true);
    vi.advanceTimersByTime(150);
    expect(published).toEqual([
      { query: 'f', coalesced: false },
      { query: 'foxe', coalesced: true },
    ]);
    expect(projection.pending()).toBe(false);
  });

  it('lets a chip commit or a clear replace pending typing immediately', () => {
    const { projection, published, current } = harness();
    projection.update({ query: 'a', tokens: [] });
    projection.update({ query: 'ab', tokens: [] });
    projection.update({ query: 'ab', tokens: [tag('x')] });
    expect(published.at(-1)).toEqual({ query: 'ab', coalesced: false });
    expect(current().tokens).toHaveLength(1);
    projection.update({ query: 'abc', tokens: [tag('x')] });
    projection.update({ query: '', tokens: [tag('x')] });
    expect(published.at(-1)).toEqual({ query: '', coalesced: false });
    // The cancelled typing never publishes stale text afterwards.
    vi.advanceTimersByTime(500);
    expect(published.at(-1)).toEqual({ query: '', coalesced: false });
  });

  it('publishes pending typing on flush and through flushDraftInputs, then refills cleanly', () => {
    const { projection, published } = harness();
    projection.update({ query: 'a', tokens: [] });
    projection.update({ query: 'ab', tokens: [] });
    projection.flush();
    expect(published.at(-1)).toEqual({ query: 'ab', coalesced: true });
    projection.update({ query: 'abc', tokens: [] });
    flushDraftInputs();
    expect(published.at(-1)).toEqual({ query: 'abc', coalesced: true });
    // Repeated flushes with nothing pending publish nothing.
    projection.flush();
    flushDraftInputs();
    expect(published).toHaveLength(3);
    projection.update({ query: '', tokens: [] });
    projection.update({ query: 'z', tokens: [] });
    expect(published.slice(-2)).toEqual([
      { query: '', coalesced: false },
      { query: 'z', coalesced: false },
    ]);
  });

  it('publishes a programmatic replacement immediately and drops typing it supersedes', () => {
    const { projection, published } = harness();
    projection.update({ query: 'a', tokens: [] });
    projection.update({ query: 'ab', tokens: [] });
    projection.update({ query: 'abc', tokens: [] }, { immediate: true });
    expect(published.at(-1)).toEqual({ query: 'abc', coalesced: false });
    expect(projection.pending()).toBe(false);
    vi.advanceTimersByTime(500);
    expect(published).toHaveLength(2);
  });

  it('drops pending typing on cancel, as when the text returns to the published value', () => {
    const { projection, published } = harness();
    projection.update({ query: 'a', tokens: [] });
    projection.update({ query: 'ab', tokens: [] });
    projection.cancel();
    vi.advanceTimersByTime(500);
    expect(published).toEqual([{ query: 'a', coalesced: false }]);
    expect(projection.pending()).toBe(false);
  });
});
