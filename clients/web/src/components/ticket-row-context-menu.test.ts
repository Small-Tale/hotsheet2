import { describe, expect, it } from 'vitest';

import { TICKET_CONTEXT_ACTIONS, TicketRowContextMenu } from './ticket-row-context-menu';

describe('TicketRowContextMenu', () => {
  it('gives every action a distinct meaningful Lucide icon', () => {
    expect(TICKET_CONTEXT_ACTIONS).toHaveLength(9);
    expect(new Set(TICKET_CONTEXT_ACTIONS.map(item => item.iconName)).size).toBe(TICKET_CONTEXT_ACTIONS.length);
    expect(TICKET_CONTEXT_ACTIONS.every(item => item.icon.length > 0)).toBe(true);
    const markup = String(TicketRowContextMenu({ x: 12, y: 24, category: 'bug', priority: 'high', status: 'started' }));
    for (const item of TICKET_CONTEXT_ACTIONS) expect(markup).toContain(`data-lucide="${item.iconName}"`);
  });

  it('renders checked metadata submenus with stable bulk mutation contracts', () => {
    const markup = String(TicketRowContextMenu({ x: 12, y: 24, category: 'bug', priority: 'high', status: 'started' }));
    for (const field of ['category', 'priority', 'status']) expect(markup).toContain(`data-context-field="${field}"`);
    expect(markup).toContain('slot="submenu" type="checkbox" checked data-context-field="category" data-context-value="bug"');
    expect(markup).toContain('slot="submenu" type="checkbox" checked data-context-field="priority" data-context-value="high"');
    expect(markup).toContain('slot="submenu" type="checkbox" checked data-context-field="status" data-context-value="started"');
    expect(markup).toContain('data-lucide="sparkles"');
    expect(markup).toContain('data-lucide="chevrons-up"');
    expect(markup).toContain('data-lucide="badge-check"');
  });

  it('omits Up Next when the selected ticket set is ineligible', () => {
    expect(String(TicketRowContextMenu({ x: 0, y: 0, upNextEligible: false }))).not.toContain('data-context-action="Toggle Up Next"');
  });
});
