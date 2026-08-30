import { describe, expect, it } from 'vitest';

import { TICKET_CONTEXT_ACTIONS, TicketRowContextMenu } from './ticket-row-context-menu';

describe('TicketRowContextMenu', () => {
  it('gives every action a distinct meaningful Lucide icon', () => {
    expect(TICKET_CONTEXT_ACTIONS).toHaveLength(9);
    expect(new Set(TICKET_CONTEXT_ACTIONS.map(item => item.iconName)).size).toBe(TICKET_CONTEXT_ACTIONS.length);
    expect(TICKET_CONTEXT_ACTIONS.every(item => item.icon.length > 0)).toBe(true);
    const markup = String(TicketRowContextMenu({ x: 12, y: 24 }));
    for (const item of TICKET_CONTEXT_ACTIONS) expect(markup).toContain(`data-lucide="${item.iconName}"`);
  });
});
