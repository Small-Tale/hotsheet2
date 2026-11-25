import { describe, expect, it } from 'vitest';

import {
  adjacentTicketSlug,
  collectionTicketSlugs,
  decodeHiddenTicketSlugs,
  encodeHiddenTicketSlugs,
  isPlainTicketReselection,
  selectAllTickets,
  updateTicketSelection,
} from './ticket-selection';

describe('HS2-3SJ5G8 Select All includes tickets in collapsed groups', () => {
  it('collects drawn rows and collapsed members in drawn order, mixing collapsed and expanded groups', () => {
    const entries = [
      { slug: 'HS2-A' },
      { hiddenSlugs: ['HS2-B', 'HS2-C'] }, // collapsed group
      { slug: 'HS2-D' }, // expanded group's rows are ordinary drawn rows
      { slug: 'HS2-E' },
      { hiddenSlugs: ['HS2-F'] },
      { slug: 'HS2-G' },
    ];
    const all = collectionTicketSlugs(entries);
    expect(all).toEqual(['HS2-A', 'HS2-B', 'HS2-C', 'HS2-D', 'HS2-E', 'HS2-F', 'HS2-G']);
    const state = selectAllTickets(all, ['HS2-A', 'HS2-D', 'HS2-E', 'HS2-G']);
    expect([...state.selected]).toEqual(all);
    expect(state.anchor).toBe('HS2-A');
  });

  it('selects every member of an all-collapsed column and anchors on the first when nothing is drawn', () => {
    const all = collectionTicketSlugs([{ hiddenSlugs: ['HS2-B', 'HS2-C'] }, { hiddenSlugs: ['HS2-D'] }]);
    expect(all).toEqual(['HS2-B', 'HS2-C', 'HS2-D']);
    const state = selectAllTickets(all, []);
    expect([...state.selected]).toEqual(all);
    expect(state.anchor).toBe('HS2-B');
  });

  it('anchors on the first drawn row when a collapsed group comes first', () => {
    const all = collectionTicketSlugs([{ hiddenSlugs: ['HS2-B'] }, { slug: 'HS2-C' }]);
    expect(selectAllTickets(all, ['HS2-C']).anchor).toBe('HS2-C');
    // A reachable slug outside the selection is never the anchor.
    expect(selectAllTickets(all, ['HS2-X', 'HS2-C']).anchor).toBe('HS2-C');
  });

  it('selects nothing in an empty column, and an empty collapsed group adds nothing', () => {
    expect(collectionTicketSlugs([])).toEqual([]);
    expect(collectionTicketSlugs([{ hiddenSlugs: [] }])).toEqual([]);
    const state = selectAllTickets([], []);
    expect(state.selected.size).toBe(0);
    expect(state.anchor).toBeUndefined();
  });

  it('never selects a ticket twice', () => {
    expect(collectionTicketSlugs([{ slug: 'HS2-A' }, { hiddenSlugs: ['HS2-A', 'HS2-B', 'HS2-B'] }])).toEqual([
      'HS2-A',
      'HS2-B',
    ]);
  });

  it('deselects a hidden member with a toggle and reselects it, and a plain click replaces the whole selection', () => {
    const all = collectionTicketSlugs([{ slug: 'HS2-A' }, { hiddenSlugs: ['HS2-B', 'HS2-C'] }, { slug: 'HS2-D' }]);
    let state = selectAllTickets(all, ['HS2-A', 'HS2-D']);
    state = updateTicketSelection(all, state, 'HS2-B', { toggle: true });
    expect([...state.selected]).toEqual(['HS2-A', 'HS2-C', 'HS2-D']);
    state = updateTicketSelection(all, state, 'HS2-B', { toggle: true });
    expect(new Set(state.selected)).toEqual(new Set(all));
    // A plain click on a drawn row deselects everything else, hidden members included.
    state = updateTicketSelection(['HS2-A', 'HS2-D'], state, 'HS2-D');
    expect([...state.selected]).toEqual(['HS2-D']);
    // Select All again after deselecting takes every ticket once more.
    expect([...selectAllTickets(all, ['HS2-A', 'HS2-D']).selected]).toEqual(all);
  });

  it('round-trips hidden member slugs and treats a missing or malformed value as none', () => {
    const slugs = ['HS2-A', 'gh:owner/repo#12', 'with space'];
    expect(decodeHiddenTicketSlugs(encodeHiddenTicketSlugs(slugs))).toEqual(slugs);
    expect(decodeHiddenTicketSlugs(undefined)).toEqual([]);
    expect(decodeHiddenTicketSlugs('')).toEqual([]);
    expect(decodeHiddenTicketSlugs('not json')).toEqual([]);
    expect(decodeHiddenTicketSlugs('{"a":1}')).toEqual([]);
    expect(decodeHiddenTicketSlugs('["HS2-A", 3, "", null]')).toEqual(['HS2-A']);
  });
});

const slugs = ['HS2-A', 'HS2-B', 'HS2-C', 'HS2-D'];

describe('ticket selection', () => {
  it('replaces, toggles, and extends a contiguous range from a stable anchor', () => {
    let state = updateTicketSelection(slugs, { selected: new Set() }, 'HS2-B');
    expect([...state.selected]).toEqual(['HS2-B']);
    state = updateTicketSelection(slugs, state, 'HS2-D', { toggle: true });
    expect([...state.selected]).toEqual(['HS2-B', 'HS2-D']);
    state = updateTicketSelection(slugs, state, 'HS2-B', { range: true });
    expect([...state.selected]).toEqual(['HS2-B', 'HS2-C', 'HS2-D']);
    expect(state.anchor).toBe('HS2-D');
  });

  it('supports select all, bounded keyboard movement, and unknown rows safely', () => {
    expect([...selectAllTickets(slugs).selected]).toEqual(slugs);
    expect(adjacentTicketSlug(slugs, 'HS2-A', -1)).toBe('HS2-A');
    expect(adjacentTicketSlug(slugs, 'HS2-C', 1)).toBe('HS2-D');
    const state = { anchor: 'HS2-A', selected: new Set(['HS2-A']) };
    expect(updateTicketSelection(slugs, state, 'missing')).toBe(state);
  });

  it('falls back to one ticket when a range anchor is outside the active column', () => {
    const state = { anchor: 'HS2-A', selected: new Set(['HS2-A']) };
    expect([...updateTicketSelection(['HS2-C', 'HS2-D'], state, 'HS2-D', { range: true }).selected]).toEqual(['HS2-D']);
  });

  it('only treats a plain activation of the one loaded selection as a no-op', () => {
    expect(isPlainTicketReselection(['HS2-A'], 'HS2-A', 'HS2-A')).toBe(true);
    expect(isPlainTicketReselection(['HS2-A'], undefined, 'HS2-A')).toBe(false);
    expect(isPlainTicketReselection(['HS2-A', 'HS2-B'], 'HS2-A', 'HS2-A')).toBe(false);
    expect(isPlainTicketReselection(['HS2-A'], 'HS2-A', 'HS2-A', { toggle: true })).toBe(false);
    expect(isPlainTicketReselection(['HS2-A'], 'HS2-A', 'HS2-A', { range: true })).toBe(false);
  });
});
