import { describe, expect, it } from 'vitest';

import type { TicketRow } from './api';
import {
  formatTicketLinkReference,
  parseTicketLinkReference,
  resolveTicketLink,
  ticketLinkMatchKey,
  ticketReferencePattern,
} from './ticket-link-resolution';

const ticket = (slug: string, connectionId: string, title = `${slug} title`): TicketRow => ({
  connection_id: connectionId,
  native_id: slug,
  qualified_id: `${connectionId}:${slug}`,
  id: `${connectionId}-${slug}`,
  slug,
  title,
  status: 'started',
  up_next: false,
  feedback_needed: false,
  tags: [],
  blocked_by: [],
  claim_count: 0,
});

const projects = [
  { id: 'alpha-01', name: 'Alpha', tickets: [ticket('HS2-SHARED1', 'git'), ticket('HS2-LOCAL1', 'git')] },
  { id: 'beta-02', name: 'Beta', tickets: [ticket('HS2-SHARED1', 'github'), ticket('HS2-REMOTE1', 'git')] },
];

describe('ticket link resolution', () => {
  it('parses local references and the explicit cross-project syntax', () => {
    expect(parseTicketLinkReference('HS2-LOCAL1')).toEqual({ raw: 'HS2-LOCAL1', slug: 'HS2-LOCAL1' });
    expect(parseTicketLinkReference('@beta-02/HS2-REMOTE1')).toEqual({ raw: '@beta-02/HS2-REMOTE1', projectId: 'beta-02', slug: 'HS2-REMOTE1' });
    expect(formatTicketLinkReference('HS2-REMOTE1', 'beta-02')).toBe('@beta-02/HS2-REMOTE1');
    expect(parseTicketLinkReference('@Beta project/HS2-REMOTE1')).toBeUndefined();
    expect(() => formatTicketLinkReference('not a slug', 'beta-02')).toThrow();
    expect('See HS2-LOCAL1 and @beta-02/HS2-REMOTE1.'.match(ticketReferencePattern())).toEqual(['HS2-LOCAL1', '@beta-02/HS2-REMOTE1']);
  });

  it('opens one exact match across every open project', () => {
    const reference = parseTicketLinkReference('HS2-REMOTE1')!;
    expect(resolveTicketLink(reference, projects, 'alpha-01')).toMatchObject({
      kind: 'open',
      match: { projectId: 'beta-02', qualifiedId: 'git:HS2-REMOTE1' },
    });
  });

  it('returns a not-found outcome instead of falling through to advanced search', () => {
    const reference = parseTicketLinkReference('HS2-MISSING')!;
    expect(resolveTicketLink(reference, projects, 'alpha-01')).toEqual({
      kind: 'not_found',
      reference,
      message: 'No exact match for HS2-MISSING.',
    });
    const crossProject = parseTicketLinkReference('@missing-project/HS2-MISSING')!;
    expect(resolveTicketLink(crossProject, projects, 'alpha-01')).toMatchObject({
      kind: 'not_found',
      message: 'No exact match for HS2-MISSING in project missing-project.',
    });
  });

  it('returns every distinct exact source match for a simple choice dialog', () => {
    const duplicate = ticket('HS2-SHARED1', 'jira', 'Shared from Jira');
    const withDuplicates = [{ ...projects[0], tickets: [...projects[0].tickets, duplicate, duplicate] }, projects[1]];
    const result = resolveTicketLink(parseTicketLinkReference('HS2-SHARED1')!, withDuplicates, 'alpha-01');
    expect(result.kind).toBe('choose');
    if (result.kind !== 'choose') throw new Error('Expected multiple exact matches.');
    expect(result.matches.map(match => match.connectionId)).toEqual(['git', 'jira', 'github']);
    expect(result.matches.map(ticketLinkMatchKey)).toEqual(['alpha-01::git%3AHS2-SHARED1', 'alpha-01::jira%3AHS2-SHARED1', 'beta-02::github%3AHS2-SHARED1']);
  });

  it('uses an explicit project ID to resolve a cross-project reference', () => {
    const reference = parseTicketLinkReference('@beta-02/HS2-SHARED1')!;
    expect(resolveTicketLink(reference, projects, 'alpha-01')).toMatchObject({
      kind: 'open',
      match: { projectId: 'beta-02', projectName: 'Beta', connectionId: 'github' },
    });
  });
});
