import './saved-view-dialog.css';

import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import type { TokenSearchModel } from '@kerfjs/ui/token-search-model';

import { orderedSearchText } from '../inline-search';
import {
  VIEWS_AND_SAVED_VIEWS_ACTIONS,
  VIEWS_AND_SAVED_VIEWS_TARGETS,
} from '../interaction-attrs/views-and-saved-views';
import { inlineSearchTokens } from '../ticket-search-model';
import { TicketSearchFormField } from './ticket-search-field';

export interface SavedViewDialogProps {
  open: boolean;
  mode?: 'create' | 'rename';
  name: string;
  /** The Kerf-managed query model: text, chips, and in-place `tag:` completion (HS2-5JXBQY). */
  searchModel: TokenSearchModel;
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
  searchModel,
  helpOpen = false,
  busy = false,
  error = '',
  session = 0,
}: SavedViewDialogProps) {
  const rename = mode === 'rename',
    title = rename ? 'Edit View' : 'Create View',
    state = searchModel.state.value,
    queryValue = orderedSearchText(state.query, inlineSearchTokens(state), () => true);
  return (
    <wa-dialog
      class="saved-view-dialog"
      {...VIEWS_AND_SAVED_VIEWS_TARGETS.savedViewDialog.attrs}
      data-mode={mode}
      label={title}
      aria-label={title}
      open={open || undefined}
      data-controlled-open={String(open)}
    >
      <form {...VIEWS_AND_SAVED_VIEWS_ACTIONS.saveSavedView.attrs}>
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
          />
          <div class="saved-view-dialog__query" data-key={`saved-view-query-session-${session}`}>
            <TicketSearchFormField
              id="saved-view-query"
              label="Search query"
              model={searchModel}
              disabled={busy}
              required
              hint="Use the same words, fields, operators, and filter chips as ticket search."
              helpOpen={helpOpen}
              clearLabel="Clear search query"
            />
            <input type="hidden" name="saved-view-query" value={queryValue} />
          </div>
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
                {...VIEWS_AND_SAVED_VIEWS_ACTIONS.cancelSavedView.attrs}
                disabled={busy || undefined}
              >
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
      {...VIEWS_AND_SAVED_VIEWS_TARGETS.savedViewDeleteDialog.attrs}
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
              {...VIEWS_AND_SAVED_VIEWS_ACTIONS.cancelDeleteSavedView.attrs}
              disabled={busy || undefined}
            >
              Cancel
            </wa-button>
            <wa-button
              variant="danger"
              type="button"
              {...VIEWS_AND_SAVED_VIEWS_ACTIONS.confirmDeleteSavedView.attrs}
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
