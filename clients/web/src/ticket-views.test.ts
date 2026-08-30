import { describe, expect, it } from 'vitest';

import type { TicketRow } from './api';
import { isArchivedTicket, isQueuedTicket, ticketsForView } from './ticket-views';

const ticket = (status: string): TicketRow => ({
  connection_id: 'git', native_id: status, qualified_id: `git:${status}`, id: status,
  slug: `HS-${status}`, title: status, status, up_next: false, tags: [], blocked_by: [], claim_count: 0,
});

describe('ticket views', () => {
  it('partitions active queue, backlog, and every archived status without overlap', () => {
    const tickets = ['not_started', 'started', 'backlog', 'completed', 'verified', 'archive', 'deleted', 'moved'].map(ticket);
    expect(tickets.filter(isQueuedTicket).map(item => item.status)).toEqual(['not_started', 'started']);
    expect(tickets.filter(isArchivedTicket).map(item => item.status)).toEqual(['completed', 'verified', 'archive', 'deleted', 'moved']);
    expect(ticketsForView(tickets, 'all').map(item => item.status)).toEqual(['not_started', 'started']);
    expect(ticketsForView(tickets, 'backlog').map(item => item.status)).toEqual(['backlog']);
    expect(ticketsForView(tickets, 'archive').map(item => item.status)).toEqual(['completed', 'verified', 'archive', 'deleted', 'moved']);
  });
});
