import './saved-view-dialog.css';

export interface SavedViewDialogProps {
  open: boolean;
  name: string;
  query: string;
  busy?: boolean;
  error?: string;
}

export function SavedViewDialog({ open, name, query, busy = false, error = '' }: SavedViewDialogProps) {
  return <wa-dialog class="saved-view-dialog" data-component="saved-view-dialog" label="Create View" aria-label="Create View" open={open || undefined}>
    <form data-action="create-saved-view" class="saved-view-dialog__form">
      <p>Save a search as a project view. Everyone using this ticket store will see it.</p>
      <wa-input name="saved-view-name" label="View name" value={name} required autofocus disabled={busy || undefined}></wa-input>
      <wa-input name="saved-view-query" label="Search query" value={query} required disabled={busy || undefined} help-text="Use the same words, fields, and operators as ticket search."></wa-input>
      {error && <p class="saved-view-dialog__error" role="alert">{error}</p>}
      <footer>
        <wa-button appearance="plain" type="button" data-action="cancel-saved-view" disabled={busy || undefined}>Cancel</wa-button>
        <wa-button appearance="accent" type="submit" disabled={busy || undefined}>{busy ? 'Creating…' : 'Create View'}</wa-button>
      </footer>
    </form>
  </wa-dialog>;
}
