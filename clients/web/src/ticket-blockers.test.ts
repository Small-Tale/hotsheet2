import { describe, expect, it } from 'vitest';

import {
  blockerAddedMessage,
  blockerAvailability,
  blockerCandidates,
  blockerReference,
  blockerRemovedMessage,
  blockerRows,
  validateBlocker,
  withBlocker,
  withoutBlocker,
} from './ticket-blockers';

const source = {
  id: 'source',
  slug: 'HS2-SOURCE',
  title: 'Synthetic source',
  projectId: 'alpha',
  projectName: 'Alpha',
  connectionId: 'git-alpha',
  nativeId: 'source',
  qualifiedId: 'git-alpha:source',
};
const sibling = { ...source, id: 'one', slug: 'HS2-ONE', nativeId: 'one', qualifiedId: 'git-alpha:one' };
const otherSource = { ...sibling, connectionId: 'gh-alpha', qualifiedId: 'gh-alpha:one' };
const otherProject = { ...sibling, projectId: 'beta', projectName: 'Beta' };

describe('ticket blockers (HS2-CHH28E)', () => {
  it('enables editing only for a loaded, dependency-capable, updatable source and says why otherwise', () => {
    const git = { name: 'Hot Sheet Git', capabilities: { dependencies: true } };
    expect(blockerAvailability(git, true)).toEqual({ enabled: true });
    expect(blockerAvailability(undefined, true)).toEqual({
      enabled: false,
      disabledReason: 'The ticket source is still loading.',
    });
    expect(blockerAvailability({ name: 'GitHub Issues', capabilities: { dependencies: false } }, true)).toEqual({
      enabled: false,
      disabledReason: 'GitHub Issues tickets cannot record blocking dependencies.',
    });
    expect(blockerAvailability(git, false)).toEqual({
      enabled: false,
      disabledReason: 'This ticket cannot be edited.',
    });
  });

  it('projects blocked_by into open, done, and unavailable rows with qualified references', () => {
    const summaries: Record<string, { slug: string; title: string; status?: string }> = {
      a: { slug: 'HS2-A', title: 'Open blocker', status: 'started' },
      b: { slug: 'HS2-B', title: 'Done blocker', status: 'completed' },
      c: { slug: 'HS2-C', title: 'Verified blocker', status: 'verified' },
      d: { slug: 'HS2-D', title: 'Backlog blocker', status: 'backlog' },
    };
    const rows = blockerRows({ blocked_by: ['a', 'b', 'c', 'd', 'gone'], connection_id: 'git-alpha' }, 'alpha', (id) =>
      Object.hasOwn(summaries, id) ? summaries[id] : undefined,
    );
    expect(rows.map((row) => [row.nativeId, row.state, row.slug])).toEqual([
      ['a', 'open', 'HS2-A'],
      ['b', 'done', 'HS2-B'],
      ['c', 'done', 'HS2-C'],
      ['d', 'open', 'HS2-D'],
      ['gone', 'unavailable', undefined],
    ]);
    expect(rows[0]?.reference).toBe('@alpha/git-alpha:a');
    expect(blockerReference('alpha', 'git-alpha', 'x')).toBe('@alpha/git-alpha:x');
    expect(blockerRows({ blocked_by: [], connection_id: 'git-alpha' }, 'alpha', () => undefined)).toEqual([]);
  });

  it('validates a choice: none, self, another source or project, and an existing blocker', () => {
    expect(validateBlocker(source, undefined, [])).toBe('Select the ticket that blocks HS2-SOURCE.');
    expect(validateBlocker(source, source, [])).toBe('A ticket cannot block itself.');
    expect(validateBlocker(source, otherSource, [])).toBe(
      'Only tickets from the same ticket source can block HS2-SOURCE.',
    );
    expect(validateBlocker(source, otherProject, [])).toBe(
      'Only tickets from the same ticket source can block HS2-SOURCE.',
    );
    expect(validateBlocker(source, sibling, ['one'])).toBe('HS2-ONE already blocks HS2-SOURCE.');
    expect(validateBlocker(source, sibling, ['two'])).toBe('');
  });

  it('offers only same-source candidates other than the ticket itself', () => {
    expect(blockerCandidates(source, [source, sibling, otherSource, otherProject])).toEqual([sibling]);
  });

  it('adds once, removes, and never mutates the input set', () => {
    const current = ['a', 'b'];
    expect(withBlocker(current, 'c')).toEqual(['a', 'b', 'c']);
    expect(withBlocker(current, 'a')).toEqual(['a', 'b']);
    expect(withoutBlocker(current, 'a')).toEqual(['b']);
    expect(withoutBlocker(current, 'missing')).toEqual(['a', 'b']);
    expect(current).toEqual(['a', 'b']);
  });

  it('confirms additions and removals in plain words', () => {
    expect(blockerAddedMessage(source, sibling)).toBe('HS2-ONE now blocks HS2-SOURCE.');
    expect(blockerRemovedMessage('HS2-SOURCE', 'HS2-ONE')).toBe('HS2-ONE no longer blocks HS2-SOURCE.');
  });
});
