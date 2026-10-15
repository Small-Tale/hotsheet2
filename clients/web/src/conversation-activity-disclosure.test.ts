import { afterEach, describe, expect, it } from 'vitest';

import {
  activityGroupExpanded,
  activityGroupKey,
  expandedActivityGroups,
  toggleActivityGroup,
  toggledActivityGroups,
} from './conversation-activity-disclosure';

afterEach(() => {
  expandedActivityGroups.value = new Set();
});

describe('conversation activity disclosure (HS2-Y3SGH3)', () => {
  it('starts every group collapsed', () => {
    expect(activityGroupExpanded('connection-1', 'a1')).toBe(false);
    expect(activityGroupExpanded(undefined, 'a1')).toBe(false);
  });
  it('expands on the first toggle and collapses on the second', () => {
    toggleActivityGroup('connection-1', 'a1');
    expect(activityGroupExpanded('connection-1', 'a1')).toBe(true);
    toggleActivityGroup('connection-1', 'a1');
    expect(activityGroupExpanded('connection-1', 'a1')).toBe(false);
  });
  it('keeps groups and connections independent through interleaved toggles', () => {
    toggleActivityGroup('connection-1', 'a1');
    toggleActivityGroup('connection-2', 'a1');
    toggleActivityGroup('connection-1', 'b1');
    toggleActivityGroup('connection-2', 'a1');
    expect(activityGroupExpanded('connection-1', 'a1')).toBe(true);
    expect(activityGroupExpanded('connection-1', 'b1')).toBe(true);
    expect(activityGroupExpanded('connection-2', 'a1')).toBe(false);
    // A preview without a connection id has its own key space.
    expect(activityGroupExpanded(undefined, 'a1')).toBe(false);
  });
  it('returns a new set without mutating the previous state', () => {
    const before: ReadonlySet<string> = new Set([activityGroupKey('c', 'x')]),
      after = toggledActivityGroups(before, activityGroupKey('c', 'y'));
    expect([...before]).toEqual([activityGroupKey('c', 'x')]);
    expect(after.has(activityGroupKey('c', 'y'))).toBe(true);
    expect(toggledActivityGroups(after, activityGroupKey('c', 'x')).has(activityGroupKey('c', 'x'))).toBe(false);
  });
});
