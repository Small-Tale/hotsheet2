import { Button } from '@kerfjs/ui/button';
import { List } from '@kerfjs/ui/list';
import { ListInsetControl } from '@kerfjs/ui/list-inset-control';
import { ListInsetText } from '@kerfjs/ui/list-inset-text';
import { Row } from '@kerfjs/ui/row';
import { Select } from '@kerfjs/ui/select';
import { DialogSurface } from '@kerfjs/ui/surface-scaffold';
import { Text } from '@kerfjs/ui/text';

import { TICKET_GROUP_SETTINGS_ACTIONS } from '../interaction-attrs/ticket-groups';

export interface TicketGroupMergeDialogState {
  /** The group whose members move, and which is removed afterwards. */
  group: { id: string; name: string; count: number };
  /** The groups it can merge into: every other non-legacy group, by id and display name. */
  destinations: readonly { id: string; name: string; duplicate?: boolean }[];
  /** The chosen destination's id. */
  into: string;
  /** Increments per open so the Select never shows a previous session's choice. */
  session: number;
  busy?: boolean;
  /** A failed merge's message. */
  error?: string;
}

const ticketCount = (count: number) => `${count} ticket${count === 1 ? '' : 's'}`;

/**
 * Project Settings › Ticket groups › Merge into… (HS2-GXKX1Q, Design v5): move every ticket of one
 * group into a chosen group, then remove the source group. A destination name several groups share
 * also shows its short id, as the settings list does.
 */
export function TicketGroupMergeDialog({ state }: { state?: TicketGroupMergeDialogState }) {
  if (!state) return <></>;
  return (
    <div data-key="ticket-group-merge-dialog">
      <DialogSurface
        label="Merge group"
        size="small"
        // The prose sits on the title's text edge and the Select on the control edge (Design v5).
        bodyInset="none"
        rootAttributes={{
          'data-key': 'ticket-group-merge-dialog',
          'data-dialog-component': 'ticket-group-merge-dialog',
          'data-controlled-open': 'true',
        }}
      >
        <form {...TICKET_GROUP_SETTINGS_ACTIONS.submitGroupMerge.attrs} data-group-id={state.group.id}>
          <List gap="m">
            <ListInsetText sides="trl">
              <Text flush>
                Move {ticketCount(state.group.count)} from <strong>{state.group.name}</strong> into another group. The
                source group will be removed.
              </Text>
            </ListInsetText>
            <ListInsetControl>
              <Select
                rootAttributes={{ 'data-key': `ticket-group-merge-into-${state.session}` }}
                name="ticket-group-merge-into"
                label="Destination Group"
                value={state.into}
                disabled={state.busy}
                choices={state.destinations.map((group) => ({
                  value: group.id,
                  label: group.duplicate ? `${group.name} (${group.id.slice(0, 7)})` : group.name,
                }))}
              />
            </ListInsetControl>
            {Boolean(state.error) && (
              <ListInsetText sides="rl">
                <Text flush tone="danger" role="alert">
                  {state.error}
                </Text>
              </ListInsetText>
            )}
            <ListInsetControl>
              <Row hAlign="right" vAlign="middle" gap="xs" wrap>
                <Button
                  type="button"
                  action={TICKET_GROUP_SETTINGS_ACTIONS.cancelGroupMerge.value}
                  emphasis="secondary"
                  disabled={state.busy}
                >
                  Cancel
                </Button>
                <Button type="submit" tone="brand" disabled={!state.into || state.busy}>
                  Merge Groups
                </Button>
              </Row>
            </ListInsetControl>
          </List>
        </form>
      </DialogSurface>
    </div>
  );
}
