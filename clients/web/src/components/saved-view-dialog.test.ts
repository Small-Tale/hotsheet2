import { describe, expect, it } from 'vitest';

import { SavedViewDeleteDialog, SavedViewDialog } from './saved-view-dialog';

describe('SavedViewDialog', () => {
  it('collects a shared view name and ordinary search query', () => {
    const markup = String(SavedViewDialog({ open: true, name: 'Needs docs', query: 'tag:docs AND NOT status:completed' }));
    expect(markup).toContain('data-component="saved-view-dialog"');
    expect(markup).toContain('name="saved-view-name"');
    expect(markup).toContain('name="saved-view-query"');
    expect(markup).toContain('Everyone using this ticket store will see it.');
    expect(markup).toContain('data-action="save-saved-view"');
  });

  it('keeps validation feedback in the dialog and locks controls while saving', () => {
    const markup = String(SavedViewDialog({ open: true, name: '', query: '', busy: true, error: 'That name is already in use.' }));
    expect(markup).toContain('role="alert"');
    expect(markup).toContain('That name is already in use.');
    expect(markup.match(/disabled/g)?.length).toBeGreaterThanOrEqual(3);
    expect(markup).toContain('Creating…');
  });

  it('renames without exposing or changing the saved query',()=>{
    const markup=String(SavedViewDialog({open:true,mode:'rename',name:'Needs docs',query:'tag:docs'}));
    expect(markup).toContain('label="Rename View"');
    expect(markup).toContain('Its search query stays the same.');
    expect(markup).not.toContain('name="saved-view-query"');
  });

  it('confirms shared deletion without implying tickets are removed',()=>{
    const markup=String(SavedViewDeleteDialog({open:true,name:'Needs docs'}));
    expect(markup).toContain('data-component="saved-view-delete-dialog"');
    expect(markup).toContain('Tickets are not affected.');
    expect(markup).toContain('data-action="confirm-delete-saved-view"');
  });
});
