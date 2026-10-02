import './ticket-list.css';

import type { CorruptTicket } from '../api';
import { VIEWS_AND_SAVED_VIEWS_TARGETS } from '../interaction-attrs/views-and-saved-views';
import { corruptTicketKey, type CorruptTicketRecoveryState, CorruptTicketRow } from './corrupt-ticket-row';
import { TicketEmptyState, type TicketEmptyStateProps } from './ticket-empty-state';
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
}

export function TicketList({
  tickets,
  totalCount = tickets.length,
  corruptTickets = [],
  corruptRecovery = {},
  selectedCorruptKey,
  label = 'Tickets',
  emptyState,
}: TicketListProps) {
  const empty = tickets.length === 0 && corruptTickets.length === 0;
  const loadingMore = totalCount > tickets.length;
  return (
    <section
      class="ticket-list"
      data-key="ticket-list"
      data-component="ticket-list"
      data-empty={empty ? 'true' : undefined}
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
      >
        {empty && emptyState ? (
          <TicketEmptyState {...emptyState} />
        ) : (
          tickets.map((ticket, index) => (
            <TicketRow {...ticket} presentation="list" listEdge={listEdge(index, tickets.length)} />
          ))
        )}
        {loadingMore && (
          <div class="ticket-list__progress" data-ticket-progressive-loading="true" role="status">
            Loading more tickets…
          </div>
        )}
      </div>
    </section>
  );
}
