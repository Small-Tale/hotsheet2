import { describe, expect, it } from 'vitest';

import {
  collectMatchingSearchPages,
  filterAdvancedSearchResults,
  matchesSearchExpression,
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
