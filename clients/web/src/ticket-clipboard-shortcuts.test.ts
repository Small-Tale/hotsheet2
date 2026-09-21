import { describe, expect, it } from 'vitest';

import { ticketClipboardAction } from './ticket-clipboard-shortcuts';

const base = {
  action: 'copy' as const,
  ticketWorkAreaFocused: true,
  editable: false,
  textSelected: false,
  hasTicketSelection: true,
  hasTicketClipboard: true,
};

describe('ticketClipboardAction', () => {
  it('allows the resolved copy, cut, and paste action only while the ticket work area owns focus', () => {
    expect(ticketClipboardAction(base)).toBe('copy');
    expect(ticketClipboardAction({ ...base, action: 'cut' })).toBe('cut');
    expect(ticketClipboardAction({ ...base, action: 'paste' })).toBe('paste');
    expect(ticketClipboardAction({ ...base, action: undefined })).toBeUndefined();
    expect(ticketClipboardAction({ ...base, ticketWorkAreaFocused: false })).toBeUndefined();
  });

  it('always yields to editable controls and ordinary text selections', () => {
    expect(ticketClipboardAction({ ...base, editable: true })).toBeUndefined();
    expect(ticketClipboardAction({ ...base, textSelected: true })).toBeUndefined();
    expect(ticketClipboardAction({ ...base, action: 'paste', editable: true })).toBeUndefined();
  });

  it('requires the relevant ticket payload', () => {
    expect(ticketClipboardAction({ ...base, hasTicketSelection: false })).toBeUndefined();
    expect(ticketClipboardAction({ ...base, action: 'cut', hasTicketSelection: false })).toBeUndefined();
    expect(ticketClipboardAction({ ...base, action: 'paste', hasTicketClipboard: false })).toBeUndefined();
  });
});
