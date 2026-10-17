import { describe, expect, it, vi } from 'vitest';

import { showTicketReaderDialog, type TicketReaderDialogElement } from './ticket-reader';

/** A shadow `<dialog>` fake with Web Awesome 3.14's own label (HS2-BH7NZ4). */
function nativeDialog(open: boolean) {
  const attributes = new Map([['aria-label', 'Read and edit HS2-1']]);
  return {
    open,
    attributes,
    getAttribute: (name: string) => attributes.get(name) ?? null,
    hasAttribute: (name: string) => attributes.has(name),
    setAttribute: vi.fn((name: string, value: string) => attributes.set(name, value)),
    removeAttribute: (name: string) => attributes.delete(name),
  };
}

describe('showTicketReaderDialog', () => {
  it('repairs a declarative open host whose native dialog has not opened', () => {
    vi.stubGlobal('CSS', { escape: (value: string) => value });
    const show = vi.fn(),
      native = nativeDialog(false),
      dialog = {
        open: true,
        shadowRoot: { querySelector: () => native },
        show,
      } as unknown as TicketReaderDialogElement,
      root = { querySelector: () => dialog } as unknown as ParentNode;
    expect(showTicketReaderDialog(root, 'reader')).toBe(true);
    expect(show).toHaveBeenCalledOnce();
    expect(native.setAttribute).toHaveBeenCalledWith('role', 'presentation');
    // The host stays the one named dialog: Web Awesome's inner label is stripped.
    expect(native.hasAttribute('aria-label')).toBe(false);
  });
  it('does not reopen an already visible native dialog', () => {
    vi.stubGlobal('CSS', { escape: (value: string) => value });
    const show = vi.fn(),
      native = nativeDialog(true),
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
