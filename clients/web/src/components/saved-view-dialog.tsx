import './saved-view-dialog.css';
import './workspace-header.css';
import '@kerfjs/ui/token-search-field.css';

import { TokenSearchField } from '@kerfjs/ui/token-search-field';
import { ToolbarControlGroup } from '@kerfjs/ui/toolbar-control-group';

import { type InlineSearchToken, orderedSearchText, toTokenSearchToken } from '../inline-search';

export interface SavedViewDialogProps {
  open: boolean;
  mode?: 'create' | 'rename';
  name: string;
  query: string;
  queryTokens?: readonly InlineSearchToken[];
  busy?: boolean;
  error?: string;
}

export function SavedViewDialog({
  open,
  mode = 'create',
  name,
  query,
  queryTokens = [],
  busy = false,
  error = '',
}: SavedViewDialogProps) {
  const rename = mode === 'rename',
    title = rename ? 'Edit View' : 'Create View',
    queryValue = orderedSearchText(query, queryTokens, () => true);
  return (
    <wa-dialog
      class="saved-view-dialog"
      data-component="saved-view-dialog"
      data-mode={mode}
      label={title}
      aria-label={title}
      open={open || undefined}
    >
      <form data-action="save-saved-view" class="saved-view-dialog__form">
        <p>
          {rename
            ? 'Change the shared view name or search query.'
            : 'Save a search as a project view. Everyone using this ticket store will see it.'}
        </p>
        <wa-input
          name="saved-view-name"
          label="View name"
          value={name}
          required
          autofocus
          disabled={busy || undefined}
        ></wa-input>
        <label class="saved-view-dialog__query">
          <span>
            Search query <sup aria-hidden="true">*</sup>
          </span>
          <ToolbarControlGroup className="workspace-header__search-group saved-view-dialog__query-field" expanded>
            <TokenSearchField
              id="saved-view-query"
              label="Search query"
              query={query}
              tokens={queryTokens.map(toTokenSearchToken)}
              placeholder="Search tickets"
              disabled={busy}
              editAction="edit-saved-view-query-token"
              removeAction="remove-saved-view-query-token"
              clearAction="clear-saved-view-query"
              clearLabel="Clear search query"
            />
          </ToolbarControlGroup>
          <input type="hidden" name="saved-view-query" value={queryValue} />
          <small>Use the same words, fields, operators, and filter chips as ticket search.</small>
        </label>
        {error && (
          <p class="saved-view-dialog__error" role="alert">
            {error}
          </p>
        )}
        <footer>
          <wa-button appearance="plain" type="button" data-action="cancel-saved-view" disabled={busy || undefined}>
            Cancel
          </wa-button>
          <wa-button appearance="accent" type="submit" disabled={busy || undefined}>
            {busy ? (rename ? 'Saving…' : 'Creating…') : title}
          </wa-button>
        </footer>
      </form>
    </wa-dialog>
  );
}

export function SavedViewDeleteDialog({
  open,
  name,
  busy = false,
  error = '',
}: {
  open: boolean;
  name: string;
  busy?: boolean;
  error?: string;
}) {
  return (
    <wa-dialog
      class="saved-view-dialog"
      data-component="saved-view-delete-dialog"
      label="Delete View?"
      aria-label="Delete View?"
      open={open || undefined}
    >
      <div class="saved-view-dialog__form">
        <p>
          Delete <strong>{name}</strong> for everyone using this ticket store? Tickets are not affected.
        </p>
        {error && (
          <p class="saved-view-dialog__error" role="alert">
            {error}
          </p>
        )}
        <footer>
          <wa-button
            appearance="plain"
            type="button"
            data-action="cancel-delete-saved-view"
            disabled={busy || undefined}
          >
            Cancel
          </wa-button>
          <wa-button
            variant="danger"
            type="button"
            data-action="confirm-delete-saved-view"
            disabled={busy || undefined}
          >
            {busy ? 'Deleting…' : 'Delete View'}
          </wa-button>
        </footer>
      </div>
    </wa-dialog>
  );
}
