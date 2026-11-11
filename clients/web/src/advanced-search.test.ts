import { describe, expect, it } from 'vitest';

import {
  collectMatchingSearchPages,
  filterAdvancedSearchResults,
  matchesSearchExpression,
  structuredSearchTerms,
  usesAdvancedSearchExpression,
  usesBooleanSearchExpression,
} from './advanced-search';
import type { TicketRow } from './api';

const row = (slug: string, status = 'not_started', extra: Partial<TicketRow> = {}): TicketRow => ({
  connection_id: 'local',
  native_id: slug,
  qualified_id: `local:${slug}`,
  id: slug,
  slug,
  title: slug,
  status,
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
  ...extra,
});
describe('advanced search semantics', () => {
  it('leaves ordinary text to provider search and filters inline expressions locally', () => {
    const rows = [row('HS2-ONE'), row('HS2-HIDDEN', 'archive')];
    expect(filterAdvancedSearchResults(rows, 'words')).toEqual(rows);
    expect(filterAdvancedSearchResults(rows, 'is:archived')).toEqual([rows[1]]);
  });
  it('supports every is: lifecycle alias and live Up Next/active state', () => {
    const now = Date.parse('2026-09-09T10:00:00Z'),
      active = row('HS2-ACTIVE', 'started', {
        up_next: true,
        claimed_by: 'worker',
        claim_lease_expires_at: '2026-09-09T10:05:00Z',
      }),
      rows = [
        row('HS2-NEW'),
        active,
        row('HS2-DONE', 'completed'),
        row('HS2-VERIFIED', 'verified'),
        row('HS2-BACKLOG', 'backlog'),
        row('HS2-ARCHIVE', 'archive'),
        row('HS2-DUPLICATE', 'completed', { close_reason: 'duplicate', duplicate_of: 'git:2' }),
      ];
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:up-next', now))).toEqual([active]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:active', now))).toEqual([active]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:open', now))).toEqual(rows.slice(0, 2));
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:closed', now))).toEqual([
      rows[2],
      rows[3],
      rows[5],
      rows[6],
    ]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:duplicate', now))).toEqual([rows[6]]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:not-started', now))).toEqual([rows[0]]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:backlogged', now))).toEqual([rows[4]]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:archived', now))).toEqual([rows[5]]);
    expect(filterAdvancedSearchResults(rows, 'is:archived')).toEqual([rows[5]]);
  });
  it('HS2-2GCMY1 matches is:blocked by a non-empty blocked reason, alone and in Boolean expressions', () => {
    const blocked = row('HS2-BLOCKED', 'started', { blocked_reason: 'Waits on the vendor' });
    const blank = row('HS2-BLANK', 'not_started', { blocked_reason: '   ' });
    const clear = row('HS2-CLEAR', 'not_started');
    const rows = [blocked, blank, clear];
    expect(usesAdvancedSearchExpression('is:blocked')).toBe(true);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:blocked'))).toEqual([blocked]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'IS:BLOCKED'))).toEqual([blocked]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'NOT is:blocked'))).toEqual([blank, clear]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:blocked AND is:started'))).toEqual([blocked]);
    expect(filterAdvancedSearchResults(rows, 'is:blocked')).toEqual([blocked]);
  });
  it('HS2-18QFJ5 matches only recorded AI creators, including case-insensitive and NOT expressions', () => {
    const ai = row('HS2-AI', 'not_started', { created_by: { role: 'ai' } }),
      human = row('HS2-HUMAN', 'not_started', { created_by: { role: 'human' } }),
      unknown = row('HS2-UNKNOWN'),
      rows = [ai, human, unknown];
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'IS:AI-CREATED'))).toEqual([ai]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'NOT is:ai-created'))).toEqual([human, unknown]);
    expect(filterAdvancedSearchResults(rows, 'is:ai-created AND is:not-started')).toEqual([ai]);
  });
  it('HS2-C7917F matches is:closed by a terminal status or a recorded close outcome on any status', () => {
    const notPlanned = row('HS2-NOT-PLANNED', 'backlog', { close_reason: 'not_planned' }),
      obsolete = row('HS2-OBSOLETE', 'backlog', { close_reason: 'obsolete' }),
      backlog = row('HS2-BACKLOG', 'backlog'),
      done = row('HS2-DONE', 'completed'),
      open = row('HS2-OPEN', 'started'),
      rows = [notPlanned, obsolete, backlog, done, open];
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:closed'))).toEqual([notPlanned, obsolete, done]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'NOT is:closed'))).toEqual([backlog, open]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:closed AND is:backlog'))).toEqual([
      notPlanned,
      obsolete,
    ]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:open'))).toEqual([open]);
    expect(filterAdvancedSearchResults(rows, 'is:closed')).toEqual([notPlanned, obsolete, done]);
  });
  it('matches review state in Boolean expressions and clears resolved feedback', () => {
    const feedback = row('HS2-FEEDBACK', 'started', { feedback_needed: true, needs_review: true });
    const requested = row('HS2-REQUEST', 'started', { needs_review: true });
    const resolved = row('HS2-RESOLVED', 'started', { feedback_needed: false, needs_review: false });
    const rows = [feedback, requested, resolved];
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:needs-review'))).toEqual([feedback, requested]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'is:needs-review AND NOT HS2-FEEDBACK'))).toEqual([
      requested,
    ]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'NOT is:needs-review'))).toEqual([resolved]);
  });
  it('parses boolean precedence, negation, parentheses, phrases, and implicit AND', () => {
    const rows = [
      row('HS2-ONE', 'started', { title: 'Parser repair', tags: ['client'] }),
      row('HS2-TWO', 'completed', { title: 'Parser docs', tags: ['docs'] }),
      row('HS2-THREE', 'backlog', { title: 'Unrelated', tags: ['client'] }),
    ];
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'parser AND NOT is:completed'))).toEqual([rows[0]]);
    expect(
      rows.filter((ticket) => matchesSearchExpression(ticket, '(tag-does-not-exist OR client) AND is:open')),
    ).toEqual([rows[0]]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, '"Parser docs" OR is:backlog'))).toEqual([
      rows[1],
      rows[2],
    ]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'parser is:completed'))).toEqual([rows[1]]);
    expect(rows.filter((ticket) => matchesSearchExpression(ticket, 'NOT tag:client AND parser'))).toEqual([rows[1]]);
    expect(usesAdvancedSearchExpression('ordinary words')).toBe(false);
    expect(usesAdvancedSearchExpression('is:duplicate')).toBe(true);
    expect(usesBooleanSearchExpression('is:duplicate')).toBe(false);
    expect(usesBooleanSearchExpression('NOT (is:archived OR is:backlog)')).toBe(true);
  });
  it('safely rejects incomplete groups and unknown lifecycle aliases', () => {
    const ticket = row('HS2-ONE', 'started', { title: 'Parser repair' });
    expect(matchesSearchExpression(ticket, '(parser')).toBe(false);
    expect(matchesSearchExpression(ticket, 'is:unknown')).toBe(false);
  });
  it('keeps quoted structured values intact and resolves Boolean membership before NOT', () => {
    const first = row('HS2-ONE', 'started', { tags: ['client'] }),
      second = row('HS2-TWO', 'started', { tags: ['docs'] }),
      query = 'attachment:"annual report*.png" OR (tag:docs AND NOT has:attachment)',
      terms = structuredSearchTerms(query);
    expect(terms.map((term) => term.raw)).toEqual(['attachment:"annual report*.png"', 'has:attachment']);
    const resolve = (ticket: TicketRow, term: { raw: string }) =>
      ticket === first && term.raw === 'attachment:"annual report*.png"';
    expect([first, second].filter((ticket) => matchesSearchExpression(ticket, query, 0, resolve))).toEqual([
      first,
      second,
    ]);
    expect(structuredSearchTerms('NOT (attachment:"unfinished')).toEqual([]);
  });
  it('collects matches beyond the first page and rejects repeated cursors', async () => {
    const pages: Record<string, { items: number[]; next_cursor?: string }> = {
      second: { items: [201, 202], next_cursor: 'third' },
      third: { items: [301] },
    };
    const load = async (cursor: string) => pages[cursor];
    await expect(
      collectMatchingSearchPages({ items: [1, 2], next_cursor: 'second' }, load, (value) => value > 200),
    ).resolves.toEqual([201, 202, 301]);
    await expect(
      collectMatchingSearchPages(
        { items: [], next_cursor: 'same' },
        async () => ({ items: [], next_cursor: 'same' }),
        () => true,
      ),
    ).rejects.toThrow('repeated cursor');
  });
  it('abandons paginated results when the active search changes', async () => {
    let active = true;
    await expect(
      collectMatchingSearchPages(
        { items: [1], next_cursor: 'next' },
        async () => {
          active = false;
          return { items: [2] };
        },
        () => true,
        () => active,
      ),
    ).resolves.toBeUndefined();
  });
});

describe('group search terms (HS2-44W3TP)', () => {
  const rows = [
    row('HS2-A', 'not_started', { group: 'Board drag & drop' }),
    row('HS2-B', 'started', { group: 'Search syntax', tags: ['client'] }),
    row('HS2-C', 'started'),
    row('HS2-D', 'started', { group: null }),
  ];
  const matching = (query: string) =>
    rows.filter((item) => matchesSearchExpression(item, query)).map((item) => item.slug);

  it('matches group: case-insensitively, quoted or bare, by normalized name', () => {
    expect(matching('group:"board  DRAG & drop"')).toEqual(['HS2-A']);
    expect(matching('group:"Search syntax" AND tag:client')).toEqual(['HS2-B']);
    expect(matching('group:search')).toEqual([]);
    expect(matching('group:unknown OR group:"search syntax"')).toEqual(['HS2-B']);
  });

  it('reads an escaped quote inside a quoted group name', () => {
    const quoted = row('HS2-Q', 'started', { group: 'The "big" one' });
    expect(matchesSearchExpression(quoted, 'group:"The \\"big\\" one" AND is:started')).toBe(true);
    expect(matchesSearchExpression(quoted, 'group:"The big one" AND is:started')).toBe(false);
  });

  it('finds grouped and ungrouped tickets with has:group and NOT', () => {
    expect(matching('has:group AND is:open')).toEqual(['HS2-A', 'HS2-B']);
    expect(matching('NOT has:group')).toEqual(['HS2-C', 'HS2-D']);
    expect(matching('NOT group:"search syntax" AND is:started')).toEqual(['HS2-C', 'HS2-D']);
    expect(matching('(group:"Search syntax" OR NOT has:group) AND is:started')).toEqual(['HS2-B', 'HS2-C', 'HS2-D']);
  });

  it('evaluates group terms on the row, never through the provider resolver', () => {
    expect(structuredSearchTerms('NOT has:group AND has:commit AND group:x').map((term) => term.raw)).toEqual([
      'has:commit',
    ]);
    const resolve = () => {
      throw new Error('group terms need no lookup');
    };
    expect(matchesSearchExpression(rows[0], 'has:group AND group:"board drag & drop"', Date.now(), resolve)).toBe(true);
  });
});
