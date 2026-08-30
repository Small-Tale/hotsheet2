import './ticket-board.css';

import { TicketBoardColumn, type TicketBoardColumnProps } from './ticket-board-column';

export type TicketColumnProps = TicketBoardColumnProps;

export interface TicketBoardProps {
  columns: TicketColumnProps[];
  label?: string;
}

export function TicketBoard({ columns, label = 'Ticket board' }: TicketBoardProps) {
  return <section class="ticket-board" data-ticket-selection-root="true" role="listbox" aria-multiselectable="true" aria-label={label}>
    <div class="ticket-board__columns" style={`--ticket-board-column-count:${columns.length};--ticket-board-min-width:${columns.length * 250 + Math.max(0, columns.length - 1) * 12}px`}>
      {columns.map(column => <TicketBoardColumn {...column} selectionRoot={false} />)}
    </div>
  </section>;
}
