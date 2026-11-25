import { describe, expect, it } from 'vitest';

import { toastReadingTime } from './toast-lifetime';
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
      'Emptied Trash — 0 tickets permanently removed. 2 tickets stayed in Trash (acme/repo#5, acme/repo#6): this connection may not delete.',
    );
    expect(trashEmptyLeftTickets(result)).toBe(true);
  });

  it('names the first three kept tickets and counts the rest', () => {
    const retained = [5, 6, 7, 8, 9].map((number) => ({
      qualified_id: `gitlab-main:${number}`,
      slug: `acme/repo#${number}`,
      message: 'refused',
    }));
    expect(trashEmptySummary({ purged: 0, tickets: [], retained })).toBe(
      'Emptied Trash — 0 tickets permanently removed. 5 tickets stayed in Trash (acme/repo#5, acme/repo#6, acme/repo#7 and 2 more): refused.',
    );
    expect(trashEmptySummary({ purged: 0, tickets: [], retained: retained.slice(0, 3) })).toContain(
      '(acme/repo#5, acme/repo#6, acme/repo#7): refused.',
    );
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

  it('keeps a plain result at the default lifetime and gives an explanation reading time (HS2-PC7VRA)', () => {
    // The central toast rule replaced the per-call Empty Trash duration with the same numbers.
    const lifetime = (result: Parameters<typeof trashEmptySummary>[0]) => toastReadingTime(trashEmptySummary(result));
    expect(lifetime({ purged: 3, tickets: ['AA-1', 'AA-2', 'AA-3'] })).toBe(2_500);
    const reason =
      "this connection's credentials cannot permanently delete tickets in the provider, so the ticket stays closed in Trash; reconnect with an account that can delete issues, or delete it in the provider (GitLab said: 403 Forbidden)";
    const kept = {
      purged: 1,
      tickets: ['AA-1'],
      retained: [{ qualified_id: 'gitlab-main:5', slug: 'acme/repo#5', message: reason }],
    };
    const words = trashEmptySummary(kept).split(/\s+/).length;
    expect(lifetime(kept)).toBe(words * 300);
    expect(lifetime(kept)).toBeGreaterThan(10_000);
    const short = { purged: 0, tickets: [], unavailable: [{ connection_id: 'x', message: 'down' }] };
    expect(lifetime(short)).toBe(Math.max(2_500, trashEmptySummary(short).split(/\s+/).length * 300));
    const many = {
      purged: 0,
      tickets: [],
      unavailable: Array.from({ length: 40 }, (_, index) => ({ connection_id: `c${index}`, message: 'unreachable' })),
    };
    expect(lifetime(many)).toBe(20_000);
  });
});
