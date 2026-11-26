import './ticket-board-column.css';

import { Badge } from '@kerfjs/ui/badge';
import { Row } from '@kerfjs/ui/row';
import { Text } from '@kerfjs/ui/text';

import { TICKET_SELECTION_ACTIONS } from '../interaction-attrs/ticket-selection';
import { columnTicketDropKey } from '../ticket-drop-target';
import { ticketGroupSegments } from '../ticket-groups';
import { TicketGroup } from './ticket-group';
import { TicketPageMore } from './ticket-page-more';
import { TicketRow, type TicketRowProps } from './ticket-row';

export interface TicketBoardColumnProps {
  id: string;
  title: string;
  tickets: TicketRowProps[];
  totalCount?: number;
  countPartial?: boolean;
  selectionRoot?: boolean;
  continuation?: { loading: boolean };
  /** Keys of the groups collapsed in this view; a collapsed group collapses in every column. */
  collapsedGroups?: readonly string[];
  /** Key of the group in this column that a dragged ticket is over (HS2-PRXD39). */
  dropGroup?: string;
  /** Whether a dragged ticket is over this column itself (HS2-G8EK08), rather than over one of its groups. */
  dropTarget?: boolean;
}

export function TicketBoardColumn({
  id,
  title,
  tickets,
  totalCount = tickets.length,
  countPartial = false,
  selectionRoot = true,
  continuation,
  collapsedGroups = [],
  dropGroup,
  dropTarget = false,
}: TicketBoardColumnProps) {
  const dropStatus = id === 'not-started' ? 'not_started' : id;
  return (
    <section
      class="ticket-board-column"
      data-key={`ticket-column:${id}`}
      data-component="ticket-board-column"
      data-column-id={id}
      data-ticket-drop-status={dropStatus}
      data-ticket-drop-key={columnTicketDropKey(id)}
      data-dragging-ticket={dropTarget ? 'true' : undefined}
      aria-label={`${title} Column`}
    >
      <header>
        <h2 id={`ticket-column-${id}`}>
          <button
            type="button"
            class="ticket-board-column__header"
            {...TICKET_SELECTION_ACTIONS.selectTicketColumn.attrs}
            aria-label={`Select all ${title} tickets`}
          >
            <span class="ticket-board-column__title">{title}</span>
            <span
              class="ticket-board-column__count"
              title={countPartial ? `At least ${totalCount} tickets` : undefined}
            >
              <Badge size="compact" label={`${countPartial ? 'At least ' : ''}${totalCount} tickets`}>
                {totalCount}
              </Badge>
            </span>
          </button>
        </h2>
      </header>
      <div
        class="ticket-board-column__tickets"
        data-key={`ticket-column-scroll:${id}`}
        data-ticket-scroll-owner={`column:${id}`}
        data-ticket-selection-root={selectionRoot ? 'true' : undefined}
        role={selectionRoot ? 'listbox' : 'group'}
        aria-label={`${title} tickets`}
        aria-multiselectable={selectionRoot ? 'true' : undefined}
      >
        {/* Each column gathers only its own members of a group, where the group's first ticket in
            this column sorts, so one group can appear in several columns (HS2-HV8S4Y). */}
        {ticketGroupSegments(tickets).map((segment) =>
          segment.kind === 'group' ? (
            <TicketGroup
              group={segment.group}
              name={segment.name}
              count={segment.items.length}
              scope={id}
              presentation="column"
              collapsed={collapsedGroups.includes(segment.key)}
              dropTarget={dropGroup === segment.key}
              memberSlugs={segment.items.map((ticket) => ticket.slug)}
            >
              {segment.items.map((ticket) => (
                <TicketRow {...ticket} presentation="column" />
              ))}
            </TicketGroup>
          ) : (
            segment.items.map((ticket) => <TicketRow {...ticket} presentation="column" />)
          ),
        )}
        {continuation ? (
          <TicketPageMore loading={continuation.loading} />
        ) : (
          totalCount > tickets.length && (
            <Row hAlign="center" controlInsets="tb">
              <Text variant="p" tone="quiet" size="compact" role="status" data-ticket-progressive-loading="true">
                {tickets.length} of {countPartial ? 'at least ' : ''}
                {totalCount} loaded
              </Text>
            </Row>
          )
        )}
      </div>
    </section>
  );
}
