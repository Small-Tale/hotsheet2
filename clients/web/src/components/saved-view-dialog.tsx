import { Button } from '@kerfjs/ui/button';
import { remify } from '@kerfjs/ui/css-values';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import type { SearchFieldModel } from '@kerfjs/ui/search-field-model';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';

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
  searchModel: SearchFieldModel;
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
    <div data-key="saved-view-dialog">
      <DialogSurface
        label={title}
        preferredWidth={remify(512)}
        viewportGutter={remify(16)}
        rootAttributes={{
          'data-key': 'saved-view-dialog',
          ...VIEWS_AND_SAVED_VIEWS_TARGETS.savedViewDialog.attrs,
          'data-mode': mode,
          'data-controlled-open': String(open),
        }}
      >
        <form {...VIEWS_AND_SAVED_VIEWS_ACTIONS.saveSavedView.attrs}>
          <List gap="l">
            <Text tone="quiet">
              {rename
                ? 'Change the shared view name or search query.'
                : 'Save a search as a project view. Everyone using this ticket store will see it.'}
            </Text>
            <TextField
              name="saved-view-name"
              label="View Name"
              value={name}
              required
              autofocus
              disabled={busy || undefined}
            />
            <div data-key={`saved-view-query-session-${session}`}>
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
              <Text flush tone="danger" role="alert">
                {error}
              </Text>
            ) : undefined}
            <footer>
              <Row hAlign="right" vAlign="middle" gap="xs">
                <Button
                  emphasis="plain"
                  type="button"
                  action={VIEWS_AND_SAVED_VIEWS_ACTIONS.cancelSavedView.value}
                  disabled={busy || undefined}
                >
                  Cancel
                </Button>
                <Button emphasis="primary" type="submit" disabled={busy || undefined}>
                  {busy ? (rename ? 'Saving…' : 'Creating…') : title}
                </Button>
              </Row>
            </footer>
          </List>
        </form>
      </DialogSurface>
    </div>
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
    <div data-key="saved-view-dialog-2">
      <DialogSurface
        label="Delete View?"
        preferredWidth={remify(512)}
        viewportGutter={remify(16)}
        rootAttributes={{
          'data-key': 'saved-view-dialog-2',
          ...VIEWS_AND_SAVED_VIEWS_TARGETS.savedViewDeleteDialog.attrs,
          'data-controlled-open': String(open),
        }}
      >
        <List gap="l">
          <Text tone="quiet">
            Delete <strong>{name}</strong> for everyone using this ticket store? Tickets are not affected.
          </Text>
          {error ? (
            <Text flush tone="danger" role="alert">
              {error}
            </Text>
          ) : undefined}
          <footer>
            <Row hAlign="right" vAlign="middle" gap="xs">
              <Button
                emphasis="plain"
                type="button"
                action={VIEWS_AND_SAVED_VIEWS_ACTIONS.cancelDeleteSavedView.value}
                disabled={busy || undefined}
              >
                Cancel
              </Button>
              <Button
                tone={'danger'}
                type="button"
                action={VIEWS_AND_SAVED_VIEWS_ACTIONS.confirmDeleteSavedView.value}
                disabled={busy || undefined}
              >
                {busy ? 'Deleting…' : 'Delete View'}
              </Button>
            </Row>
          </footer>
        </List>
      </DialogSurface>
    </div>
  );
}
