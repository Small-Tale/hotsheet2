import './ticket-list.css';

import { List } from '@kerfjs/ui/list';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';

import type { CorruptTicket } from '../api';
import { TICKET_GROUP_TARGETS } from '../interaction-attrs/ticket-groups';
import { VIEWS_AND_SAVED_VIEWS_TARGETS } from '../interaction-attrs/views-and-saved-views';
import { ticketGroupSegments } from '../ticket-groups';
import { corruptTicketKey, type CorruptTicketRecoveryState, CorruptTicketRow } from './corrupt-ticket-row';
import { TicketEmptyState, type TicketEmptyStateProps } from './ticket-empty-state';
import { TicketGroup } from './ticket-group';
import { TicketRow, type TicketRowListEdge, type TicketRowProps } from './ticket-row';

/** The rounded-corner edge of the item at `index` in a section of `count` rows. */
export function listEdge(index: number, count: number): TicketRowListEdge | undefined {
  if (count === 1) return 'only';
  if (index === 0) return 'start';
  return index === count - 1 ? 'end' : undefined;
}

export interface TicketListProps {
  tickets: TicketRowProps[];
  totalCount?: number;
  corruptTickets?: CorruptTicket[];
  corruptRecovery?: Record<string, CorruptTicketRecoveryState>;
  selectedCorruptKey?: string;
  label?: string;
  emptyState?: TicketEmptyStateProps;
  /** Keys of the groups collapsed in this view (HS2-WZ3JGZ). */
  collapsedGroups?: readonly string[];
  /** Key of the group a dragged ticket is over (HS2-PRXD39). */
  dropGroup?: string;
}

export function TicketList({
  tickets,
  totalCount = tickets.length,
  corruptTickets = [],
  corruptRecovery = {},
  selectedCorruptKey,
  label = 'Tickets',
  emptyState,
  collapsedGroups = [],
  dropGroup,
}: TicketListProps) {
  const empty = tickets.length === 0 && corruptTickets.length === 0;
  // A list with groups splits into segments: each run of ungrouped tickets is its own rounded
  // card, and each group sits directly on the workspace with its own card of members, so no list
  // surface shows behind a group's rounded corners (HS2-WZ3JGZ).
  const segments = ticketGroupSegments(tickets),
    grouped = segments.some((segment) => segment.kind === 'group'),
    rows = (items: TicketRowProps[]) =>
      items.map((ticket, index) => (
        <TicketRow {...ticket} presentation="list" listEdge={listEdge(index, items.length)} />
      ));
  const loadingMore = totalCount > tickets.length;
  const loadingRow = (
    <Row hAlign="center" controlInsets="tb">
      <Text variant="p" tone="quiet" size="compact" role="status" data-ticket-progressive-loading="true">
        Loading more tickets…
      </Text>
    </Row>
  );
  return (
    <section
      class="ticket-list"
      data-key="ticket-list"
      data-component="ticket-list"
      data-empty={empty ? 'true' : undefined}
      data-grouped={grouped ? 'true' : undefined}
      data-rendered-count={tickets.length}
      data-total-count={totalCount}
    >
      {corruptTickets.length > 0 && (
        <div class="ticket-list__diagnostics" aria-label="Unreadable tickets">
          {corruptTickets.map((ticket, index) => (
            <CorruptTicketRow
              ticket={ticket}
              listEdge={listEdge(index, corruptTickets.length)}
              recovery={corruptRecovery[corruptTicketKey(ticket)]}
              selected={selectedCorruptKey === corruptTicketKey(ticket)}
            />
          ))}
        </div>
      )}
      <div
        class="ticket-list__tickets"
        {...VIEWS_AND_SAVED_VIEWS_TARGETS.ticketSelectionRoot.attrs}
        role="listbox"
        aria-label={label}
        aria-multiselectable="true"
        // Dropping a grouped ticket on the list outside every group removes its group (HS2-PRXD39).
        {...(grouped ? TICKET_GROUP_TARGETS.ungroupDropZone.attrs : {})}
      >
        {grouped ? (
          <>
            {segments.map((segment) =>
              segment.kind === 'group' ? (
                <TicketGroup
                  group={segment.group}
                  name={segment.name}
                  count={segment.items.length}
                  scope="list"
                  collapsed={collapsedGroups.includes(segment.key)}
                  dropTarget={dropGroup === segment.key}
                >
                  <List gap="none" rootAttributes={{ role: 'none' }}>
                    {rows(segment.items)}
                  </List>
                </TicketGroup>
              ) : (
                <div
                  class="ticket-list__segment"
                  data-key={`ticket-list-segment:${segment.items[0].qualifiedId ?? segment.items[0].slug}`}
                >
                  <List gap="none" rootAttributes={{ role: 'none' }}>
                    {rows(segment.items)}
                  </List>
                </div>
              ),
            )}
            {loadingMore && loadingRow}
          </>
        ) : (
          <List gap="none" rootAttributes={{ role: 'none' }}>
            {empty && emptyState ? <TicketEmptyState {...emptyState} /> : rows(tickets)}
            {loadingMore && loadingRow}
          </List>
        )}
      </div>
    </section>
  );
}
