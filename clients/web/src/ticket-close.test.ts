import { describe, expect, it } from 'vitest';

import { duplicateOutcomeLabel, duplicateReference, duplicateTargetKey, parseDuplicateReference, resolveDuplicateReferenceTarget, ticketCloseReasonLabel, validateTicketClose } from './ticket-close';

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

  it('resolves readable duplicate targets even when they are absent from loaded ticket pages', async () => {
    const projects = [{ id: 'alpha', name: 'Alpha' }, { id: 'beta', name: 'Beta' }];
    const load = async (project: (typeof projects)[number], id: string) => {
      if (project.id !== 'beta' || id !== 'git-beta:target') throw new Error('not found');
      return { id: 'target', slug: 'HS2-TARGET', title: 'Canonical', connection_id: 'git-beta', native_id: 'target', qualified_id: 'git-beta:target' };
    };
    const resolved = await resolveDuplicateReferenceTarget('@beta/git-beta:target', projects, load);
    expect(resolved).toMatchObject(target);
    expect(duplicateOutcomeLabel(resolved!, 'alpha')).toBe('HS2-TARGET · Beta');
    expect(duplicateOutcomeLabel(resolved!, 'beta')).toBe('HS2-TARGET');
    await expect(resolveDuplicateReferenceTarget('@missing/git:target', projects, load)).resolves.toBeUndefined();
  });

  it('only resolves a legacy internal id when exactly one open project owns it', async () => {
    const projects = [{ id: 'alpha', name: 'Alpha' }, { id: 'beta', name: 'Beta' }];
    const ticket = { id: 'legacy', slug: 'HS2-LEGACY', title: 'Legacy target', connection_id: 'git', native_id: 'legacy', qualified_id: 'git:legacy' };
    const unique = await resolveDuplicateReferenceTarget('legacy', projects, async project => {
      if (project.id === 'alpha') return ticket;
      throw new Error('not found');
    });
    expect(unique?.slug).toBe('HS2-LEGACY');
    await expect(resolveDuplicateReferenceTarget('legacy', projects, async () => ticket)).resolves.toBeUndefined();
  });
});
