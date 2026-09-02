import { describe, expect, it } from 'vitest';

import type { Capabilities, TicketRow } from './api';
import { bulkTagChoices, bulkTicketPatch, canBulkUpdate } from './ticket-bulk-operations';

const ticket = (slug: string, connection_id = 'git', tags: string[] = []): TicketRow => ({
  connection_id, native_id: slug, qualified_id: `${connection_id}:${slug}`, id: slug, slug, title: slug,
  up_next: false, feedback_needed: false, tags, blocked_by: [], claim_count: 0,
});
const capabilities = (update: boolean, atomic_batch = update) => ({ update, atomic_batch } as Capabilities);

describe('bulk ticket operations', () => {
  it('requires atomic batch update support from every selected provider', () => {
    const selected = [ticket('ONE', 'git'), ticket('TWO', 'jira')];
    expect(canBulkUpdate(selected, id => capabilities(id === 'git'))).toBe(false);
    expect(canBulkUpdate(selected, () => capabilities(true))).toBe(true);
    expect(canBulkUpdate(selected, () => capabilities(true, false))).toBe(false);
    expect(canBulkUpdate([], () => capabilities(true))).toBe(false);
  });

  it('builds field and soft-delete patches', () => {
    expect(bulkTicketPatch(ticket('ONE'), { kind: 'field', field: 'priority', value: 'high' })).toEqual({ priority: 'high' });
    expect(bulkTicketPatch(ticket('ONE'), { kind: 'delete' })).toEqual({ status: 'deleted' });
  });

  it('adds and removes a tag per ticket without duplicates or empty writes', () => {
    const tagged = ticket('ONE', 'git', ['client', 'bug']);
    expect(bulkTicketPatch(tagged, { kind: 'add-tag', tag: ' regression ' })).toEqual({ tags: ['client', 'bug', 'regression'] });
    expect(bulkTicketPatch(tagged, { kind: 'add-tag', tag: 'client' })).toBeUndefined();
    expect(bulkTicketPatch(tagged, { kind: 'remove-tag', tag: 'client' })).toEqual({ tags: ['bug'] });
    expect(bulkTicketPatch(tagged, { kind: 'remove-tag', tag: 'missing' })).toBeUndefined();
    expect(bulkTicketPatch(tagged, { kind: 'add-tag', tag: ' ' })).toBeUndefined();
  });

  it('collects stable unique remove choices across a mixed selection', () => {
    expect(bulkTagChoices([ticket('ONE', 'git', ['zeta', 'client']), ticket('TWO', 'git', ['client', 'alpha'])])).toEqual(['alpha', 'client', 'zeta']);
  });
});
