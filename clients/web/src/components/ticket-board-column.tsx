import './ticket-board-column.css';

import { TicketRow, type TicketRowProps } from './ticket-row';

export interface TicketBoardColumnProps {
  id: string;
  title: string;
  tickets: TicketRowProps[];
  selectionRoot?: boolean;
}

export function TicketBoardColumn({ id, title, tickets, selectionRoot = true }: TicketBoardColumnProps) {
  return <section class="ticket-board-column" data-component="ticket-board-column" data-column-id={id} aria-label={`${title} column`}>
    <header>
      <h2 id={`ticket-column-${id}`}><button type="button" class="ticket-board-column__header" data-action="select-ticket-column" aria-label={`Select all ${title} tickets`}>
        <span class="ticket-board-column__title">{title}</span>
        <span aria-label={`${tickets.length} tickets`}>{tickets.length}</span>
      </button></h2>
    </header>
    <div class="ticket-board-column__tickets" data-ticket-selection-root={selectionRoot ? 'true' : undefined} role={selectionRoot ? 'listbox' : 'group'} aria-label={`${title} tickets`} aria-multiselectable={selectionRoot ? 'true' : undefined}>
      {tickets.map(ticket => <TicketRow {...ticket} presentation="column" />)}
    </div>
  </section>;
}
