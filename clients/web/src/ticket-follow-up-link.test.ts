import { describe, expect, it } from 'vitest';

import {
  followUpEvidenceKeys,
  followUpLinkAvailability,
  followUpLinkedMessage,
  followUpQualifiedReference,
  isLinkedFollowUp,
  validateFollowUpLink,
} from './ticket-follow-up-link';

const source = {
  id: 'source',
  slug: 'HS2-SOURCE',
  title: 'Synthetic source',
  projectId: 'alpha-1234',
  projectName: 'Alpha',
  connectionId: 'git-alpha',
  nativeId: '01SOURCE',
  qualifiedId: 'git-alpha:01SOURCE',
};
const target = {
  ...source,
  id: 'target',
  slug: 'HS2-TARGET',
  projectId: 'beta-5678',
  projectName: 'Beta',
  connectionId: 'git-beta',
  nativeId: '01TARGET',
  qualifiedId: 'git-beta:01TARGET',
};

describe('follow-up link logic (HS2-MCGCPQ)', () => {
  it('enables the action only for Hot Sheet Git sources and explains every other provider', () => {
    expect(followUpLinkAvailability({ provider: 'git', name: 'Hot Sheet Git' })).toEqual({ enabled: true });
    expect(followUpLinkAvailability({ provider: 'github', name: 'GitHub Issues' })).toEqual({
      enabled: false,
      disabledReason: 'GitHub Issues tickets cannot record follow-up links. Only Hot Sheet Git tickets can.',
    });
    expect(followUpLinkAvailability({ provider: 'jira', name: 'Jira' }).enabled).toBe(false);
    expect(followUpLinkAvailability(undefined)).toEqual({
      enabled: false,
      disabledReason: 'The ticket source is still loading.',
    });
  });

  it('builds the exact qualified reference the follow-up route accepts', () => {
    expect(followUpQualifiedReference(source)).toBe('@alpha-1234/git-alpha:01SOURCE');
    expect(followUpQualifiedReference(target)).toBe('@beta-5678/git-beta:01TARGET');
  });

  it('requires a target and refuses a self-link', () => {
    expect(validateFollowUpLink(source, undefined)).toBe('Select the existing ticket to link as a follow-up.');
    expect(validateFollowUpLink(source, { ...source })).toBe('A ticket cannot be its own follow-up.');
    expect(validateFollowUpLink(source, target)).toBe('');
    // The same slug in another project is a different ticket.
    expect(validateFollowUpLink(source, { ...target, slug: source.slug })).toBe('');
  });

  it('recognizes an already-recorded follow-up by project, connection, and native id', () => {
    const keys = followUpEvidenceKeys([
      {
        target: { project_id: 'beta-5678', connection_id: 'git-beta', native_id: '01TARGET' },
        state: 'resolved',
      },
      { target: { project_id: 'gamma', connection_id: 'git-gamma', native_id: '01GONE' }, state: 'missing' },
    ]);
    expect(isLinkedFollowUp(keys, target)).toBe(true);
    expect(isLinkedFollowUp(keys, { ...target, projectId: 'other' })).toBe(false);
    expect(isLinkedFollowUp(keys, undefined)).toBe(false);
    expect(followUpEvidenceKeys(undefined)).toEqual([]);
  });

  it('confirms a new link and an idempotent repeat differently', () => {
    expect(followUpLinkedMessage(source, target, false)).toBe('HS2-TARGET is now a follow-up of HS2-SOURCE.');
    expect(followUpLinkedMessage(source, target, true)).toBe('HS2-TARGET was already a follow-up of HS2-SOURCE.');
  });
});
