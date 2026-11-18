import { Chip } from '@kerfjs/ui/chip';
import { uiColor } from '@kerfjs/ui/css-values';
import { EmptyState } from '@kerfjs/ui/empty-state';
import { List } from '@kerfjs/ui/list';
import { ListActionRow } from '@kerfjs/ui/list-action-row';
import { ListInsetText } from '@kerfjs/ui/list-inset-text';
import { LucideIcon } from '@kerfjs/ui/lucide-icon';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';
import { Ellipsis, FolderKanban, GitMerge, List as ListIcon, Pencil, Trash2 } from 'lucide';

import { TICKET_GROUP_SETTINGS_ACTIONS } from '../interaction-attrs/ticket-groups';
import type { CheckoutGroupSummary } from '../server-response-parsers';

export interface TicketGroupSettingsProps {
  /** Every registry group of the project, empty ones included; undefined until the first read answers. */
  groups?: readonly CheckoutGroupSummary[];
  /** A failed read's message, shown with the last list. */
  error?: string;
  /** While a merge or delete runs, its group's actions are disabled. */
  busyGroup?: string;
}

/** The short id a duplicate name shows to tell its groups apart (the design's "01M7P6A"). */
export function ticketGroupShortId(id: string): string {
  return id.slice(0, 7);
}

/**
 * Project Settings › Ticket groups (HS2-GXKX1Q, Design v6): every group of the project, including
 * groups with no tickets, which ticket views leave out. A row opens its tickets; its "…" menu offers
 * View tickets, Rename…, Merge into…, and Delete group…. Each row shows the group's live ticket count
 * as a chip after its name, and a name several groups share also shows each group's short id. The
 * page's New group action sits in the workspace toolbar, as "+ Data Source…" does for ticket sources.
 */
export function TicketGroupSettings({ groups, error, busyGroup }: TicketGroupSettingsProps) {
  const others = (group: CheckoutGroupSummary) =>
    (groups ?? []).filter((other) => other.id !== group.id && !other.legacy);
  return (
    <div class="ticket-group-settings" data-component="ticket-group-settings">
      <List gap="l">
        {/* Bare text takes the rows' content edge, so it lines up with each row's icon (Design v6). */}
        <ListInsetText sides="rl">
          <Text tone="quiet" size="compact" flush>
            All project groups appear here, including empty ones. Ticket views show groups with active tickets.
          </Text>
        </ListInsetText>
        {error ? (
          <ListInsetText sides="rl">
            <Text tone="danger" role="alert" size="compact" flush>
              {error}
            </Text>
          </ListInsetText>
        ) : (
          <></>
        )}
        {groups === undefined ? (
          <EmptyState title="Loading Ticket Groups…" busy />
        ) : groups.length === 0 ? (
          <EmptyState
            title="No Ticket Groups Yet"
            detail="Create a group here, or group tickets from a ticket's menu."
            icon={<LucideIcon icon={FolderKanban} name="folder-kanban" />}
          />
        ) : (
          <List>
            {groups.map((group) => {
              const busy = busyGroup === group.id,
                attributes = { 'data-group-id': group.id };
              return (
                <ListActionRow
                  action={TICKET_GROUP_SETTINGS_ACTIONS.viewGroupTickets.value}
                  itemId={group.id}
                  accessibleLabel={`View tickets in ${group.name}`}
                  disabled={busy}
                  busy={busy}
                  icon={<LucideIcon icon={FolderKanban} name="folder-kanban" />}
                  label={
                    <Row gap="xs" vAlign="middle" wrap={false}>
                      <span>{group.name}</span>
                      <Chip size="compact" rootAttributes={{ 'data-group-count': String(group.count) }}>
                        {group.count}
                      </Chip>
                      {group.duplicate && (
                        <Text tone="quiet" size="compact" flush>
                          {ticketGroupShortId(group.id)}
                        </Text>
                      )}
                    </Row>
                  }
                  rootAttributes={{ 'data-group-row': '', ...attributes }}
                  trailingMenu={{
                    label: `${group.name} Group Actions`,
                    icon: <LucideIcon icon={Ellipsis} name="ellipsis" />,
                    disabled: busy,
                    items: [
                      {
                        label: 'View tickets',
                        action: TICKET_GROUP_SETTINGS_ACTIONS.viewGroupTickets.value,
                        icon: <LucideIcon icon={ListIcon} name="list" />,
                        attributes,
                      },
                      {
                        label: 'Rename…',
                        action: TICKET_GROUP_SETTINGS_ACTIONS.renameSettingsGroup.value,
                        icon: <LucideIcon icon={Pencil} name="pencil" />,
                        attributes,
                      },
                      {
                        label: 'Merge into…',
                        action: TICKET_GROUP_SETTINGS_ACTIONS.mergeSettingsGroup.value,
                        icon: <LucideIcon icon={GitMerge} name="git-merge" />,
                        attributes,
                        ...(others(group).length === 0
                          ? { disabled: true, disabledReason: 'There is no other group to merge into.' }
                          : {}),
                      },
                      {
                        label: 'Delete group…',
                        action: TICKET_GROUP_SETTINGS_ACTIONS.deleteSettingsGroup.value,
                        icon: <LucideIcon icon={Trash2} name="trash-2" color={uiColor('danger-on-quiet')} />,
                        tone: 'danger',
                        attributes,
                      },
                    ],
                  }}
                />
              );
            })}
          </List>
        )}
        <ListInsetText sides="rl">
          <Text tone="quiet" size="compact" flush>
            Deleting a group keeps its tickets and removes their group assignment. Merging moves members into another
            group.
          </Text>
        </ListInsetText>
      </List>
    </div>
  );
}
