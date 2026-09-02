import { describe, expect, it } from 'vitest';

import type { FullTicket, Note } from './api';
import { ticketTimelineEntries } from './ticket-timeline-data';

const note = (id: string, kind: Note['kind'], created_at: string, text: string): Note => ({ id, kind, created_at, edited_at: created_at, text });
const ticket = (overrides: Partial<FullTicket> = {}): FullTicket => ({
  id: '01TEST', native_id: '01TEST', qualified_id: 'git:01TEST', connection_id: 'git', slug: 'HS2-TEST', title: 'Timeline test',
  details: '', status: 'verified', up_next: false, feedback_needed: false, tags: [], blocked_by: [], claim_count: 0,
  created_at: '2026-09-02T01:00:00Z', updated_at: '2026-09-02T04:00:00Z', completed_at: '2026-09-02T03:00:00Z',
  notes: [], attachments: [], ...overrides,
});

describe('ticketTimelineEntries', () => {
  it('backfills legacy lifecycle timestamps so an old ticket timeline is never empty', () => {
    expect(ticketTimelineEntries(ticket()).map(entry => entry.title)).toEqual([
      'Ticket created',
      'Completed',
    ]);
  });

  it('orders activity/status notes chronologically and ignores ordinary notes', () => {
    const entries = ticketTimelineEntries(ticket({ notes: [
      note('regular', 'regular', '2026-09-02T01:30:00Z', 'Discussion'),
      note('done', 'activity', '2026-09-02T03:00:00Z', 'Status changed from Started to Completed'),
      note('start', 'activity', '2026-09-02T02:00:00Z', 'Claude started work\nImplement the fix.'),
    ] }));
    expect(entries.map(entry => entry.id)).toEqual(['01TEST-created', 'start', 'done']);
    expect(entries[1]).toMatchObject({ title: 'Claude started work', subtitle: 'Implement the fix.' });
    expect(entries[2].emphasized).toBe(true);
    expect(entries[2].title).toBe('Completed');
  });

  it('deduplicates a persisted transition but not unrelated activity at the same time', () => {
    const entries = ticketTimelineEntries(ticket({ notes: [
      note('work', 'activity', '2026-09-02T03:00:00Z', 'Finished implementation'),
      note('done', 'activity', '2026-09-02T03:00:00Z', 'Status changed from Started to Completed'),
    ] }));
    expect(entries.map(entry => entry.title)).toEqual([
      'Ticket created',
      'Completed',
      'Finished implementation',
    ]);
  });

  it('keeps repeated and reversed transitions concise without collapsing history', () => {
    const entries = ticketTimelineEntries(ticket({ completed_at: undefined, notes: [
      note('one', 'activity', '2026-09-02T02:00:00Z', 'Status changed from Not Started to Started'),
      note('two', 'activity', '2026-09-02T03:00:00Z', 'Status changed from Started to Completed'),
      note('three', 'activity', '2026-09-02T04:00:00Z', 'Status changed from Completed to Not Started'),
      note('four', 'activity', '2026-09-02T05:00:00Z', 'Status changed from Not Started to Started'),
    ] }));
    expect(entries.map(entry => entry.title)).toEqual(['Ticket created', 'Started', 'Completed', 'Not Started', 'Started']);
  });
});
