import { describe, expect, it } from 'vitest';

import { ticketClipboardAction } from './ticket-clipboard-shortcuts';

const base = { key: 'c', command: true, ticketWorkAreaFocused: true, editable: false, textSelected: false, hasTicketSelection: true, hasTicketClipboard: true };

describe('ticketClipboardAction', () => {
  it('allows ticket copy, cut, and paste only while the ticket work area owns focus', () => {
    expect(ticketClipboardAction(base)).toBe('copy');
    expect(ticketClipboardAction({ ...base, key: 'x' })).toBe('cut');
    expect(ticketClipboardAction({ ...base, key: 'v' })).toBe('paste');
    expect(ticketClipboardAction({ ...base, ticketWorkAreaFocused: false })).toBeUndefined();
  });

  it('always yields to editable controls and ordinary text selections', () => {
    expect(ticketClipboardAction({ ...base, editable: true })).toBeUndefined();
    expect(ticketClipboardAction({ ...base, textSelected: true })).toBeUndefined();
    expect(ticketClipboardAction({ ...base, key: 'v', editable: true })).toBeUndefined();
  });

  it('requires the relevant ticket payload', () => {
    expect(ticketClipboardAction({ ...base, hasTicketSelection: false })).toBeUndefined();
    expect(ticketClipboardAction({ ...base, key: 'x', hasTicketSelection: false })).toBeUndefined();
    expect(ticketClipboardAction({ ...base, key: 'v', hasTicketClipboard: false })).toBeUndefined();
  });
});
