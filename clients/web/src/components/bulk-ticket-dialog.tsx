import '@awesome.me/webawesome/dist/components/button/button.js';
import '@awesome.me/webawesome/dist/components/input/input.js';

import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';

import { TICKET_SELECTION_ACTIONS } from '../interaction-attrs/ticket-selection';

export type BulkTicketDialogState =
  | { kind: 'tag'; mode: 'add' | 'remove'; count: number; choices: string[] }
  | { kind: 'delete'; count: number }
  | { kind: 'empty-trash'; count: number; busy?: boolean; error?: string };

export function BulkTicketDialog({ state }: { state?: BulkTicketDialogState }) {
  if (!state) return <></>;
  if (state.kind === 'empty-trash')
    return (
      <div data-key="bulk-ticket-dialog">
        <DialogSurface
          label="Empty Trash?"
          size="small"
          rootAttributes={{
            'data-key': 'bulk-ticket-dialog',
            'data-dialog-component': 'empty-trash-dialog',
            'data-controlled-open': 'true',
          }}
          footer={
            <Row hAlign="right" vAlign="middle" gap="xs" wrap>
              <wa-button
                {...TICKET_SELECTION_ACTIONS.cancelBulkTicketAction.attrs}
                appearance="outlined"
                disabled={state.busy}
              >
                Cancel
              </wa-button>
              <wa-button {...TICKET_SELECTION_ACTIONS.confirmEmptyTrash.attrs} variant="danger" disabled={state.busy}>
                {state.busy ? 'Emptying…' : 'Empty Trash'}
              </wa-button>
            </Row>
          }
        >
          <List gap="m">
            <Text flush>
              Permanently remove {state.count} ticket{state.count === 1 ? '' : 's'} from this project’s active store.
              Git history will still contain the removed files.
            </Text>
            {Boolean(state.error) && (
              <Text flush tone="danger" role="alert">
                {state.error}
              </Text>
            )}
          </List>
        </DialogSurface>
      </div>
    );
  if (state.kind === 'delete')
    return (
      <div data-key="bulk-ticket-dialog-2">
        <DialogSurface
          label={`Delete ${state.count} ticket${state.count === 1 ? '' : 's'}?`}
          size="small"
          rootAttributes={{
            'data-key': 'bulk-ticket-dialog-2',
            'data-dialog-component': 'bulk-delete-dialog',
            'data-controlled-open': 'true',
          }}
          footer={
            <Row hAlign="right" vAlign="middle" gap="xs" wrap>
              <wa-button {...TICKET_SELECTION_ACTIONS.cancelBulkTicketAction.attrs} appearance="outlined">
                Cancel
              </wa-button>
              <wa-button {...TICKET_SELECTION_ACTIONS.confirmBulkDelete.attrs} variant="danger">
                Delete {state.count} ticket{state.count === 1 ? '' : 's'}
              </wa-button>
            </Row>
          }
        >
          <Text flush>
            Deleted tickets leave the active project views. This action can be undone with the standard Undo shortcut.
          </Text>
        </DialogSurface>
      </div>
    );
  const adding = state.mode === 'add';
  return (
    <div data-key="bulk-ticket-dialog-3">
      <DialogSurface
        label={`${adding ? 'Add' : 'Remove'} tag — ${state.count} selected`}
        size="small"
        rootAttributes={{
          'data-key': 'bulk-ticket-dialog-3',
          'data-dialog-component': 'bulk-tag-dialog',
          'data-controlled-open': 'true',
        }}
      >
        <form {...TICKET_SELECTION_ACTIONS.submitBulkTag.attrs} data-tag-mode={state.mode}>
          <List gap="m">
            <wa-input name="bulk-ticket-tag" label={adding ? 'Tag to add' : 'Tag to remove'} required autofocus />
            {!adding && state.choices.length > 0 && (
              <div role="group" aria-label="Tags in selection">
                <Row gap="xs" wrap>
                  {state.choices.map((tag) => (
                    <wa-button
                      type="button"
                      size="small"
                      appearance="outlined"
                      {...TICKET_SELECTION_ACTIONS.chooseBulkTag.attrs}
                      data-tag={tag}
                    >
                      {tag}
                    </wa-button>
                  ))}
                </Row>
              </div>
            )}
            <Row hAlign="right" vAlign="middle" gap="xs" wrap>
              <wa-button type="button" {...TICKET_SELECTION_ACTIONS.cancelBulkTicketAction.attrs} appearance="outlined">
                Cancel
              </wa-button>
              <wa-button type="submit" variant="brand">
                {adding ? 'Add tag' : 'Remove tag'}
              </wa-button>
            </Row>
          </List>
        </form>
      </DialogSurface>
    </div>
  );
}
