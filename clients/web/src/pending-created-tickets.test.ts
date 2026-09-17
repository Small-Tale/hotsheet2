import { describe, expect, it } from 'vitest';

import type { TicketRow as WireTicketRow } from './api';
import { mergeRetainedCreatedRows, PendingCreatedTickets } from './pending-created-tickets';

const row = (id: string): WireTicketRow => ({ id, slug: id.toUpperCase() }) as WireTicketRow;

describe('PendingCreatedTickets', () => {
  it('keeps a created row while the fetched index omits it, then releases it once indexed', () => {
    const pending = new PendingCreatedTickets();
    pending.register('p', row('new'), 1_000);

    // A stale refresh whose page does not yet contain the new ticket retains it.
    expect(pending.retain('p', [row('a'), row('b')], 1_100).map(item => item.id)).toEqual(['new']);
    // Still retained on a second stale refresh (not consumed like an acknowledgement).
    expect(pending.retain('p', [row('a')], 1_200).map(item => item.id)).toEqual(['new']);
    // Once the index reflects it, the authoritative server row wins and it is dropped.
    expect(pending.retain('p', [row('new'), row('a')], 1_300)).toEqual([]);
    // Permanently released afterwards.
    expect(pending.retain('p', [row('a')], 1_400)).toEqual([]);
  });

  it('scopes retained rows to their project and expires them after the TTL', () => {
    const pending = new PendingCreatedTickets();
    pending.register('p', row('new'), 0);
    // Another project's fetch never sees project p's pending row.
    expect(pending.retain('other', [], 100)).toEqual([]);
    expect(pending.retain('p', [], 100).map(item => item.id)).toEqual(['new']);
    // After the TTL the row is no longer retained.
    expect(pending.retain('p', [], 31_000)).toEqual([]);
  });

  it('returns multiple pending rows newest-first and forgets a closed project', () => {
    const pending = new PendingCreatedTickets();
    pending.register('p', row('first'), 1_000);
    pending.register('p', row('second'), 1_001);
    expect(pending.retain('p', [], 1_002).map(item => item.id)).toEqual(['second', 'first']);
    pending.forgetProject('p');
    expect(pending.retain('p', [], 1_003)).toEqual([]);
  });

  it('re-registering the same id keeps a single entry with a refreshed timestamp', () => {
    const pending = new PendingCreatedTickets();
    pending.register('p', row('new'), 0);
    pending.register('p', row('new'), 20_000);
    // Only one entry, and its TTL is measured from the later registration.
    expect(pending.retain('p', [], 25_000).map(item => item.id)).toEqual(['new']);
    expect(pending.retain('p', [], 51_000)).toEqual([]);
  });
});

describe('mergeRetainedCreatedRows', () => {
  it('prepends retained rows the fetched page omits and never duplicates', () => {
    expect(mergeRetainedCreatedRows([row('a')], []).map(item => item.id)).toEqual(['a']);
    expect(mergeRetainedCreatedRows([row('a')], [row('new')]).map(item => item.id)).toEqual(['new', 'a']);
    // A retained row that the page already includes is not duplicated.
    expect(mergeRetainedCreatedRows([row('new'), row('a')], [row('new')]).map(item => item.id)).toEqual(['new', 'a']);
  });
});
