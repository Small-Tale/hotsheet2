import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { ProjectDialog, projectDialogRoot, RemoteProjectDialog } from './project-dialog';

describe('project dialogs', () => {
  it('uses the selected project root and a portable first-run fallback', () => {
    expect(projectDialogRoot({ root: '/work/current-project' })).toBe('/work/current-project');
    expect(projectDialogRoot()).toBe('.');
  });

  it('renders open-project input, errors, and exact recovery process context', () => {
    const markup = String(
      ProjectDialog({ open: true, root: '/work/demo', error: 'Unavailable', recovery: { expected: { pid: 42 } } }),
    );
    expect(markup).toContain('value="/work/demo"');
    expect(markup).toContain('Unavailable');
    expect(markup).toContain('process 42');
    expect(markup).toContain('data-action="recover-unhealthy-server"');
  });

  it('renders server-known checkouts and owns their styles', () => {
    const markup = String(
      RemoteProjectDialog({ open: true, checkouts: [{ id: 'demo', root: '/work/demo', alias: 'Demo', stores: [] }] }),
    );
    const css = readFileSync(resolve(import.meta.dirname, 'project-dialog.css'), 'utf8');
    expect(markup).toContain('data-checkout-root="/work/demo"');
    expect(markup).toContain('>Demo</span>');
    expect(css).toContain('.remote-project-dialog__item');
  });

  it('uses canonical dialog, field-group, and connected-item spacing', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'project-dialog.css'), 'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(css).toMatch(/\.project-dialog \{[^}]*gap: var\(--kui-space-m\)/);
    expect(css).toMatch(/\.project-dialog__path \{[^}]*gap: var\(--kui-space-xs\)/);
    expect(css).toMatch(/\.project-dialog footer \{[^}]*gap: var\(--kui-space-xs\)/);
    expect(css).toMatch(/\.remote-project-dialog__list \{[^}]*gap: var\(--kui-space-2xs\)/);
    expect(css).toMatch(
      /\.remote-project-dialog__item \{[^}]*gap: var\(--kui-space-2xs\);[^}]*padding: var\(--kui-space-xs\) var\(--kui-space-m\)/,
    );
    expect(css).toMatchSource(
      /\.project-dialog__server-recovery \{[^}]*gap:var\(--kui-space-xs\); padding:var\(--kui-space-m\)/,
    );
  });
});
