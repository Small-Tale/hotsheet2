import { describe, expect, it } from 'vitest';

import {
  CLOSE_TICKET_CONTEXT_ACTION,
  COMPLETED_TICKET_CONTEXT_ACTIONS,
  eventTargetsContextMenu,
  REOPEN_TICKET_CONTEXT_ACTION,
  RESTORE_TICKET_CONTEXT_ACTION,
  TICKET_CONTEXT_ACTIONS,
  TicketRowContextMenu,
} from './ticket-row-context-menu';

/** The opening tag of the Kerf PopupMenu item whose leading attributes start with `attribute`. */
const itemTag = (markup: string, attribute: string) => new RegExp(`<button ${attribute}[^>]*>`).exec(markup)?.[0] ?? '';
/** Kerf PopupMenu items disable with `aria-disabled="true"` (Kerf UI 5.0.0-beta.106). */
const disabledItem = (markup: string, attribute: string) => itemTag(markup, attribute).includes('aria-disabled="true"');

describe('TicketRowContextMenu', () => {
  it('uses the composed path so shadow-menu interactions are inside and rows are outside', () => {
    const shadowPart = { matches: () => false };
    const menu = { matches: (selector: string) => selector === '[data-context-menu="ticket"]' };
    expect(eventTargetsContextMenu({ composedPath: () => [shadowPart, menu] })).toBe(true);
    expect(
      eventTargetsContextMenu({
        composedPath: () => [{ matches: (selector: string) => selector === '[data-action="select-ticket-row"]' }],
      }),
    ).toBe(false);
    expect(eventTargetsContextMenu({ composedPath: () => [{}] })).toBe(false);
  });
  it('gives every action a distinct meaningful Lucide icon', () => {
    expect(TICKET_CONTEXT_ACTIONS).toHaveLength(11);
    expect(new Set(TICKET_CONTEXT_ACTIONS.map((item) => item.iconName)).size).toBe(TICKET_CONTEXT_ACTIONS.length);
    expect(TICKET_CONTEXT_ACTIONS.every((item) => item.icon.length > 0)).toBe(true);
    const markup = String(TicketRowContextMenu({ x: 12, y: 24, category: 'bug', priority: 'high', status: 'started' }));
    for (const item of TICKET_CONTEXT_ACTIONS) expect(markup).toContain(`data-lucide="${item.iconName}"`);
  });

  it('offers Move to Backlog immediately before Archive (HS2-ZEQBMH)', () => {
    const markup = String(TicketRowContextMenu({ x: 0, y: 0, status: 'started' }));
    expect(markup).toContain('data-context-action="Move to Backlog"');
    expect(markup.indexOf('data-context-action="Move to Backlog"')).toBeLessThan(
      markup.indexOf('data-context-action="Archive ticket"'),
    );
  });

  it('disables destination actions when every selected ticket is already there', () => {
    const backlog = String(TicketRowContextMenu({ x: 0, y: 0, status: 'backlog', allInBacklog: true }));
    expect(disabledItem(backlog, 'data-context-action="Move to Backlog"')).toBe(true);
    expect(disabledItem(backlog, 'data-context-action="Archive ticket"')).toBe(false);
    const archive = String(TicketRowContextMenu({ x: 0, y: 0, status: 'archive', allInArchive: true }));
    expect(disabledItem(archive, 'data-context-action="Archive ticket"')).toBe(true);
    expect(disabledItem(archive, 'data-context-action="Move to Backlog"')).toBe(false);
  });

  it('offers Restore from Trash first only when every selected ticket is in Trash (HS2-MWDR19)', () => {
    const restore = `data-context-action="${RESTORE_TICKET_CONTEXT_ACTION.action}"`;
    expect(String(TicketRowContextMenu({ x: 0, y: 0, status: 'archive', allInArchive: true }))).not.toContain(restore);
    const trash = String(TicketRowContextMenu({ x: 0, y: 0, allInTrash: true, selectionCount: 2 }));
    expect(trash).toContain(restore);
    expect(trash).toContain('Restore from Trash');
    expect(trash).toContain('data-lucide="archive-restore"');
    expect(trash.indexOf(restore)).toBeLessThan(trash.indexOf('data-context-field="category"'));
    expect(disabledItem(trash, restore)).toBe(false);
    expect(
      disabledItem(String(TicketRowContextMenu({ x: 0, y: 0, allInTrash: true, canBulkUpdate: false })), restore),
    ).toBe(true);
  });

  it('renders checked metadata submenus with stable bulk mutation contracts', () => {
    const markup = String(TicketRowContextMenu({ x: 12, y: 24, category: 'bug', priority: 'high', status: 'started' }));
    for (const field of ['category', 'priority', 'status']) expect(markup).toContain(`data-context-field="${field}"`);
    // PopupMenu submenu choices: the app's routing attributes lead, then Kerf's checkable item (HS2-2EHD8R).
    for (const [field, value] of [
      ['category', 'bug'],
      ['priority', 'high'],
      ['status', 'started'],
    ]) {
      const tag = itemTag(markup, `data-context-field="${field}" data-context-value="${value}"`);
      expect(tag).toContain('role="menuitemcheckbox"');
      expect(tag).toContain(`value="${value}" aria-checked="true"`);
    }
    expect(markup).toContain('data-lucide="sparkles"');
    expect(markup).toContain('data-lucide="chevrons-up"');
    expect(markup).toContain('data-lucide="badge-check"');
    // A submenu cannot hold the status separator before Backlog (Kerf gap KF-7KR1BC).
    const backlog = itemTag(markup, 'data-context-field="status" data-context-value="backlog"');
    expect(backlog).toContain('role="menuitemcheckbox"');
    expect(backlog).toContain('aria-checked="false"');
    expect(markup).not.toMatch(/data-submenu=""[^>]*>(?:(?!<\/div>)[^])*kui-popup-menu__divider/);
    expect(markup).toContain('data-context-menu="ticket"');
    expect(markup).toContain('data-context-field="status" data-context-value="archive"');
  });

  it('omits Up Next when the selected ticket set is ineligible', () => {
    expect(String(TicketRowContextMenu({ x: 0, y: 0, upNextEligible: false }))).not.toContain(
      'data-context-action="Toggle Up Next"',
    );
  });

  it('omits Up Next when a colocated toolbar button already owns that action', () => {
    expect(String(TicketRowContextMenu({ x: 0, y: 0, upNextEligible: true, hideUpNext: true }))).not.toContain(
      'data-context-action="Toggle Up Next"',
    );
  });

  it('capability-disables every mutating bulk action while preserving duplication', () => {
    const markup = String(TicketRowContextMenu({ x: 0, y: 0, canBulkUpdate: false }));
    expect(disabledItem(markup, 'data-context-action="Add tag"')).toBe(true);
    expect(disabledItem(markup, 'data-context-action="Remove tag"')).toBe(true);
    expect(disabledItem(markup, 'data-context-action="Delete ticket"')).toBe(true);
    expect(itemTag(markup, 'data-context-action="Delete ticket"')).toContain('data-tone="danger"');
    expect(markup).toContain('data-context-action="Duplicate ticket"');
    expect(disabledItem(markup, 'data-context-action="Duplicate ticket"')).toBe(false);
  });

  it('hides Open ticket when more than one ticket is selected (HS2-XRENF2)', () => {
    expect(String(TicketRowContextMenu({ x: 0, y: 0 }))).toContain('data-context-action="Open ticket"');
    expect(String(TicketRowContextMenu({ x: 0, y: 0, selectionCount: 1 }))).toContain(
      'data-context-action="Open ticket"',
    );
    expect(String(TicketRowContextMenu({ x: 0, y: 0, selectionCount: 3 }))).not.toContain(
      'data-context-action="Open ticket"',
    );
  });

  it('prepends icon-bearing completed actions only when explicitly eligible', () => {
    const ordinary = String(TicketRowContextMenu({ x: 0, y: 0, status: 'started' }));
    expect(ordinary).not.toContain('data-context-action="Verify ticket"');
    const completed = String(
      TicketRowContextMenu({
        x: 0,
        y: 0,
        status: 'completed',
        upNextEligible: false,
        verifyAction: true,
        notWorkingAction: true,
      }),
    );
    for (const item of COMPLETED_TICKET_CONTEXT_ACTIONS) {
      expect(completed).toContain(`data-context-action="${item.action}"`);
      expect(completed).toContain(`data-lucide="${item.iconName}"`);
    }
    expect(completed.indexOf('data-context-action="Verify ticket"')).toBeLessThan(
      completed.indexOf('data-context-action="Open ticket"'),
    );
    expect(completed).not.toContain('data-context-action="Toggle Up Next"');
    const verifyOnly = String(TicketRowContextMenu({ x: 0, y: 0, verifyAction: true }));
    expect(verifyOnly).toContain('data-context-action="Verify ticket"');
    expect(verifyOnly).not.toContain('data-context-action="Report not working"');
  });

  it('offers a capability-gated Reopen Ticket action for terminal rows', () => {
    const action = `data-context-action="${REOPEN_TICKET_CONTEXT_ACTION.action}"`;
    const verified = String(
      TicketRowContextMenu({ x: 0, y: 0, status: 'verified', upNextEligible: false, reopenAction: true }),
    );
    expect(verified).toContain(action);
    expect(verified).toContain('Reopen Ticket');
    expect(verified).toContain('data-lucide="rotate-ccw"');
    expect(verified.indexOf(action)).toBeLessThan(verified.indexOf('data-context-action="Open ticket"'));
    expect(disabledItem(verified, action)).toBe(false);
    expect(
      disabledItem(
        String(TicketRowContextMenu({ x: 0, y: 0, status: 'archive', reopenAction: true, canBulkUpdate: false })),
        action,
      ),
    ).toBe(true);
    expect(String(TicketRowContextMenu({ x: 0, y: 0, status: 'completed' }))).not.toContain(action);
  });

  it('exposes structured close only when the owning provider supports it', () => {
    const available = String(TicketRowContextMenu({ x: 0, y: 0, closeAction: true }));
    expect(available).toContain(`data-context-action="${CLOSE_TICKET_CONTEXT_ACTION.action}"`);
    expect(available).toContain(`data-lucide="${CLOSE_TICKET_CONTEXT_ACTION.iconName}"`);
    expect(String(TicketRowContextMenu({ x: 0, y: 0 }))).not.toContain('data-context-action="Close ticket"');
  });
});
