import './saved-view-dialog.css';

export interface SavedViewDialogProps {
  open: boolean;
  mode?: 'create' | 'rename';
  name: string;
  query: string;
  busy?: boolean;
  error?: string;
}

export function SavedViewDialog({ open, mode = 'create', name, query, busy = false, error = '' }: SavedViewDialogProps) {
  const rename=mode==='rename',title=rename?'Rename View':'Create View';
  return <wa-dialog class="saved-view-dialog" data-component="saved-view-dialog" data-mode={mode} label={title} aria-label={title} open={open || undefined}>
    <form data-action="save-saved-view" class="saved-view-dialog__form">
      <p>{rename?'Change the shared view name. Its search query stays the same.':'Save a search as a project view. Everyone using this ticket store will see it.'}</p>
      <wa-input name="saved-view-name" label="View name" value={name} required autofocus disabled={busy || undefined}></wa-input>
      {!rename&&<wa-input name="saved-view-query" label="Search query" value={query} required disabled={busy || undefined} help-text="Use the same words, fields, and operators as ticket search."></wa-input>}
      {error && <p class="saved-view-dialog__error" role="alert">{error}</p>}
      <footer>
        <wa-button appearance="plain" type="button" data-action="cancel-saved-view" disabled={busy || undefined}>Cancel</wa-button>
        <wa-button appearance="accent" type="submit" disabled={busy || undefined}>{busy ? rename?'Renaming…':'Creating…' : title}</wa-button>
      </footer>
    </form>
  </wa-dialog>;
}

export function SavedViewDeleteDialog({open,name,busy=false,error=''}:{open:boolean;name:string;busy?:boolean;error?:string}){
  return <wa-dialog class="saved-view-dialog" data-component="saved-view-delete-dialog" label="Delete View?" aria-label="Delete View?" open={open||undefined}>
    <div class="saved-view-dialog__form"><p>Delete <strong>{name}</strong> for everyone using this ticket store? Tickets are not affected.</p>{error&&<p class="saved-view-dialog__error" role="alert">{error}</p>}<footer><wa-button appearance="plain" type="button" data-action="cancel-delete-saved-view" disabled={busy||undefined}>Cancel</wa-button><wa-button variant="danger" type="button" data-action="confirm-delete-saved-view" disabled={busy||undefined}>{busy?'Deleting…':'Delete View'}</wa-button></footer></div>
  </wa-dialog>
}
