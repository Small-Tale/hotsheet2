import { describe, expect, it } from 'vitest';

import { COMPLETED_TICKET_CONTEXT_ACTIONS, TICKET_CONTEXT_ACTIONS, TicketRowContextMenu } from './ticket-row-context-menu';

describe('TicketRowContextMenu', () => {
  it('gives every action a distinct meaningful Lucide icon', () => {
    expect(TICKET_CONTEXT_ACTIONS).toHaveLength(10);
    expect(new Set(TICKET_CONTEXT_ACTIONS.map(item => item.iconName)).size).toBe(TICKET_CONTEXT_ACTIONS.length);
    expect(TICKET_CONTEXT_ACTIONS.every(item => item.icon.length > 0)).toBe(true);
    const markup = String(TicketRowContextMenu({ x: 12, y: 24, category: 'bug', priority: 'high', status: 'started' }));
    for (const item of TICKET_CONTEXT_ACTIONS) expect(markup).toContain(`data-lucide="${item.iconName}"`);
  });

  it('offers Move to Backlog immediately before Archive (HS2-ZEQBMH)', () => {
    const markup = String(TicketRowContextMenu({ x: 0, y: 0, status: 'started' }));
    expect(markup).toContain('data-context-action="Move to Backlog"');
    expect(markup.indexOf('data-context-action="Move to Backlog"')).toBeLessThan(markup.indexOf('data-context-action="Archive ticket"'));
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

  it('hides Open ticket when more than one ticket is selected (HS2-XRENF2)', () => {
    expect(String(TicketRowContextMenu({ x: 0, y: 0 }))).toContain('data-context-action="Open ticket"');
    expect(String(TicketRowContextMenu({ x: 0, y: 0, selectionCount: 1 }))).toContain('data-context-action="Open ticket"');
    expect(String(TicketRowContextMenu({ x: 0, y: 0, selectionCount: 3 }))).not.toContain('data-context-action="Open ticket"');
  });

  it('prepends icon-bearing completed actions only when explicitly eligible', () => {
    const ordinary = String(TicketRowContextMenu({ x: 0, y: 0, status: 'started' }));
    expect(ordinary).not.toContain('data-context-action="Verify ticket"');
    const completed = String(TicketRowContextMenu({ x: 0, y: 0, status: 'completed', upNextEligible: false, verifyAction: true, notWorkingAction: true }));
    for (const item of COMPLETED_TICKET_CONTEXT_ACTIONS) {
      expect(completed).toContain(`data-context-action="${item.action}"`);
      expect(completed).toContain(`data-lucide="${item.iconName}"`);
    }
    expect(completed.indexOf('data-context-action="Verify ticket"')).toBeLessThan(completed.indexOf('data-context-action="Open ticket"'));
    expect(completed).not.toContain('data-context-action="Toggle Up Next"');
    const verifyOnly = String(TicketRowContextMenu({ x: 0, y: 0, verifyAction: true }));
    expect(verifyOnly).toContain('data-context-action="Verify ticket"');
    expect(verifyOnly).not.toContain('data-context-action="Report not working"');
  });
});
