import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { tokenFromRaw } from '../inline-search';
import { SavedViewDeleteDialog, SavedViewDialog } from './saved-view-dialog';

describe('SavedViewDialog', () => {
  it('owns the full query width independently of the collapsed workspace toolbar', () => {
    const css = readFileSync(new URL('./saved-view-dialog.css', import.meta.url), 'utf8');
    expect(css).toMatch(
      /\.saved-view-dialog__query > \.saved-view-dialog__query-field\s*\{[^}]*width: 100%;[^}]*min-width: 0;/,
    );
  });
  it('collects a shared view name in the standard tokenized query editor', () => {
    const markup = String(
      SavedViewDialog({
        open: true,
        name: 'Needs docs',
        query: ' AND NOT status:completed',
        queryTokens: [tokenFromRaw('tag:docs')!],
      }),
    );
    expect(markup).toContain('data-component="saved-view-dialog"');
    expect(markup).toContain('name="saved-view-name"');
    expect(markup).toContain('name="saved-view-query"');
    expect(markup).toContain('data-token-search-editor="saved-view-query"');
    expect(markup).toContain('data-component="token-search-token"');
    expect(markup).toContain('data-token-value="tag:docs"');
    expect(markup).toContain('tag:docs');
    expect(markup).toContain('Everyone using this ticket store will see it.');
    expect(markup).toContain('data-action="save-saved-view"');
  });

  it('keeps validation feedback in the dialog and locks controls while saving', () => {
    const markup = String(
      SavedViewDialog({ open: true, name: '', query: '', busy: true, error: 'That name is already in use.' }),
    );
    expect(markup).toContain('role="alert"');
    expect(markup).toContain('That name is already in use.');
    expect(markup.match(/disabled/g)?.length).toBeGreaterThanOrEqual(3);
    expect(markup).toContain('Creating…');
  });

  it('edits both the shared view name and tokenized query', () => {
    const markup = String(
      SavedViewDialog({
        open: true,
        mode: 'rename',
        name: 'Needs docs',
        query: '',
        queryTokens: [tokenFromRaw('tag:docs')!],
      }),
    );
    expect(markup).toContain('label="Edit View"');
    expect(markup).toContain('Change the shared view name or search query.');
    expect(markup).toContain('name="saved-view-query"');
    expect(markup).toContain('data-action="edit-saved-view-query-token"');
  });

  it('retains one native name autofocus target through opening, edits, busy, close, rename, and reset', () => {
    for (const state of [
      { open: false, name: '', query: '' },
      { open: true, name: '', query: '' },
      { open: true, name: '', query: 'before after', queryTokens: [tokenFromRaw('is:active')!] },
      { open: true, name: 'Draft', query: 'before after', busy: true },
      { open: true, name: 'Draft', query: 'before after', error: 'Try another name.' },
      { open: false, name: 'Draft', query: 'before after' },
      { open: true, mode: 'rename' as const, name: 'Shared view', query: 'tag:docs' },
      { open: false, name: '', query: '' },
      { open: true, name: '', query: '' },
    ]) {
      const markup = String(SavedViewDialog(state));
      expect(markup.match(/autofocus/g)).toHaveLength(1);
      expect(markup).toMatch(/<wa-input[^>]*name="saved-view-name"[^>]*autofocus/);
      const host = markup.slice(0, markup.indexOf('>') + 1);
      expect(/\sopen(?:[\s=>])/.test(host)).toBe(state.open);
      expect(markup).toContain(`value="${state.name}"`);
      expect(markup).toContain('data-token-search-editor="saved-view-query"');
    }
  });

  it('confirms shared deletion without implying tickets are removed', () => {
    const markup = String(SavedViewDeleteDialog({ open: true, name: 'Needs docs' }));
    expect(markup).toContain('data-component="saved-view-delete-dialog"');
    expect(markup).toContain('Tickets are not affected.');
    expect(markup).toContain('data-action="confirm-delete-saved-view"');
  });
});
