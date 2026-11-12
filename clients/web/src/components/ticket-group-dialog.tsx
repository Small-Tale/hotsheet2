import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';
import { TextField } from '@kerfjs/ui/text-field';

import { TICKET_GROUP_ACTIONS } from '../interaction-attrs/ticket-groups';
import { ticketGroupNameError, ticketGroupNameProblem } from '../ticket-groups';

export interface TicketGroupDialogState {
  /** `new` names a group for the target tickets; `rename` renames an existing group (HS2-8CDFKR). */
  mode: 'new' | 'rename';
  /** The typed name. */
  name: string;
  /** The display names a new name must not repeat: every other group in the project. */
  groups: readonly string[];
  /** The stored value of the group being renamed (HS2-9KZSD4). */
  group?: string;
  /** Increments per open so the field never shows a previous session's text. */
  session: number;
  busy?: boolean;
  /** A failed write's message. */
  error?: string;
}

/** The New Group and Rename Group dialog (HS2-TX17CM, HS2-8CDFKR). */
export function TicketGroupDialog({ state }: { state?: TicketGroupDialogState }) {
  if (!state) return <></>;
  const renaming = state.mode === 'rename',
    problem = ticketGroupNameProblem(state.name, state.groups),
    error = ticketGroupNameError(problem, state.name);
  return (
    <div data-key="ticket-group-dialog">
      <DialogSurface
        label={renaming ? 'Rename Group' : 'New Group'}
        size="small"
        rootAttributes={{
          'data-key': 'ticket-group-dialog',
          'data-dialog-component': 'ticket-group-dialog',
          'data-controlled-open': 'true',
        }}
      >
        <form {...TICKET_GROUP_ACTIONS.submitTicketGroupName.attrs} data-group-mode={state.mode}>
          <List gap="m">
            <TextField
              rootAttributes={{ 'data-key': `ticket-group-name-${state.session}` }}
              name="ticket-group-name"
              label="Group Name"
              value={state.name}
              error={error}
              required
              autofocus
            />
            {Boolean(state.error) && (
              <Text flush tone="danger" role="alert">
                {state.error}
              </Text>
            )}
            <Row hAlign="right" vAlign="middle" gap="xs" wrap>
              <Button
                type="button"
                action={TICKET_GROUP_ACTIONS.cancelTicketGroupName.value}
                emphasis="secondary"
                disabled={state.busy}
              >
                Cancel
              </Button>
              <Button type="submit" tone="brand" disabled={Boolean(problem) || state.busy}>
                {renaming ? 'Rename' : 'Create Group'}
              </Button>
            </Row>
          </List>
        </form>
      </DialogSurface>
    </div>
  );
}
