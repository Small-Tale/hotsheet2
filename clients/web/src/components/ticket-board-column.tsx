import './ticket-board-column.css';

import { TicketRow, type TicketRowProps } from './ticket-row';

export interface TicketBoardColumnProps {
  id: string;
  title: string;
  tickets: TicketRowProps[];
  selectionRoot?: boolean;
}

export function TicketBoardColumn({ id, title, tickets, selectionRoot = true }: TicketBoardColumnProps) {
  return <section class="ticket-board-column" data-component="ticket-board-column" data-column-id={id} aria-labelledby={`ticket-column-${id}`}>
    <header class="ticket-board-column__header">
      <h2 id={`ticket-column-${id}`}>{title}</h2>
      <span aria-label={`${tickets.length} tickets`}>{tickets.length}</span>
    </header>
    <div class="ticket-board-column__tickets" data-ticket-selection-root={selectionRoot ? 'true' : undefined} role={selectionRoot ? 'listbox' : 'group'} aria-label={`${title} tickets`} aria-multiselectable={selectionRoot ? 'true' : undefined}>
      {tickets.map(ticket => <TicketRow {...ticket} />)}
    </div>
  </section>;
}
