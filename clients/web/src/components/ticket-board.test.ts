import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { describe, expect, it } from 'vitest';

import { TicketBoard } from './ticket-board';
import { TicketBoardColumn } from './ticket-board-column';
import type { TicketRowProps } from './ticket-row';

const ticket: TicketRowProps = { slug: 'HS2-BOARD', title: 'Shared board row', status: 'started', priority: 'high', category: 'feature', tags: ['client'] };

describe('TicketBoard', () => {
  it('renders labeled columns, accurate counts, and the same TicketRow component', () => {
    const markup = String(TicketBoard({ columns: [
      { id: 'active', title: 'Active', tickets: [ticket] },
      { id: 'done', title: 'Done', tickets: [] },
    ] }));
    expect(markup).toContain('data-column-id="active"');
    expect(markup).toContain('data-key="ticket-board"');
    expect(markup).toContain('data-key="ticket-column:active"');
    expect(markup).toContain('data-key="ticket-column-scroll:active"');
    expect(markup).toContain('data-key="ticket:HS2-BOARD"');
    expect(markup).toContain('data-ticket-selection-root="true"');
    expect(markup).toContain('role="listbox"');
    expect(markup).toContain('role="group"');
    expect(markup.match(/data-component="ticket-board-column"/g)).toHaveLength(2);
    expect(markup).toContain('aria-label="1 tickets"');
    expect(markup).toContain('aria-label="0 tickets"');
    expect(markup.match(/data-component="ticket-list-row"/g)).toHaveLength(1);
    expect(markup).toContain('data-presentation="column"');
    expect(markup).not.toContain('ticket-card');
  });

  it('leaves columns visually unframed beneath their title and count', () => {
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-board-column.css'), 'utf8');
    const rule = css.match(/\.ticket-board-column \{([^}]*)\}/)?.[1] ?? '';
    expect(rule).not.toMatch(/background|border|padding|border-radius/);
    expect(css).toMatch(/ticket-board-column > header h2[^}]*font: inherit/);
    expect(css).toMatch(/ticket-board-column__header[^}]*height: 2rem/);
    expect(css).toMatch(/ticket-board-column__tickets[^}]*padding: \.1rem \.5rem 1rem/);
  });

  it('matches the outer margin and inter-column spacing (HS2-VX9E4Z)', () => {
    // Outer margin before the first column = board padding-inline (.5rem) + column padding (.5rem).
    // With a 0 column-grid gap, inter-column spacing = column padding (.5rem) + 0 + column padding (.5rem),
    // so the space between columns matches the space before the first / after the last column.
    const markup = String(TicketBoard({ columns: [{ id: 'one', title: 'One', tickets: [] }, { id: 'two', title: 'Two', tickets: [] }] }));
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-board.css'), 'utf8');
    expect(markup).toContain('--ticket-board-min-width:500px');
    expect(css).toMatch(/ticket-board[^}]*padding-inline: \.5rem/);
    expect(css).toMatch(/ticket-board__columns[^}]*gap: 0/);
  });

  it('projects one independently scrollable column with a derived count', () => {
    const markup = String(TicketBoardColumn({ id: 'active', title: 'Active', tickets: [ticket] }));
    expect(markup).toContain('data-ticket-drop-status="active"');
    expect(markup).toContain('data-component="ticket-board-column"');
    expect(markup).toContain('aria-label="1 tickets"');
    expect(markup).toContain('data-action="select-ticket-column"');
    expect(markup).toContain('aria-label="Select all Active tickets"');
    expect(markup).toContain('aria-label="Active tickets"');
    const css = readFileSync(resolve(import.meta.dirname, 'ticket-board-column.css'), 'utf8');
    expect(css).toMatch(/ticket-board-column__tickets[^}]*overflow-y: auto/);
  });

  it('maps the Not Started column id to the wire status used by ticket drops', () => {
    expect(String(TicketBoardColumn({ id: 'not-started', title: 'Not Started', tickets: [] }))).toContain('data-ticket-drop-status="not_started"');
  });
});
