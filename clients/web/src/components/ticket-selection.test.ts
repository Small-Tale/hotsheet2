import { describe, expect, it } from 'vitest';

import { adjacentTicketSlug, isPlainTicketReselection, selectAllTickets, updateTicketSelection } from './ticket-selection';

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

  it('falls back to one ticket when a range anchor is outside the active column',()=>{
    const state={anchor:'HS2-A',selected:new Set(['HS2-A'])};
    expect([...updateTicketSelection(['HS2-C','HS2-D'],state,'HS2-D',{range:true}).selected]).toEqual(['HS2-D']);
  });

  it('only treats a plain activation of the one loaded selection as a no-op', () => {
    expect(isPlainTicketReselection(['HS2-A'], 'HS2-A', 'HS2-A')).toBe(true);
    expect(isPlainTicketReselection(['HS2-A'], undefined, 'HS2-A')).toBe(false);
    expect(isPlainTicketReselection(['HS2-A', 'HS2-B'], 'HS2-A', 'HS2-A')).toBe(false);
    expect(isPlainTicketReselection(['HS2-A'], 'HS2-A', 'HS2-A', { toggle: true })).toBe(false);
    expect(isPlainTicketReselection(['HS2-A'], 'HS2-A', 'HS2-A', { range: true })).toBe(false);
  });
});
