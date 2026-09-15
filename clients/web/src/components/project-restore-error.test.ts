import { describe, expect, it } from 'vitest';

import { ProjectRestoreError, projectRestoreTabId, rememberedProjectName } from './project-restore-error';

describe('ProjectRestoreError', () => {
  it('derives a stable human name and distinct tab identity from a remembered root', () => {
    expect(rememberedProjectName('/work/older-server/')).toBe('older-server');
    expect(rememberedProjectName('C:\\work\\older-server\\')).toBe('older-server');
    expect(projectRestoreTabId('/work/older-server')).toBe('project-restore-error:/work/older-server');
  });

  it('keeps the exact failure, recovery context, root, and retry action visible', () => {
    const markup = String(ProjectRestoreError({
      root: '/work/older-server',
      name: 'older-server',
      error: 'The older server only supports schema 2.',
      recoveryPid: 4242,
    }));
    expect(markup).toContain('older-server could not be reopened');
    expect(markup).toContain('The older server only supports schema 2.');
    expect(markup).toContain('process (4242) is not responding');
    expect(markup).toContain('Project: /work/older-server');
    expect(markup).toContain('data-action="retry-project-restore"');
    expect(markup).toContain('data-project-root="/work/older-server"');
  });
});
