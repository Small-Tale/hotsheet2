import { describe, expect, it } from 'vitest';

import { cycleTabId } from './tab-cycle';

describe('cycleTabId', () => {
  const order = ['a', 'b', 'c'];

  it('returns undefined for an empty order regardless of direction', () => {
    expect(cycleTabId([], 'a', 1)).toBeUndefined();
    expect(cycleTabId([], undefined, -1)).toBeUndefined();
  });

  it('advances forward and wraps past the end', () => {
    expect(cycleTabId(order, 'a', 1)).toBe('b');
    expect(cycleTabId(order, 'b', 1)).toBe('c');
    expect(cycleTabId(order, 'c', 1)).toBe('a');
  });

  it('advances backward and wraps past the start', () => {
    expect(cycleTabId(order, 'c', -1)).toBe('b');
    expect(cycleTabId(order, 'b', -1)).toBe('a');
    expect(cycleTabId(order, 'a', -1)).toBe('c');
  });

  it('enters at the first id forward and the last id backward when nothing is selected', () => {
    expect(cycleTabId(order, undefined, 1)).toBe('a');
    expect(cycleTabId(order, undefined, -1)).toBe('c');
  });

  it('enters at the appropriate end when the current id is stale (not in the order)', () => {
    expect(cycleTabId(order, 'gone', 1)).toBe('a');
    expect(cycleTabId(order, 'gone', -1)).toBe('c');
  });

  it('stays put for a single-entry order', () => {
    expect(cycleTabId(['only'], 'only', 1)).toBe('only');
    expect(cycleTabId(['only'], 'only', -1)).toBe('only');
  });
});
