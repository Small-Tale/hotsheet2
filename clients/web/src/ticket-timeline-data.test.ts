import { describe, expect, it } from 'vitest';

import type { FullTicket, Note } from './api';
import { completionConfidenceByNote, ticketTimelineEntries } from './ticket-timeline-data';

const note = (id: string, kind: Note['kind'], created_at: string, text: string): Note => ({
  id,
  kind,
  created_at,
  edited_at: created_at,
  text,
});
const ticket = (overrides: Partial<FullTicket> = {}): FullTicket => ({
  id: '01TEST',
  native_id: '01TEST',
  qualified_id: 'git:01TEST',
  connection_id: 'git',
  slug: 'HS2-TEST',
  title: 'Timeline test',
  details: '',
  status: 'verified',
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
  created_at: '2026-09-02T01:00:00Z',
  updated_at: '2026-09-02T04:00:00Z',
  completed_at: '2026-09-02T03:00:00Z',
  notes: [],
  attachments: [],
  ...overrides,
});

describe('ticketTimelineEntries', () => {
  it('backfills legacy lifecycle timestamps so an old ticket timeline is never empty', () => {
    expect(ticketTimelineEntries(ticket()).map((entry) => entry.title)).toEqual(['Ticket created', 'Completed']);
  });

  it('orders activity/status notes chronologically and keeps detail out of the timeline', () => {
    const entries = ticketTimelineEntries(
      ticket({
        notes: [
          note('regular', 'regular', '2026-09-02T01:30:00Z', 'Discussion'),
          note('done', 'activity', '2026-09-02T03:00:00Z', 'Status changed from Started to Completed'),
          note('start', 'activity', '2026-09-02T02:00:00Z', 'Claude started work\nImplement the fix.'),
        ],
      }),
    );
    expect(entries.map((entry) => entry.id)).toEqual(['01TEST-created', 'start', 'done']);
    expect(entries[1]).toMatchObject({ title: 'Claude started work' });
    expect(entries[1].subtitle).toBeUndefined();
    expect(entries[2].emphasized).toBe(true);
    expect(entries[2].title).toBe('Completed');
  });

  it('appends each completion cycle its own confidence across reopen and re-completion (HS2-DWTJ43)', () => {
    const scored = (id: string, created_at: string, confidence: number): Note => ({
      ...note(id, 'regular', created_at, '## Confidence'),
      confidence,
    });
    const notes = [
      note('start-1', 'activity', '2026-09-02T01:10:00Z', 'Status changed from Not Started to Started'),
      // A score written before the flip still belongs to that completion cycle.
      scored('early', '2026-09-02T01:20:00Z', 60),
      note('done-1', 'activity', '2026-09-02T01:30:00Z', 'Status changed from Started to Completed'),
      scored('late', '2026-09-02T01:40:00Z', 82),
      note('verify-1', 'activity', '2026-09-02T01:50:00Z', 'Status changed from Completed to Verified'),
      note('reopen', 'activity', '2026-09-02T02:00:00Z', 'Status changed from Verified to Started'),
      note('done-2', 'activity', '2026-09-02T02:30:00Z', 'Status changed from Started to Completed'),
      {
        ...note('broken', 'activity', '2026-09-02T02:40:00Z', 'Reported as not working'),
        summary: 'Reported as not working',
      },
      note('done-3', 'activity', '2026-09-02T03:00:00Z', 'Status changed from Not Started to Completed'),
      scored('final', '2026-09-02T03:10:00Z', 35),
    ];
    const titles = new Map(
      ticketTimelineEntries(ticket({ status: 'completed', notes: [...notes].reverse() })).map((entry) => [
        entry.id,
        entry.title,
      ]),
    );
    expect(titles.get('done-1')).toBe('Completed · 82% confidence');
    expect(titles.get('verify-1')).toBe('Verified');
    expect(titles.get('done-2')).toBe('Completed');
    expect(titles.get('done-3')).toBe('Completed · 35% confidence');
    expect(titles.get('reopen')).toBe('Started');
    expect(titles.has('late')).toBe(false);
    expect(completionConfidenceByNote([])).toEqual(new Map());
    // Zero is a real score.
    expect(
      completionConfidenceByNote([
        note('d', 'activity', '2026-09-02T01:00:00Z', 'Status changed from Started to Completed'),
        scored('z', '2026-09-02T01:01:00Z', 0),
      ]).get('d'),
    ).toBe(0);
  });

  it('deduplicates a persisted transition but not unrelated activity at the same time', () => {
    const entries = ticketTimelineEntries(
      ticket({
        notes: [
          note('work', 'activity', '2026-09-02T03:00:00Z', 'Finished implementation'),
          note('done', 'activity', '2026-09-02T03:00:00Z', 'Status changed from Started to Completed'),
        ],
      }),
    );
    expect(entries.map((entry) => entry.title)).toEqual(['Ticket created', 'Completed', 'Finished implementation']);
  });

  it('keeps repeated and reversed transitions concise without collapsing history', () => {
    const entries = ticketTimelineEntries(
      ticket({
        completed_at: undefined,
        notes: [
          note('one', 'activity', '2026-09-02T02:00:00Z', 'Status changed from Not Started to Started'),
          note('two', 'activity', '2026-09-02T03:00:00Z', 'Status changed from Started to Completed'),
          note('three', 'activity', '2026-09-02T04:00:00Z', 'Status changed from Completed to Not Started'),
          note('four', 'activity', '2026-09-02T05:00:00Z', 'Status changed from Not Started to Started'),
        ],
      }),
    );
    expect(entries.map((entry) => entry.title)).toEqual([
      'Ticket created',
      'Started',
      'Completed',
      'Re-enqueued',
      'Started',
    ]);
  });

  it('shows each Started phase transition without treating it as a reopen', () => {
    const notes = [
      note('start', 'activity', '2026-09-02T02:00:00Z', 'Status changed from Not Started to Started'),
      note('analyzing', 'activity', '2026-09-02T02:00:00Z', 'Started phase changed from Unspecified to Analyzing'),
      note('planning', 'activity', '2026-09-02T02:01:00Z', 'Started phase changed from Analyzing to Planning'),
      note('clear', 'activity', '2026-09-02T02:02:00Z', 'Started phase changed from Planning to Unspecified'),
    ];
    const entries = ticketTimelineEntries(ticket({ status: 'started', completed_at: undefined, notes }));
    expect(entries.map((entry) => entry.title)).toEqual([
      'Ticket created',
      'Started',
      'Analyzing',
      'Planning',
      'Phase cleared',
    ]);
    expect(entries.slice(2).every((entry) => entry.emphasized)).toBe(true);
    expect(completionConfidenceByNote(notes)).toEqual(new Map());
  });

  it('renders every status destination as a past-tense action and uses the source when reopening', () => {
    const entries = ticketTimelineEntries(
      ticket({
        completed_at: undefined,
        notes: [
          note('backlog', 'activity', '2026-09-02T02:00:00Z', 'Status changed from Started to Backlog'),
          note('unbacklog', 'activity', '2026-09-02T03:00:00Z', 'Status changed from Backlog to Not Started'),
          note('archive', 'activity', '2026-09-02T04:00:00Z', 'Status changed from Verified to Archive'),
          note('delete', 'activity', '2026-09-02T05:00:00Z', 'Status changed from Archive to Deleted'),
          note('move', 'activity', '2026-09-02T06:00:00Z', 'Status changed from Deleted to Moved'),
        ],
      }),
    );
    expect(entries.map((entry) => entry.title)).toEqual([
      'Ticket created',
      'Moved to backlog',
      'Moved out of backlog',
      'Archived',
      'Deleted',
      'Moved',
    ]);
  });

  it('prefers a durable summary and deterministically bounds legacy activity headlines', () => {
    const entries = ticketTimelineEntries(
      ticket({
        notes: [
          {
            ...note(
              'report',
              'activity',
              '2026-09-02T04:00:00Z',
              'Full implementation and verification detail that belongs only in Notes.',
            ),
            summary: 'Resolved the refresh regression',
          },
          note(
            'legacy',
            'activity',
            '2026-09-02T05:00:00Z',
            '## A very long legacy activity headline with enough words that it must be shortened before being presented in the compact timeline index',
          ),
        ],
      }),
    );
    expect(entries.at(-2)).toMatchObject({ title: 'Resolved the refresh regression' });
    expect(entries.at(-2)?.subtitle).toBeUndefined();
    expect(entries.at(-1)?.title).toBe('A very long legacy activity headline with enough words that it must be…');
  });
});
