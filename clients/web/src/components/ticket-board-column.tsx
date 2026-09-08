import './ticket-board-column.css';

import { TicketRow, type TicketRowProps } from './ticket-row';

export interface TicketBoardColumnProps {
  id: string;
  title: string;
  tickets: TicketRowProps[];
  totalCount?: number;
  selectionRoot?: boolean;
}

export function TicketBoardColumn({
  id,
  title,
  tickets,
  totalCount = tickets.length,
  selectionRoot = true,
}: TicketBoardColumnProps) {
  const dropStatus = id === 'not-started' ? 'not_started' : id;
  return <section class="ticket-board-column" data-key={`ticket-column:${id}`} data-component="ticket-board-column" data-column-id={id} data-ticket-drop-status={dropStatus} aria-label={`${title} column`}>
    <header>
      <h2 id={`ticket-column-${id}`}><button type="button" class="ticket-board-column__header" data-action="select-ticket-column" aria-label={`Select all ${title} tickets`}>
        <span class="ticket-board-column__title">{title}</span>
        <span aria-label={`${totalCount} tickets`}>{totalCount}</span>
      </button></h2>
    </header>
    <div class="ticket-board-column__tickets" data-key={`ticket-column-scroll:${id}`} data-ticket-scroll-owner={`column:${id}`} data-ticket-selection-root={selectionRoot ? 'true' : undefined} role={selectionRoot ? 'listbox' : 'group'} aria-label={`${title} tickets`} aria-multiselectable={selectionRoot ? 'true' : undefined}>
      {tickets.map(ticket => <TicketRow {...ticket} presentation="column" />)}
      {totalCount>tickets.length&&<div class="ticket-board-column__progress" data-ticket-progressive-loading="true" role="status">Loading more…</div>}
    </div>
  </section>;
}
