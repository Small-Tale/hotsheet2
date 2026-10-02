import { readFileSync } from 'node:fs';

import { describe, expect, it } from 'vitest';

import { createTicketSearchModel, replaceTicketSearch } from '../ticket-search-model';
import { SavedViewDeleteDialog, SavedViewDialog } from './saved-view-dialog';

/** A Kerf-managed query model: `text` committed through the app parser, `typing` left as trailing text. */
function queryModel(text = '', typing = '', tags: readonly string[] = ['design', 'docs', 'server']) {
  const model = createTicketSearchModel({ tags: () => tags });
  if (text) replaceTicketSearch(model, text);
  if (typing) model.edit({ query: `${model.state.value.query}${typing}`, tokens: model.state.value.tokens });
  return model;
}

describe('SavedViewDialog', () => {
  it('lays the query out as a Kerf form field beside the name input, not a toolbar (HS2-E40KC0)', () => {
    const css = readFileSync(new URL('./saved-view-dialog.css', import.meta.url), 'utf8');
    // Kerf's form-field presentation owns the label, inset, and full width; the dialog adds no query layout CSS.
    expect(css).not.toContain('saved-view-dialog__query');
    const markup = String(SavedViewDialog({ open: true, name: 'Needs docs', searchModel: queryModel('tag:docs') }));
    expect(markup).not.toContain('kui-toolbar');
    expect(markup).not.toContain('saved-view-dialog__query-toolbar');
    expect(markup).toContain('<div class="ticket-search-form-field" data-ticket-search-for="saved-view-query">');
    expect(markup).toContain('data-presentation="form-field"');
    expect(markup).toMatch(
      /<div class="kui-token-search__field-label" id="saved-view-query-label"[^>]*>Search query<span class="kui-token-search__field-required" aria-hidden="true">\*<\/span>/,
    );
    expect(markup).toContain('aria-labelledby="saved-view-query-label"');
    expect(markup).toContain('aria-describedby="saved-view-query-hint"');
    expect(markup).toContain('aria-required="true"');
    expect(markup).toContain('Use the same words, fields, operators, and filter chips as ticket search.');
  });
  it('collects a shared view name in the standard tokenized query editor', () => {
    const markup = String(
      SavedViewDialog({
        open: true,
        name: 'Needs docs',
        searchModel: queryModel('tag:docs', ' AND NOT status:completed tag:d'),
        helpOpen: true,
      }),
    );
    expect(markup).toContain('data-component="saved-view-dialog"');
    // The shared ticket search brings tag completion and syntax help into the dialog (HS2-N5G6JS).
    expect(markup).not.toContain('workspace-header__search-group');
    expect(markup).toContain('aria-label="Search syntax help"');
    expect(markup).toContain('aria-label="Search syntax"');
    // The surfaces stack in flow below the field, inside the same wrapper, so nothing floats in the clipped body.
    const fieldStart = markup.indexOf('data-presentation="form-field"'),
      surfacesStart = markup.indexOf('class="ticket-search-surfaces"');
    expect(fieldStart).toBeGreaterThan(0);
    expect(surfacesStart).toBeGreaterThan(fieldStart);
    expect(markup).toMatch(
      /<div class="ticket-search-surfaces" data-ticket-search-for="saved-view-query" data-token-search-keep-open>[\s\S]*aria-label="Search syntax"[\s\S]*<input type="hidden" name="saved-view-query"/,
    );
    // Kerf's tag completion excludes the committed chip (HS2-5JXBQY).
    expect(markup).toContain('data-token-search-suggestion="tag:design"');
    expect(markup).not.toContain('data-token-search-suggestion="tag:docs"');
    expect(markup).not.toContain('data-token-search-suggestion="tag:server"');
    expect(markup).toContain('data-action="clear-ticket-search"');
    expect(markup).toContain('aria-label="Clear search query"');
    expect(markup).toContain('name="saved-view-name"');
    expect(markup).toContain('name="saved-view-query"');
    expect(markup).toContain('data-token-search-editor="saved-view-query"');
    expect(markup).toContain('data-component="token-search-token"');
    expect(markup).toContain('data-token-value="tag:docs"');
    expect(markup).toContain('Everyone using this ticket store will see it.');
    expect(markup).toContain('data-action="save-saved-view"');
    expect(markup).toContain('data-component="list"');
    expect(markup).toContain('data-gap="true"');
    expect(markup).toContain('data-component="row"');
    expect(markup).toContain('data-h-align="right"');
  });

  it('keeps validation feedback in the dialog and locks controls while saving', () => {
    const markup = String(
      SavedViewDialog({
        open: true,
        name: '',
        searchModel: queryModel(),
        busy: true,
        error: 'That name is already in use.',
      }),
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
        searchModel: queryModel('tag:docs'),
      }),
    );
    expect(markup).toContain('label="Edit View"');
    expect(markup).toContain('Change the shared view name or search query.');
    expect(markup).toContain('name="saved-view-query"');
    expect(markup).toContain('data-action="edit-ticket-search-token"');
  });

  it('retains one native name autofocus target through opening, edits, busy, close, rename, and reset', () => {
    for (const state of [
      { open: false, name: '', searchModel: queryModel() },
      { open: true, name: '', searchModel: queryModel() },
      { open: true, name: '', searchModel: queryModel('before is:active after') },
      { open: true, name: 'Draft', searchModel: queryModel('before after'), busy: true },
      { open: true, name: 'Draft', searchModel: queryModel('before after'), error: 'Try another name.' },
      { open: false, name: 'Draft', searchModel: queryModel('before after') },
      { open: true, mode: 'rename' as const, name: 'Shared view', searchModel: queryModel('tag:docs') },
      { open: false, name: '', searchModel: queryModel() },
      { open: true, name: '', searchModel: queryModel() },
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
    expect(markup).toContain('data-component="list"');
    expect(markup).toContain('data-component="text"');
  });
});
