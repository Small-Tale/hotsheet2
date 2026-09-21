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
    const markup = String(ProjectDialog({ open: true, root: '/work/demo', error: 'Unavailable', recovery: { expected: { pid: 42 } } }));
    expect(markup).toContain('value="/work/demo"');
    expect(markup).toContain('Unavailable');
    expect(markup).toContain('process 42');
    expect(markup).toContain('data-action="recover-unhealthy-server"');
  });

  it('renders server-known checkouts and owns their styles', () => {
    const markup = String(RemoteProjectDialog({ open: true, checkouts: [{ id: 'demo', root: '/work/demo', alias: 'Demo', stores: [] }] }));
    const css = readFileSync(resolve(import.meta.dirname, 'project-dialog.css'), 'utf8');
    expect(markup).toContain('data-checkout-root="/work/demo"');
    expect(markup).toContain('>Demo</span>');
    expect(css).toContain('.remote-project-dialog__item');
  });
});
