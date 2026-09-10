import { describe, expect, it, vi } from 'vitest';

import { loadLastTicketCategory, saveLastTicketCategory } from './ticket-category-preference';

describe('last ticket category preference', () => {
  it('loads a recognized last-used category and defaults invalid values to task', () => {
    expect(loadLastTicketCategory({ getItem: () => 'investigation' })).toBe('investigation');
    expect(loadLastTicketCategory({ getItem: () => 'custom-or-stale' })).toBe('task');
    expect(loadLastTicketCategory({ getItem: () => null })).toBe('task');
  });

  it('persists only recognized ticket categories', () => {
    const setItem = vi.fn();
    saveLastTicketCategory({ setItem }, 'requirement_change');
    saveLastTicketCategory({ setItem }, 'custom-or-stale');
    expect(setItem).toHaveBeenCalledOnce();
    expect(setItem).toHaveBeenCalledWith('hotsheet.ticket-composer.last-category.v1', 'requirement_change');
  });
});
