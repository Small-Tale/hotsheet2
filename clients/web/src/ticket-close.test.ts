import { describe, expect, it } from 'vitest';

import { duplicateReference, duplicateTargetKey, parseDuplicateReference, ticketCloseReasonLabel, validateTicketClose } from './ticket-close';

const source = { id: 'source', slug: 'HS2-SELF', title: 'Same', projectId: 'alpha', projectName: 'Alpha', connectionId: 'git-alpha', nativeId: 'source', qualifiedId: 'git-alpha:source' };
const target = { id: 'target', slug: 'HS2-TARGET', title: 'Canonical', projectId: 'beta', projectName: 'Beta', connectionId: 'git-beta', nativeId: 'target', qualifiedId: 'git-beta:target' };
const sameSlug = { ...target, id: 'collision', projectId: 'gamma', projectName: 'Gamma', connectionId: 'git-gamma', nativeId: 'collision', qualifiedId: 'git-gamma:collision' };

describe('structured ticket close outcomes', () => {
  it('requires a distinct canonical ticket only for duplicates', () => {
    expect(validateTicketClose('completed', source)).toBe('');
    expect(validateTicketClose('duplicate', source)).toContain('Select');
    expect(validateTicketClose('duplicate', source, source)).toContain('itself');
    expect(validateTicketClose('duplicate', source, target)).toBe('');
  });

  it('keeps project, connection, and native identity exact across persistence', () => {
    expect(duplicateTargetKey(target)).toBe('beta::git-beta%3Atarget');
    expect(duplicateReference(target)).toEqual({ project_id: 'beta', connection_id: 'git-beta', native_id: 'target' });
    expect(parseDuplicateReference('@beta/git-beta:ENG:42')).toEqual({ project_id: 'beta', connection_id: 'git-beta', native_id: 'ENG:42' });
    expect(parseDuplicateReference('01ARZ3NDEKTSV4RRFFQ69G5FC0')).toBeUndefined();
  });

  it('distinguishes same-slug tickets and allows the same connection/native in another project', () => {
    expect(sameSlug.slug).toBe(target.slug);
    expect(duplicateTargetKey(sameSlug)).not.toBe(duplicateTargetKey(target));
    expect(validateTicketClose('duplicate', source, { ...source, projectId: 'other', projectName: 'Other' })).toBe('');
  });

  it('presents every persisted close reason in human language', () => {
    expect(['completed', 'not_planned', 'duplicate', 'obsolete'].map(value => ticketCloseReasonLabel(value as never))).toEqual(['Completed', 'Not planned', 'Duplicate', 'Obsolete']);
  });
});
