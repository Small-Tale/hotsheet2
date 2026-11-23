import { describe, expect, it, vi } from 'vitest';

import { CONTROLLED_OPEN_ATTRIBUTE, syncControlledOpen } from './controlled-open';

vi.mock('@kerfjs/ui/surface-scaffold', () => ({ openDialog: vi.fn(), closeDialog: vi.fn() }));

describe('Web Awesome dialog reopened while it hides (HS2-6TJRYX)', () => {
  /** Mirrors wa-dialog's requestClose: `open` stays true through the hide, then the dialog closes. */
  class HidingDialog extends HTMLElement {
    open = true;
    readonly native: HTMLDialogElement;
    constructor() {
      super();
      this.native = this.attachShadow({ mode: 'open' }).appendChild(document.createElement('dialog'));
      this.native.setAttribute('open', '');
    }
    startHide() {
      this.native.classList.add('hide');
    }
    finishHide() {
      this.native.classList.remove('hide');
      this.open = false;
      this.native.removeAttribute('open');
      this.dispatchEvent(new Event('wa-after-hide', { bubbles: true, composed: true }));
    }
  }
  if (!customElements.get('wa-dialog')) customElements.define('wa-dialog', HidingDialog);
  const hidingDialog = (state: string) => {
    const dialog = document.createElement('wa-dialog') as unknown as HidingDialog;
    dialog.setAttribute(CONTROLLED_OPEN_ATTRIBUTE, state);
    document.body.append(dialog);
    return dialog;
  };

  it('reopens a dialog declared open again during its hide once the hide finishes', () => {
    const dialog = hidingDialog('true');
    dialog.startHide();
    syncControlledOpen(document.body);
    syncControlledOpen(document.body);
    // The live property already matches, so the sync alone cannot keep it open.
    expect(dialog.open).toBe(true);
    dialog.finishHide();
    expect(dialog.open).toBe(true);
    // One re-assertion only: a later hide without a new declaration stays closed.
    dialog.startHide();
    dialog.finishHide();
    expect(dialog.open).toBe(false);
    dialog.remove();
  });

  it('leaves the dialog closed when a close is declared before the hide finishes', () => {
    const dialog = hidingDialog('true');
    dialog.startHide();
    syncControlledOpen(document.body);
    dialog.setAttribute(CONTROLLED_OPEN_ATTRIBUTE, 'false');
    dialog.finishHide();
    expect(dialog.open).toBe(false);
    dialog.remove();
  });

  it('ignores a nested popup finishing its own hide, and an ordinary open dialog', () => {
    const dialog = hidingDialog('true');
    syncControlledOpen(document.body);
    dialog.startHide();
    dialog.open = false;
    dialog.finishHide();
    expect(dialog.open).toBe(false);
    dialog.open = true;
    dialog.native.setAttribute('open', '');
    dialog.startHide();
    syncControlledOpen(document.body);
    const nested = dialog.appendChild(document.createElement('span'));
    nested.dispatchEvent(new Event('wa-after-hide', { bubbles: true }));
    expect(dialog.open).toBe(true);
    dialog.finishHide();
    expect(dialog.open).toBe(true);
    dialog.remove();
  });
});
