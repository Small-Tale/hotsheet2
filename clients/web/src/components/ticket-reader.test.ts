import { describe, expect, it, vi } from 'vitest';

import { showTicketReaderDialog, type TicketReaderDialogElement } from './ticket-reader';

describe('showTicketReaderDialog', () => {
  it('repairs a declarative open host whose native dialog has not opened', () => {
    vi.stubGlobal('CSS', { escape: (value: string) => value });
    const show = vi.fn(),
      native = { open: false, setAttribute: vi.fn() },
      dialog = {
        open: true,
        shadowRoot: { querySelector: () => native },
        show,
      } as unknown as TicketReaderDialogElement,
      root = { querySelector: () => dialog } as unknown as ParentNode;
    expect(showTicketReaderDialog(root, 'reader')).toBe(true);
    expect(show).toHaveBeenCalledOnce();
    expect(native.setAttribute).toHaveBeenCalledWith('role', 'presentation');
  });
  it('does not reopen an already visible native dialog', () => {
    vi.stubGlobal('CSS', { escape: (value: string) => value });
    const show = vi.fn(),
      native = { open: true, setAttribute: vi.fn() },
      dialog = {
        open: true,
        shadowRoot: { querySelector: () => native },
        show,
      } as unknown as TicketReaderDialogElement,
      root = { querySelector: () => dialog } as unknown as ParentNode;
    expect(showTicketReaderDialog(root, 'reader')).toBe(false);
    expect(show).not.toHaveBeenCalled();
  });
});
