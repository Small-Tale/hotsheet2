import { describe, expect, it } from 'vitest';

import { ProjectRestoreError, projectRestoreTabId, rememberedProjectName } from './project-restore-error';

describe('ProjectRestoreError', () => {
  it('derives a stable human name and distinct tab identity from a remembered root', () => {
    expect(rememberedProjectName('/work/older-server/')).toBe('older-server');
    expect(rememberedProjectName('C:\\work\\older-server\\')).toBe('older-server');
    expect(projectRestoreTabId('/work/older-server')).toBe('project-restore-error:/work/older-server');
  });

  it('explains the cause plainly and keeps the raw failure, recovery context, and root as technical details', () => {
    const markup = String(
      ProjectRestoreError({
        root: '/work/older-server',
        name: 'older-server',
        error: 'The older server only supports schema 2.',
        recoveryPid: 4242,
      }),
    );
    // An unresponsive registered process is a server cause (HS2-ZKAVZZ).
    expect(markup).toContain('data-project-unavailable-cause="server-unreachable"');
    expect(markup).toContain('Server Unavailable');
    expect(markup).toContain('The local Hot Sheet server is not responding, so older-server cannot open.');
    expect(markup).toContain('Technical Details');
    expect(markup).toContain('The older server only supports schema 2.');
    expect(markup).toContain('process 4242 is not responding');
    expect(markup).toContain('Project folder: /work/older-server');
    expect(markup).toContain('data-action="retry-project-restore"');
    expect(markup).toContain('data-project-root="/work/older-server"');
  });

  it('offers Retry and Close Tab, with Close Tab primary when the folder is gone (HS2-ZKAVZZ)', () => {
    const markup = String(
      ProjectRestoreError({
        root: '/work/gone',
        name: 'gone',
        error: "ENOENT: no such file or directory, realpath '/work/gone'",
      }),
    );
    expect(markup).toContain('Project Folder Not Found');
    expect(markup).toContain('The folder for gone was moved, renamed, or deleted.');
    expect(markup).toContain('data-action="close-project-tab"');
    expect(markup).toContain('data-project-id="project-restore-error:/work/gone"');
    expect(markup).toContain('>Retry<');
    expect(markup).toContain('>Close Tab<');
    expect(markup).toMatch(/data-emphasis="secondary"[^>]*data-action="retry-project-restore"/);
    expect(markup).toMatch(/data-emphasis="primary"[^>]*data-action="close-project-tab"/);
  });

  it('puts Technical Details in the EmptyState secondary region, apart from the centered actions (HS2-1ZKMS9)', () => {
    const failure = {
        root: '/work/gone',
        name: 'gone',
        error: "ENOENT: no such file or directory, realpath '/work/gone'",
      },
      markup = String(ProjectRestoreError(failure)),
      [beforeSecondary, secondary] = markup.split('class="kui-empty-state__secondary"');
    // The disclosure is the secondary region's content, never part of the action zone (Kerf KF-GZDZXH).
    expect(secondary).toContain('data-project-unavailable-details');
    expect(secondary).toContain('Technical Details');
    expect(beforeSecondary).not.toContain('data-project-unavailable-details');
    expect(beforeSecondary.split('class="kui-empty-state__action"')[1]).toContain('data-action="close-project-tab"');
    // Collapsed by default; `detailsOpen` opens it initially.
    expect(markup).not.toMatch(/<details[^>]*\bopen\b/);
    expect(String(ProjectRestoreError({ ...failure, detailsOpen: true }))).toMatch(/<details[^>]*\bopen\b/);
  });

  it('disables Retry and marks the state busy while a retry runs', () => {
    const markup = String(ProjectRestoreError({ root: '/work/x', name: 'x', error: 'Failed to fetch', busy: true }));
    expect(markup).toContain('Retrying…');
  });
});
