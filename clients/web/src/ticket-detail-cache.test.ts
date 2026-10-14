import { describe, expect, it } from 'vitest';

import type { FullTicket, TicketRow } from './api';
import { createTicketDetailCache, sameTicketRevision, TICKET_DETAIL_CACHE_LIMIT } from './ticket-detail-cache';

function row(id: string, overrides: Partial<TicketRow> = {}): TicketRow {
  return {
    connection_id: 'git',
    native_id: id,
    qualified_id: `git:${id}`,
    id,
    slug: `HS2-${id}`,
    title: `Ticket ${id}`,
    status: 'not_started',
    up_next: false,
    feedback_needed: false,
    tags: [],
    blocked_by: [],
    claim_count: 0,
    updated_at: '2026-01-01T00:00:00Z',
    ...overrides,
  };
}
function full(id: string, overrides: Partial<FullTicket> = {}): FullTicket {
  return { ...row(id), details: `details ${id}`, notes: [], attachments: [], ...overrides };
}

describe('ticket detail cache (HS2-4Q04Z9)', () => {
  it('cold: misses an uncached ticket', () => {
    expect(createTicketDetailCache().get('p', row('A'))).toBeUndefined();
  });

  it('warm: returns the cached detail as fresh when the list row has the same revision', () => {
    const cache = createTicketDetailCache(),
      ticket = full('A');
    cache.put('p', ticket);
    expect(cache.get('p', row('A'))).toEqual({ ticket, fresh: true });
  });

  it('stale + live update: overlays the newer list row fields and reports stale', () => {
    const cache = createTicketDetailCache();
    cache.put('p', full('A', { title: 'Old', status: 'not_started', blocked_reason: 'waiting' }));
    const result = cache.get('p', row('A', { title: 'New', status: 'started', updated_at: '2026-01-02T00:00:00Z' }));
    expect(result?.fresh).toBe(false);
    expect(result?.ticket).toMatchObject({ title: 'New', status: 'started', details: 'details A' });
    expect(result?.ticket.blocked_reason).toBe('waiting');
    // A refresh that lands replaces the entry, which is then fresh again.
    cache.put('p', full('A', { title: 'New', status: 'started', updated_at: '2026-01-02T00:00:00Z' }));
    expect(cache.get('p', row('A', { updated_at: '2026-01-02T00:00:00Z' }))?.fresh).toBe(true);
  });

  it('scopes entries by project', () => {
    const cache = createTicketDetailCache();
    cache.put('p1', full('A'));
    expect(cache.get('p2', row('A'))).toBeUndefined();
  });

  it('evicts the least recently used entry beyond the bound, counting reads as use', () => {
    const cache = createTicketDetailCache(2);
    cache.put('p', full('A'));
    cache.put('p', full('B'));
    cache.get('p', row('A'));
    cache.put('p', full('C'));
    expect(cache.size).toBe(2);
    expect(cache.get('p', row('B'))).toBeUndefined();
    expect(cache.get('p', row('A'))).toBeDefined();
    expect(cache.get('p', row('C'))).toBeDefined();
    expect(TICKET_DETAIL_CACHE_LIMIT).toBeGreaterThan(0);
  });

  it('repeated put, delete, clear, then refill', () => {
    const cache = createTicketDetailCache(2);
    cache.put('p', full('A'));
    cache.put('p', full('A', { details: 'edited' }));
    expect(cache.size).toBe(1);
    expect(cache.get('p', row('A'))?.ticket.details).toBe('edited');
    cache.delete('p', 'git:A');
    expect(cache.get('p', row('A'))).toBeUndefined();
    cache.put('p', full('B'));
    cache.clear();
    expect(cache.size).toBe(0);
    cache.put('p', full('B'));
    expect(cache.get('p', row('B'))?.fresh).toBe(true);
  });

  it('compares revisions for the background refresh', () => {
    const shown = full('A', { concurrency_token: 't1' });
    expect(sameTicketRevision(shown, { ...shown })).toBe(true);
    expect(sameTicketRevision(shown, { ...shown, concurrency_token: 't2' })).toBe(false);
    expect(sameTicketRevision(shown, { ...shown, updated_at: 'later' })).toBe(false);
    expect(sameTicketRevision(shown, { ...shown, notes: [{} as never] })).toBe(false);
    expect(sameTicketRevision(shown, { ...shown, attachments: [{} as never] })).toBe(false);
    expect(sameTicketRevision(shown, full('B'))).toBe(false);
  });
});
