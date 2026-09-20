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

  it('detects only the single-digit legacy HS1 exception among one-character suffixes (HS2-T9TVYT)', () => {
    expect(parseTicketLinkReference('HS-1')).toEqual({ raw: 'HS-1', slug: 'HS-1' });
    expect(parseTicketLinkReference('@beta-02/HS-9')).toEqual({ raw: '@beta-02/HS-9', projectId: 'beta-02', slug: 'HS-9' });
    expect('See HS-1, HS-9, AB-1, HS-A, and HS-10.'.match(ticketReferencePattern())).toEqual(['HS-1', 'HS-9', 'HS-10']);
    expect(parseTicketLinkReference('AB-1')).toBeUndefined();
    expect(parseTicketLinkReference('HS-A')).toBeUndefined();
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

  it('resolves a bare legacy HS-N reference to the imported ticket by legacy_number (HS2-XB5R3Y)', () => {
    const imported = { ...ticket('HS2-IMPORTED1', 'git'), legacy_number: 'HS-1234' };
    const withLegacy = [{ ...projects[0], tickets: [...projects[0].tickets, imported] }, projects[1]];
    const reference = parseTicketLinkReference('HS-1234')!;
    // Detection already handles HS-N (it matches the shared slug pattern).
    expect(reference).toEqual({ raw: 'HS-1234', slug: 'HS-1234' });
    expect(resolveTicketLink(reference, withLegacy, 'alpha-01')).toMatchObject({
      kind: 'open',
      match: { projectId: 'alpha-01', slug: 'HS2-IMPORTED1', qualifiedId: 'git:HS2-IMPORTED1' },
    });
    // The retained number matches case-insensitively even if stored in a different case.
    const lowerStored = [{ ...projects[0], tickets: [{ ...ticket('HS2-IMPORTED2', 'git'), legacy_number: 'hs-777' }] }];
    expect(resolveTicketLink(parseTicketLinkReference('HS-777')!, lowerStored, 'alpha-01').kind).toBe('open');
    // A legacy number no ticket carries still reports not-found.
    expect(resolveTicketLink(parseTicketLinkReference('HS-9999')!, withLegacy, 'alpha-01').kind).toBe('not_found');
  });

  it('resolves a single-digit legacy HS-N reference to the imported ticket (HS2-T9TVYT)', () => {
    const imported = { ...ticket('HS2-IMPORTED1', 'git'), legacy_number: 'HS-7' };
    const withLegacy = [{ ...projects[0], tickets: [...projects[0].tickets, imported] }];
    expect(resolveTicketLink(parseTicketLinkReference('HS-7')!, withLegacy, 'alpha-01')).toMatchObject({
      kind: 'open',
      match: { slug: 'HS2-IMPORTED1' },
    });
  });

  it('offers the ambiguity chooser when a legacy number matches multiple imported tickets (HS2-XB5R3Y)', () => {
    const importedA = { ...ticket('HS2-IMPA', 'git'), legacy_number: 'HS-42' };
    const importedB = { ...ticket('HS2-IMPB', 'git'), legacy_number: 'HS-42' };
    const withLegacy = [
      { ...projects[0], tickets: [...projects[0].tickets, importedA] },
      { ...projects[1], tickets: [...projects[1].tickets, importedB] },
    ];
    const result = resolveTicketLink(parseTicketLinkReference('HS-42')!, withLegacy, 'alpha-01');
    expect(result.kind).toBe('choose');
    if (result.kind !== 'choose') throw new Error('Expected multiple legacy matches.');
    expect(result.matches.map(match => match.slug)).toEqual(['HS2-IMPA', 'HS2-IMPB']);
  });

  it('uses an explicit project ID to resolve a cross-project reference', () => {
    const reference = parseTicketLinkReference('@beta-02/HS2-SHARED1')!;
    expect(resolveTicketLink(reference, projects, 'alpha-01')).toMatchObject({
      kind: 'open',
      match: { projectId: 'beta-02', projectName: 'Beta', connectionId: 'github' },
    });
  });
});
