import { describe, expect, it } from 'vitest';

import {
  clusterTicketsByGroup,
  existingTicketGroup,
  isTicketGroupId,
  normalizeTicketGroupName,
  TICKET_GROUP_NAME_LIMIT,
  ticketGroupCollapseKey,
  ticketGroupDisplayName,
  ticketGroupDomId,
  ticketGroupKey,
  ticketGroupNameError,
  ticketGroupNameKey,
  ticketGroupNameProblem,
  ticketGroupSegments,
  ticketGroupSummaries,
  ticketGroupValue,
  withoutCollapsedGroupMembers,
} from './ticket-groups';

const row = (slug: string, group?: string | null) => ({ slug, group });
const slugs = (rows: readonly { slug: string }[]) => rows.map((item) => item.slug);

describe('ticket group names (HS2-PDSHNN)', () => {
  it('trims, collapses inner whitespace, and compares case-insensitively', () => {
    expect(normalizeTicketGroupName('  Board   drag &\tdrop ')).toBe('Board drag & drop');
    expect(ticketGroupKey(' Board  DRAG & drop')).toBe('board drag & drop');
    expect(ticketGroupValue({ group: '  ' })).toBeUndefined();
    expect(ticketGroupValue({ group: null })).toBeUndefined();
    expect(ticketGroupValue({})).toBeUndefined();
    expect(ticketGroupValue({ group: ' Übersicht ' })).toBe('Übersicht');
  });

  it('finds an existing group in its stored spelling', () => {
    expect(existingTicketGroup('search  SYNTAX', ['Board', 'Search syntax'])).toBe('Search syntax');
    expect(existingTicketGroup('other', ['Board'])).toBeUndefined();
    expect(existingTicketGroup('  ', ['Board'])).toBeUndefined();
  });

  it('names each problem a new or renamed group can have', () => {
    const groups = ['Board', 'Search syntax'];
    expect(ticketGroupNameProblem('   ', groups)).toBe('empty');
    expect(ticketGroupNameProblem('board', groups)).toBe('duplicate');
    expect(ticketGroupNameProblem('a\u0007b', groups)).toBe('invalid');
    expect(ticketGroupNameProblem('x'.repeat(TICKET_GROUP_NAME_LIMIT + 1), groups)).toBe('too-long');
    expect(ticketGroupNameProblem('x'.repeat(TICKET_GROUP_NAME_LIMIT), groups)).toBeUndefined();
    expect(ticketGroupNameProblem('Polish', groups)).toBeUndefined();
    // Renaming a group to another casing of its own name is allowed; another group's name is not.
    expect(ticketGroupNameProblem('BOARD', groups, 'Board')).toBeUndefined();
    expect(ticketGroupNameProblem('search syntax', groups, 'Board')).toBe('duplicate');
  });

  it('words only the problems worth an inline error', () => {
    expect(ticketGroupNameError('duplicate', ' board ')).toBe('A group named “board” already exists.');
    expect(ticketGroupNameError('too-long', 'x')).toContain(String(TICKET_GROUP_NAME_LIMIT));
    expect(ticketGroupNameError('invalid', 'x')).toBe('Use a single line of text.');
    expect(ticketGroupNameError('empty', '')).toBeUndefined();
    expect(ticketGroupNameError(undefined, 'x')).toBeUndefined();
  });

  it('summarizes groups with counts, merged by key and sorted by name', () => {
    expect(
      ticketGroupSummaries([row('a', 'Search'), row('b', 'board'), row('c'), row('d', 'search '), row('e', null)]),
    ).toEqual([
      { id: 'board', name: 'board', count: 1 },
      { id: 'Search', name: 'Search', count: 2 },
    ]);
    expect(ticketGroupSummaries([])).toEqual([]);
  });
});

describe('ticket group ordering (HS2-WZ3JGZ)', () => {
  it('places a group where its first member sorts and keeps member order', () => {
    const sorted = [row('a'), row('g1', 'G'), row('b'), row('h1', 'H'), row('g2', 'g'), row('c'), row('h2', 'H')];
    expect(slugs(clusterTicketsByGroup(sorted))).toEqual(['a', 'g1', 'g2', 'b', 'h1', 'h2', 'c']);
  });

  it('returns ungrouped input unchanged in a new array', () => {
    const sorted = [row('a'), row('b')],
      result = clusterTicketsByGroup(sorted);
    expect(result).toEqual(sorted);
    expect(result).not.toBe(sorted);
  });

  it('is stable when applied twice', () => {
    const sorted = [row('g1', 'G'), row('a'), row('g2', 'G')],
      once = clusterTicketsByGroup(sorted);
    expect(clusterTicketsByGroup(once)).toEqual(once);
  });

  it('segments ungrouped runs and groups in visual order', () => {
    const segments = ticketGroupSegments([row('a'), row('g1', 'G'), row('b'), row('g2', 'G'), row('c')]);
    expect(segments.map((segment) => [segment.kind, slugs(segment.items)])).toEqual([
      ['ungrouped', ['a']],
      ['group', ['g1', 'g2']],
      ['ungrouped', ['b', 'c']],
    ]);
    expect(segments[1]).toMatchObject({ name: 'G', key: 'g' });
  });

  it('keeps adjacent groups separate and handles empty and all-grouped input', () => {
    expect(ticketGroupSegments([])).toEqual([]);
    expect(
      ticketGroupSegments([row('g1', 'G'), row('h1', 'H'), row('g2', 'G')]).map((segment) => slugs(segment.items)),
    ).toEqual([['g1', 'g2'], ['h1']]);
  });

  // Transitions a live collection goes through: regroup, empty a group, and refill it.
  it('follows membership changes across repeated projections', () => {
    let rows = [row('a'), row('b', 'G'), row('c'), row('d', 'G')];
    const shape = () => ticketGroupSegments(rows).map((segment) => slugs(segment.items).join(','));
    expect(shape()).toEqual(['a', 'b,d', 'c']);
    rows = rows.map((item) => (item.slug === 'a' ? row('a', 'G') : item));
    expect(shape()).toEqual(['a,b,d', 'c']);
    rows = rows.map((item) => row(item.slug));
    expect(shape()).toEqual(['a,b,c,d']);
    rows = rows.map((item) => (item.slug === 'c' ? row('c', 'H') : item));
    expect(shape()).toEqual(['a,b', 'c', 'd']);
    rows = rows.filter((item) => item.slug !== 'c');
    expect(shape()).toEqual(['a,b,d']);
  });
});

describe('ticket group view state (HS2-WZ3JGZ)', () => {
  it('keys collapse by project, view, and group key', () => {
    expect(ticketGroupCollapseKey('p', 'all', ' Board ')).toBe(ticketGroupCollapseKey('p', 'all', 'board'));
    expect(ticketGroupCollapseKey('p', 'all', 'Board')).not.toBe(ticketGroupCollapseKey('p', 'backlog', 'Board'));
    expect(ticketGroupCollapseKey('p', 'all', 'Board')).not.toBe(ticketGroupCollapseKey('q', 'all', 'Board'));
  });

  it('drops members of collapsed groups only', () => {
    const rows = [row('a'), row('g1', 'G'), row('h1', 'H')];
    expect(slugs(withoutCollapsedGroupMembers(rows, (group) => group === 'G'))).toEqual(['a', 'h1']);
    expect(slugs(withoutCollapsedGroupMembers(rows, () => false))).toEqual(['a', 'g1', 'h1']);
  });

  it('derives one DOM id per scope and group key', () => {
    expect(ticketGroupDomId('list', 'Board')).toBe(ticketGroupDomId('list', ' board'));
    expect(ticketGroupDomId('list', 'Board')).not.toBe(ticketGroupDomId('started', 'Board'));
    expect(ticketGroupDomId('list', 'Board & "x"')).toMatch(/^ticket-group-[a-z0-9]+$/);
  });
});

describe('ticket group ids (HS2-9KZSD4)', () => {
  const SPRINT_A = '7K3M9P2QXA',
    SPRINT_B = '8B4N0R3SYB',
    registry = [
      { id: SPRINT_A, name: 'Sprint', duplicate: true },
      { id: SPRINT_B, name: 'sprint', duplicate: true },
    ];

  it('recognizes only 10-character uppercase Crockford ids', () => {
    expect(isTicketGroupId(SPRINT_A)).toBe(true);
    expect(isTicketGroupId('7k3m9p2qxa')).toBe(false);
    expect(isTicketGroupId('7K3M9P2QXI')).toBe(false);
    expect(isTicketGroupId('7K3M9P2QX')).toBe(false);
    expect(isTicketGroupId('Board')).toBe(false);
  });

  it('keys an id exactly and a legacy name by its name key', () => {
    expect(ticketGroupKey(SPRINT_A)).toBe(SPRINT_A);
    expect(ticketGroupKey(` ${SPRINT_A} `)).toBe(SPRINT_A);
    expect(ticketGroupKey(' Board  Drop')).toBe('board drop');
    expect(ticketGroupKey(SPRINT_A)).not.toBe(ticketGroupKey(SPRINT_B));
    expect(ticketGroupNameKey(' Sprint  One ')).toBe('sprint one');
  });

  it('resolves display names from the registry, a legacy value, or the placeholder', () => {
    expect(ticketGroupDisplayName(SPRINT_A, registry)).toBe('Sprint');
    expect(ticketGroupDisplayName('  Board  drop ', registry)).toBe('Board drop');
    expect(ticketGroupDisplayName('9C5P1T4VZC', registry)).toBe('Group 9C5P1T4VZC');
    expect(ticketGroupDisplayName(SPRINT_A)).toBe(`Group ${SPRINT_A}`);
    // A legacy group listed by the server keeps the server's spelling.
    expect(ticketGroupDisplayName('board', [{ id: 'Board', name: 'Board' }])).toBe('Board');
  });

  it('renders two ids with the same display name as two groups', () => {
    const rows = [
      { slug: 'a', group: SPRINT_A, groupName: 'Sprint' },
      { slug: 'b', group: SPRINT_B, groupName: 'sprint' },
      { slug: 'c', group: SPRINT_A, groupName: 'Sprint' },
    ];
    const segments = ticketGroupSegments(rows);
    expect(segments.map((segment) => [segment.kind, slugs(segment.items)])).toEqual([
      ['group', ['a', 'c']],
      ['group', ['b']],
    ]);
    expect(segments[0]).toMatchObject({ group: SPRINT_A, name: 'Sprint', key: SPRINT_A });
    expect(segments[1]).toMatchObject({ group: SPRINT_B, name: 'sprint', key: SPRINT_B });
    expect(ticketGroupDomId('list', SPRINT_A)).not.toBe(ticketGroupDomId('list', SPRINT_B));
    expect(ticketGroupSummaries(rows, registry)).toEqual([
      { id: SPRINT_A, name: 'Sprint', count: 2 },
      { id: SPRINT_B, name: 'sprint', count: 1 },
    ]);
  });

  it('shows an unknown id under its placeholder and a legacy value under its name', () => {
    const segments = ticketGroupSegments([row('a', '9C5P1T4VZC'), row('b', 'board '), row('c', 'Board')]);
    expect(segments.map((segment) => (segment.kind === 'group' ? [segment.name, segment.key] : []))).toEqual([
      ['Group 9C5P1T4VZC', '9C5P1T4VZC'],
      ['board', 'board'],
    ]);
  });

  it('keys collapse by id, so a renamed id group keeps its collapse key', () => {
    expect(ticketGroupCollapseKey('p', 'all', SPRINT_A)).toBe(JSON.stringify(['p', 'all', SPRINT_A]));
    expect(ticketGroupCollapseKey('p', 'all', SPRINT_A)).not.toBe(ticketGroupCollapseKey('p', 'all', SPRINT_B));
  });
});
