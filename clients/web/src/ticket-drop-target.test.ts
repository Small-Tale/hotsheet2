import { afterEach, describe, expect, it } from 'vitest';

import {
  clearTicketDropTarget,
  columnTicketDropKey,
  isTicketDropTarget,
  markTicketDropTarget,
  NEW_TICKET_LAUNCHER_DROP_KEY,
  ticketDropTarget,
  viewTicketDropKey,
} from './ticket-drop-target';

describe('ticketDropTarget (HS2-G8EK08)', () => {
  afterEach(() => {
    ticketDropTarget.value = undefined;
  });

  it('marks one target at a time', () => {
    markTicketDropTarget(viewTicketDropKey('backlog'));
    markTicketDropTarget(viewTicketDropKey('backlog'));
    expect(isTicketDropTarget('view:backlog')).toBe(true);
    markTicketDropTarget(columnTicketDropKey('started'));
    expect(isTicketDropTarget('view:backlog')).toBe(false);
    expect(isTicketDropTarget('column:started')).toBe(true);
  });

  it('ignores a late leave from a target the drag already left, then clears the current one', () => {
    markTicketDropTarget('view:backlog');
    markTicketDropTarget('view:archive');
    clearTicketDropTarget('view:backlog');
    expect(ticketDropTarget.value).toBe('view:archive');
    clearTicketDropTarget('view:archive');
    expect(ticketDropTarget.value).toBeUndefined();
    clearTicketDropTarget('view:archive');
    expect(ticketDropTarget.value).toBeUndefined();
  });

  it('clears any target on drop or dragend and accepts a new one afterwards', () => {
    markTicketDropTarget(NEW_TICKET_LAUNCHER_DROP_KEY);
    clearTicketDropTarget();
    expect(ticketDropTarget.value).toBeUndefined();
    clearTicketDropTarget();
    markTicketDropTarget('column:completed');
    expect(ticketDropTarget.value).toBe('column:completed');
  });
});
