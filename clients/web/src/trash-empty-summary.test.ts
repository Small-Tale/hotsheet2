import { describe, expect, it } from 'vitest';

import { trashEmptyLeftTickets, trashEmptySummary } from './trash-empty-summary';

describe('trashEmptySummary (GH-5)', () => {
  it('counts only removed tickets when everything was purged', () => {
    const result = { purged: 1, tickets: ['AA-1'], retained: [], unavailable: [] };
    expect(trashEmptySummary(result)).toBe('Emptied Trash — 1 ticket permanently removed.');
    expect(trashEmptyLeftTickets(result)).toBe(false);
  });

  it('accepts an older server response without provider fields', () => {
    expect(trashEmptySummary({ purged: 2, tickets: ['AA-1', 'AA-2'] })).toBe(
      'Emptied Trash — 2 tickets permanently removed.',
    );
    expect(trashEmptyLeftTickets({ purged: 0, tickets: [] })).toBe(false);
  });

  it('names tickets a provider kept with one copy of each reason', () => {
    const message = 'this connection may not delete';
    const result = {
      purged: 0,
      tickets: [],
      retained: [
        { qualified_id: 'github-main:5', slug: 'acme/repo#5', message },
        { qualified_id: 'github-main:6', slug: 'acme/repo#6', message },
      ],
    };
    expect(trashEmptySummary(result)).toBe(
      'Emptied Trash — 0 tickets permanently removed. 2 tickets stayed in Trash: this connection may not delete.',
    );
    expect(trashEmptyLeftTickets(result)).toBe(true);
  });

  it('reports a provider whose Trash could not be read', () => {
    const result = {
      purged: 1,
      tickets: ['AA-1'],
      unavailable: [{ connection_id: 'github-main', message: 'rate limited' }],
    };
    expect(trashEmptySummary(result)).toBe(
      'Emptied Trash — 1 ticket permanently removed. Could not empty github-main: rate limited.',
    );
    expect(trashEmptyLeftTickets(result)).toBe(true);
  });
});
