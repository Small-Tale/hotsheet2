import { describe, expect, it } from 'vitest';
import { TicketBoard } from './ticket-board';
import type { TicketRowProps } from './ticket-row';

const ticket: TicketRowProps = { slug: 'HS2-BOARD', title: 'Shared board row', status: 'started', priority: 'high', category: 'feature', tags: ['client'] };

describe('TicketBoard', () => {
  it('renders labeled columns, accurate counts, and the same TicketRow component', () => {
    const markup = String(TicketBoard({ columns: [
      { id: 'active', title: 'Active', tickets: [ticket] },
      { id: 'done', title: 'Done', tickets: [] },
    ] }));
    expect(markup).toContain('data-column-id="active"');
    expect(markup).toContain('aria-label="1 tickets"');
    expect(markup).toContain('aria-label="0 tickets"');
    expect(markup.match(/data-component="ticket-list-row"/g)).toHaveLength(1);
    expect(markup).not.toContain('ticket-card');
  });
});
