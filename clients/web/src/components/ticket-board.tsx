import './ticket-board.css';

import { VIEWS_AND_SAVED_VIEWS_TARGETS } from '../interaction-attrs/views-and-saved-views';
import { TicketBoardColumn, type TicketBoardColumnProps } from './ticket-board-column';
import { TicketEmptyState, type TicketEmptyStateProps } from './ticket-empty-state';

export type TicketColumnProps = TicketBoardColumnProps;

/**
 * `grid` fits every column side by side (scrolling horizontally only below the per-column minimum);
 * `paged` shows one near-full-width column at a time with the next one peeking, and snaps to the
 * nearest column with an animated scroll when a horizontal scroll or swipe is released (HS2-ZYJMDP).
 */
export type TicketBoardLayout = 'grid' | 'paged';

export interface TicketBoardProps {
  columns: TicketColumnProps[];
  label?: string;
  emptyState?: TicketEmptyStateProps;
  layout?: TicketBoardLayout;
}

export function TicketBoard({ columns, label = 'Ticket board', emptyState, layout = 'grid' }: TicketBoardProps) {
  // A layout switch remounts the board so a paged scroll offset never strands the grid mid-column.
  const empty = columns.every((column) => (column.totalCount ?? column.tickets.length) === 0);
  return (
    <section
      class="ticket-board"
      data-key={layout === 'grid' ? 'ticket-board' : `ticket-board:${layout}`}
      data-component="ticket-board"
      data-layout={layout}
      {...VIEWS_AND_SAVED_VIEWS_TARGETS.ticketSelectionRoot.attrs}
      role="listbox"
      aria-multiselectable="true"
      aria-label={label}
    >
      <div
        class="ticket-board__columns"
        style={`--ticket-board-column-count:${columns.length};--ticket-board-min-width:${columns.length * 250}px`}
      >
        {/* Each column carries its own continuation so every column paginates independently (HS2-8NBGBX). */}
        {columns.map((column) => (
          <TicketBoardColumn {...column} selectionRoot={false} scrollSnap={layout === 'paged'} />
        ))}
      </div>
      {empty && emptyState && (
        <div class="ticket-board__empty">
          <TicketEmptyState {...emptyState} />
        </div>
      )}
    </section>
  );
}
