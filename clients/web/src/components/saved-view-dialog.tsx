import './saved-view-dialog.css';

import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { Toolbar } from '@kerfjs/ui/toolbar';

import { type InlineSearchToken, orderedSearchText } from '../inline-search';
import { TicketSearchField, TicketSearchSurfaces } from './ticket-search-field';

export interface SavedViewDialogProps {
  open: boolean;
  mode?: 'create' | 'rename';
  name: string;
  query: string;
  queryTokens?: readonly InlineSearchToken[];
  /** Every project tag, for the query field's in-place `tag:` completion. */
  tags?: readonly string[];
  /** Whether the query field's syntax help popover is open. */
  helpOpen?: boolean;
  busy?: boolean;
  error?: string;
  /**
   * Increments on every open. The token editor keeps its own text while morphing, so a new session replaces it
   * with the freshly seeded query even when the tokens match the previous session's.
   */
  session?: number;
}

export function SavedViewDialog({
  open,
  mode = 'create',
  name,
  query,
  queryTokens = [],
  tags = [],
  helpOpen = false,
  busy = false,
  error = '',
  session = 0,
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
      data-controlled-open={String(open)}
    >
      <form data-action="save-saved-view">
        <List className="saved-view-dialog__form" gap="l">
          <Text tone="quiet">
            {rename
              ? 'Change the shared view name or search query.'
              : 'Save a search as a project view. Everyone using this ticket store will see it.'}
          </Text>
          <wa-input
            name="saved-view-name"
            label="View name"
            value={name}
            required
            autofocus
            disabled={busy || undefined}
          ></wa-input>
          <label class="saved-view-dialog__query" data-key={`saved-view-query-session-${session}`}>
            <span>
              Search query <sup aria-hidden="true">*</sup>
            </span>
            <Toolbar
              className="saved-view-dialog__query-toolbar"
              centerAlign="stretch"
              center={
                <TicketSearchField
                  id="saved-view-query"
                  label="Search query"
                  query={query}
                  tokens={queryTokens}
                  disabled={busy}
                  tags={tags}
                  helpOpen={helpOpen}
                  clearLabel="Clear search query"
                  surfaces="external"
                />
              }
            />
            <TicketSearchSurfaces
              id="saved-view-query"
              query={query}
              tokens={queryTokens}
              tags={tags}
              helpOpen={helpOpen}
            />
            <input type="hidden" name="saved-view-query" value={queryValue} />
            <small>Use the same words, fields, operators, and filter chips as ticket search.</small>
          </label>
          {error ? (
            <p class="saved-view-dialog__error" role="alert">
              {error}
            </p>
          ) : undefined}
          <footer>
            <Row hAlign="right" vAlign="middle" gap="xs">
              <wa-button appearance="plain" type="button" data-action="cancel-saved-view" disabled={busy || undefined}>
                Cancel
              </wa-button>
              <wa-button appearance="accent" type="submit" disabled={busy || undefined}>
                {busy ? (rename ? 'Saving…' : 'Creating…') : title}
              </wa-button>
            </Row>
          </footer>
        </List>
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
      data-controlled-open={String(open)}
    >
      <List className="saved-view-dialog__form" gap="l">
        <Text tone="quiet">
          Delete <strong>{name}</strong> for everyone using this ticket store? Tickets are not affected.
        </Text>
        {error ? (
          <p class="saved-view-dialog__error" role="alert">
            {error}
          </p>
        ) : undefined}
        <footer>
          <Row hAlign="right" vAlign="middle" gap="xs">
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
          </Row>
        </footer>
      </List>
    </wa-dialog>
  );
}
