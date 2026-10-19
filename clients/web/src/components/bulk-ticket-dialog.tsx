import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';

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
              <Button
                action={TICKET_SELECTION_ACTIONS.cancelBulkTicketAction.value}
                emphasis="secondary"
                disabled={state.busy}
              >
                Cancel
              </Button>
              <Button action={TICKET_SELECTION_ACTIONS.confirmEmptyTrash.value} tone={'danger'} disabled={state.busy}>
                {state.busy ? 'Emptying…' : 'Empty Trash'}
              </Button>
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
              <Button action={TICKET_SELECTION_ACTIONS.cancelBulkTicketAction.value} emphasis="secondary">
                Cancel
              </Button>
              <Button action={TICKET_SELECTION_ACTIONS.confirmBulkDelete.value} tone={'danger'}>
                {`Delete ${state.count} ticket${state.count === 1 ? '' : 's'}`}
              </Button>
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
            <TextField name="bulk-ticket-tag" label={adding ? 'Tag to add' : 'Tag to remove'} required autofocus />
            {!adding && state.choices.length > 0 && (
              <div role="group" aria-label="Tags in selection">
                <Row gap="xs" wrap>
                  {state.choices.map((tag) => (
                    <Button
                      type="button"
                      size="compact"
                      emphasis="secondary"
                      action={TICKET_SELECTION_ACTIONS.chooseBulkTag.value}
                      rootAttributes={{ 'data-tag': tag }}
                    >
                      {tag}
                    </Button>
                  ))}
                </Row>
              </div>
            )}
            <Row hAlign="right" vAlign="middle" gap="xs" wrap>
              <Button type="button" action={TICKET_SELECTION_ACTIONS.cancelBulkTicketAction.value} emphasis="secondary">
                Cancel
              </Button>
              <Button type="submit" tone={'brand'}>
                {adding ? 'Add tag' : 'Remove tag'}
              </Button>
            </Row>
          </List>
        </form>
      </DialogSurface>
    </div>
  );
}
