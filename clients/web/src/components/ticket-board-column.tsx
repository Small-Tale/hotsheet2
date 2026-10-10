import './ticket-board-column.css';

import { TICKET_SELECTION_ACTIONS } from '../interaction-attrs/ticket-selection';
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
  /** Snap a horizontally scrolling board to this column's start edge (the paged board layout). */
  scrollSnap?: boolean;
}

export function TicketBoardColumn({
  id,
  title,
  tickets,
  totalCount = tickets.length,
  countPartial = false,
  selectionRoot = true,
  continuation,
  scrollSnap = false,
}: TicketBoardColumnProps) {
  const dropStatus = id === 'not-started' ? 'not_started' : id;
  return (
    <section
      class="ticket-board-column"
      data-key={`ticket-column:${id}`}
      data-component="ticket-board-column"
      data-column-id={id}
      data-scroll-snap={scrollSnap ? 'start' : undefined}
      data-ticket-drop-status={dropStatus}
      aria-label={`${title} column`}
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
              aria-label={`${countPartial ? 'At least ' : ''}${totalCount} tickets`}
              title={countPartial ? `At least ${totalCount} tickets` : undefined}
            >
              {totalCount}
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
        {tickets.map((ticket) => (
          <TicketRow {...ticket} presentation="column" />
        ))}
        {continuation ? (
          <TicketPageMore loading={continuation.loading} />
        ) : (
          totalCount > tickets.length && (
            <div class="ticket-board-column__progress" data-ticket-progressive-loading="true" role="status">
              {tickets.length} of {countPartial ? 'at least ' : ''}
              {totalCount} loaded
            </div>
          )
        )}
      </div>
    </section>
  );
}
