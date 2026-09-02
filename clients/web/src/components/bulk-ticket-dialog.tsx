import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/dialog/dialog.js';
import '@awesome.me/webawesome/dist/components/input/input.js';
import './bulk-ticket-dialog.css';

export type BulkTicketDialogState =
  | { kind: 'tag'; mode: 'add' | 'remove'; count: number; choices: string[] }
  | { kind: 'delete'; count: number };

export function BulkTicketDialog({ state }: { state?: BulkTicketDialogState }) {
  if (!state) return <></>;
  if (state.kind === 'delete') return <wa-dialog open data-component="bulk-delete-dialog" label={`Delete ${state.count} ticket${state.count === 1 ? '' : 's'}?`}>
    <p>Deleted tickets leave the active project views. This action can be undone with the standard Undo shortcut.</p>
    <div slot="footer" class="bulk-ticket-dialog__actions">
      <wa-button data-action="cancel-bulk-ticket-action" appearance="outlined">Cancel</wa-button>
      <wa-button data-action="confirm-bulk-delete" variant="danger">Delete {state.count} ticket{state.count === 1 ? '' : 's'}</wa-button>
    </div>
  </wa-dialog>;
  const adding = state.mode === 'add';
  return <wa-dialog open data-component="bulk-tag-dialog" label={`${adding ? 'Add' : 'Remove'} tag — ${state.count} selected`}>
    <form data-action="submit-bulk-tag" data-tag-mode={state.mode}>
      <wa-input name="bulk-ticket-tag" label={adding ? 'Tag to add' : 'Tag to remove'} required autofocus></wa-input>
      {!adding && state.choices.length > 0 && <div class="bulk-ticket-dialog__choices" aria-label="Tags in selection">
        {state.choices.map(tag => <button type="button" data-action="choose-bulk-tag" data-tag={tag}>{tag}</button>)}
      </div>}
      <div class="bulk-ticket-dialog__actions">
        <wa-button type="button" data-action="cancel-bulk-ticket-action" appearance="outlined">Cancel</wa-button>
        <wa-button type="submit" variant="brand">{adding ? 'Add tag' : 'Remove tag'}</wa-button>
      </div>
    </form>
  </wa-dialog>;
}
