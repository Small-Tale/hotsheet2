import { describe, expect, it } from 'vitest';

import {
  applyRememberedTabOrder,
  insertTabByRank,
  interleaveByRank,
  reorderTabs,
  replaceTabInPlace,
} from './tab-order';

const items = ['one', 'two', 'three'].map((id) => ({ id })),
  identity = (item: { id: string }) => item.id;

describe('tab order', () => {
  it('moves tabs before or after a target without losing identity', () => {
    expect(reorderTabs(items, identity, 'three', 'one', 'before').map(identity)).toEqual(['three', 'one', 'two']);
    expect(reorderTabs(items, identity, 'one', 'two', 'after').map(identity)).toEqual(['two', 'one', 'three']);
    expect(reorderTabs(items, identity, 'missing', 'two', 'after')).toEqual(items);
  });

  it('restores a partial remembered order and appends newly discovered tabs stably', () => {
    expect(applyRememberedTabOrder(items, identity, ['three', 'one']).map(identity)).toEqual(['three', 'one', 'two']);
    expect(applyRememberedTabOrder(items, identity, ['removed', 'two']).map(identity)).toEqual(['two', 'one', 'three']);
  });

  it('updates a reopened tab in place and appends a genuinely new tab', () => {
    expect(replaceTabInPlace(items, identity, { id: 'two', name: 'updated' } as { id: string }).map(identity)).toEqual([
      'one',
      'two',
      'three',
    ]);
    expect(replaceTabInPlace(items, identity, { id: 'four' }).map(identity)).toEqual(['one', 'two', 'three', 'four']);
  });
});

describe('insertTabByRank (HS2-X74D4B)', () => {
  const identity = (item: { id: string }) => item.id;
  it('lands out-of-order registrations in remembered positions, replaces in place, and keeps unranked tabs last', () => {
    const ranks = new Map<string, number>(),
      rankOf = (item: { id: string }) => ranks.get(item.id),
      add = (items: { id: string }[], id: string, rank?: number) => {
        if (rank !== undefined) ranks.set(id, rank);
        return rank === undefined
          ? replaceTabInPlace(items, identity, { id })
          : insertTabByRank(items, identity, { id }, rank, rankOf);
      };
    // The active middle project registers first, then a user-opened (unranked) project, then the rest.
    let items = add([], 'beta', 1);
    items = add(items, 'opened-later');
    items = add(items, 'gamma', 2);
    items = add(items, 'alpha', 0);
    expect(items.map(identity)).toEqual(['alpha', 'beta', 'gamma', 'opened-later']);
    // Re-registering an existing tab replaces it without moving it.
    const updated = insertTabByRank(items, identity, { id: 'beta', name: 'renamed' } as { id: string }, 1, rankOf);
    expect(updated.map(identity)).toEqual(['alpha', 'beta', 'gamma', 'opened-later']);
    expect(updated[1]).toEqual({ id: 'beta', name: 'renamed' });
    // Empty, then refill.
    expect(insertTabByRank([], identity, { id: 'solo' }, 5, rankOf).map(identity)).toEqual(['solo']);
  });
});

describe('interleaveByRank (HS2-2BEJXD)', () => {
  const ranks = new Map([
    ['beta', 1],
    ['delta', 3],
  ]);
  const rankOf = (id: string) => ranks.get(id);
  it('places pending entries at their remembered positions around ranked and unranked items', () => {
    expect(
      interleaveByRank(['beta', 'delta', 'opened-later'], rankOf, [
        { rank: 2, item: 'gamma' },
        { rank: 0, item: 'alpha' },
      ]),
    ).toEqual(['alpha', 'beta', 'gamma', 'delta', 'opened-later']);
    // Pending entries ranked after every item trail them; nothing pending leaves the list unchanged.
    expect(interleaveByRank(['beta'], rankOf, [{ rank: 9, item: 'omega' }])).toEqual(['beta', 'omega']);
    expect(interleaveByRank(['beta', 'delta'], rankOf, [])).toEqual(['beta', 'delta']);
    // Only pending (nothing registered yet), then empty.
    expect(
      interleaveByRank([], rankOf, [
        { rank: 1, item: 'b' },
        { rank: 0, item: 'a' },
      ]),
    ).toEqual(['a', 'b']);
    expect(interleaveByRank([], rankOf, [])).toEqual([]);
  });
});
