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
    expect(markup).toContain('opens its existing ticket sources automatically');
    expect(markup).toContain('name="project-root"');
    expect(markup).not.toContain('name="ticket-store"');
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
    expect(markup).toContain('remote-project-dialog__path"');
    expect(markup).toContain('data-component="list-item"');
    expect(markup).toContain('data-multiline="true"');
    expect(markup).toContain('data-action="open-remote-checkout"');
    expect(markup).toContain('data-item-id="demo"');
    expect(markup).toContain('aria-label="Open Projects"');
    // Kerf sizes the dialog and scrolls the checkout list; no app max-height remains (HS2-W4RNKJ).
    expect(markup).toContain('data-component="dialog-surface"');
    expect(markup).toMatch(/role="navigation"[^>]*data-scrollable="true"|data-scrollable="true"[^>]*role="navigation"/);
    expect(css).not.toContain('.remote-project-dialog__list');
    expect(css).not.toContain('.remote-project-dialog__copy');
    expect(css).not.toContain('.remote-project-dialog__item');
  });

  it('preserves full paths, escapes checkout metadata, and falls back to the folder name', () => {
    const root = `/work/${'long-folder-'.repeat(30)}checkout`;
    const markup = String(
      RemoteProjectDialog({
        open: true,
        checkouts: [
          { id: 'long', root, alias: '', stores: [] },
          { id: 'root', root: '/', alias: '', stores: [] },
          { id: 'quoted', root: '/work/"quoted"', alias: '<script>oops</script>', stores: [] },
        ],
      }),
    );
    expect(markup).toContain(`data-checkout-root="${root}"`);
    expect(markup).toContain(`>${root.split('/').pop()}</span>`);
    expect(markup).toContain(`>${root}</span>`);
    expect(markup).toContain('&lt;script&gt;oops&lt;/script&gt;');
    expect(markup).not.toContain('<script>');
    expect(markup).toContain('data-checkout-root="/work/&quot;quoted&quot;"');
  });

  it('renders loading, error, empty, and populated lists without stale rows', () => {
    const checkouts = [{ id: 'demo', root: '/work/demo', alias: '', stores: [] }];
    for (const state of [
      { checkouts, loading: true, copy: 'Loading projects…' },
      { checkouts, error: 'Unavailable', copy: 'Unavailable' },
      { checkouts: [], copy: 'No projects are open on the server yet.' },
    ]) {
      const markup = String(RemoteProjectDialog({ open: true, ...state }));
      expect(markup).toContain(state.copy);
      expect(markup).not.toContain('data-action="open-remote-checkout"');
    }
    expect(String(RemoteProjectDialog({ open: true, checkouts }))).toContain('data-action="open-remote-checkout"');
  });

  it('uses canonical dialog, field-group, and connected-item spacing', () => {
    const localMarkup = String(ProjectDialog({ open: true, root: '.', error: '' }));
    const remoteMarkup = String(RemoteProjectDialog({ open: true, checkouts: [] }));
    const css = readFileSync(resolve(import.meta.dirname, 'project-dialog.css'), 'utf8');
    expect(css).not.toContain('--wa-space-');
    expect(localMarkup).toContain('data-component="list"');
    expect(localMarkup).toContain('data-gap="true"');
    expect(localMarkup).toContain('data-component="row"');
    expect(localMarkup).toContain('data-h-align="right"');
    expect(remoteMarkup).toContain('data-component="list"');
    expect(remoteMarkup).toContain('data-gap="true"');
    expect(css).toMatch(/\.project-dialog__path \{[^}]*gap: var\(--kui-space-xs\)/);
    expect(css).not.toContain('--kui-layout-inline-margin:');
    // Kerf owns the dialog width, error tone, recovery banner, and checkout rows (HS2-FKPQW2).
    expect([...css.matchAll(/^[.\w@][^{\n]*\{/gm)].map((match) => match[0].trim())).toEqual([
      '.project-dialog__path {',
      '.project-dialog__error:empty {',
    ]);
    expect(localMarkup).toContain('data-component="dialog-surface"');
    const recovery = String(
      ProjectDialog({ open: true, root: '.', error: 'Down', recovery: { expected: { pid: 7 } } }),
    );
    expect(recovery).toContain('data-component="state-banner"');
    expect(recovery).toContain('data-tone="warning"');
    expect(recovery).toContain('data-tone="danger"');
  });
});
