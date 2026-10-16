import { describe, expect, it } from 'vitest';

import { createToastModalHost, type ToastModalHostEnvironment, type ToastStack } from './toast-modal-host';

/** A DOM stand-in: named parents, one stack, a modal that can open, close, or unmount, and one watcher. */
function fixture() {
  const events: string[] = [];
  let modal: string | null = null,
    callback: (() => void) | undefined;
  const state = { parent: 'body' as string | null, items: 0, open: false };
  const stack: ToastStack = {
    get isConnected() {
      return state.parent !== null;
    },
    get parentElement() {
      return state.parent;
    },
    hasItems: () => state.items > 0,
    isOpen: () => state.open,
    showPopover: () => {
      state.open = true;
      events.push('show');
    },
  };
  const environment: ToastModalHostEnvironment<string> = {
    topmostModal: () => modal,
    body: 'body',
    hostIn: (dialog) => `${dialog}/host`,
    move: (_stack, parent) => {
      // Moving a popover takes it out of the top layer.
      state.parent = parent;
      state.open = false;
      events.push(`move:${parent}`);
    },
    observe: (next) => {
      callback = next;
      events.push('observe');
      return {
        disconnect: () => {
          callback = undefined;
          events.push('disconnect');
        },
      };
    },
  };
  let changes = 0;
  const host = createToastModalHost(stack, environment, () => {
    changes += 1;
  });
  return {
    host,
    state,
    events,
    changes: () => changes,
    /** A toast item appears (Web Awesome shows the stack itself when it creates one). */
    toast() {
      state.items = 1;
      state.open = true;
    },
    dismiss() {
      state.items = 0;
      state.open = false;
    },
    openModal(name: string) {
      modal = name;
      callback?.();
    },
    closeModal() {
      modal = null;
      callback?.();
    },
    /** The dialog leaves the document with the stack still inside it. */
    unmountModal() {
      modal = null;
      if (state.parent?.endsWith('/host')) state.parent = null;
      callback?.();
    },
    watching: () => callback !== undefined,
  };
}

describe('toast stack hosting over modal dialogs (HS2-M2G31B)', () => {
  it('stays on the body and watches only while a toast shows when no modal is open', () => {
    const f = fixture();
    f.host.start();
    expect(f.state.parent).toBe('body');
    expect(f.events).toEqual(['observe']);
    f.host.stop();
    expect(f.watching()).toBe(false);
    expect(f.host.watching()).toBe(false);
  });

  it('hosts a toast raised over an open modal inside that modal', () => {
    const f = fixture();
    f.openModal('gallery');
    f.host.start();
    expect(f.state.parent).toBe('gallery/host');
    // The stack had no items yet: Web Awesome shows it when it creates the toast.
    expect(f.events).toEqual(['move:gallery/host', 'observe']);
  });

  it('moves a toast that is already showing into a modal that opens, re-shows it, and re-measures', () => {
    const f = fixture();
    f.host.start();
    f.toast();
    f.openModal('gallery');
    expect(f.state.parent).toBe('gallery/host');
    expect(f.state.open).toBe(true);
    expect(f.events.slice(-2)).toEqual(['move:gallery/host', 'show']);
    expect(f.changes()).toBe(1);
    // An unrelated mutation while nothing changed neither moves nor re-measures.
    f.openModal('gallery');
    expect(f.changes()).toBe(1);
  });

  it('returns a showing toast to the body when its modal closes or unmounts', () => {
    for (const leave of ['closeModal', 'unmountModal'] as const) {
      const f = fixture();
      f.openModal('gallery');
      f.host.start();
      f.toast();
      f[leave]();
      expect(f.state.parent, leave).toBe('body');
      expect(f.state.open, leave).toBe(true);
      expect(f.changes(), leave).toBe(1);
      expect(f.watching(), leave).toBe(true);
    }
  });

  it('after the toast hides, keeps the stack in its open modal and returns it when the modal goes', () => {
    const f = fixture();
    f.openModal('gallery');
    f.host.start();
    f.toast();
    f.dismiss();
    f.host.stop();
    // Still inside the open gallery, so the exit animation is not cut short; still watching.
    expect(f.state.parent).toBe('gallery/host');
    expect(f.watching()).toBe(true);
    f.unmountModal();
    expect(f.state.parent).toBe('body');
    expect(f.state.open).toBe(false);
    expect(f.watching()).toBe(false);
  });

  it('an inactive stack never follows a different modal, and a later toast re-homes a detached stack', () => {
    const f = fixture();
    f.openModal('gallery');
    f.host.start();
    f.dismiss();
    f.host.stop();
    // A second modal opening while no toast shows sends the stack home instead of following it.
    f.openModal('settings');
    expect(f.state.parent).toBe('body');
    expect(f.watching()).toBe(false);
    // The next toast moves it into the modal that is open now.
    f.host.start();
    expect(f.state.parent).toBe('settings/host');
    // A stack detached by an unobserved removal is reattached by the next start.
    f.host.stop();
    f.state.parent = null;
    f.closeModal();
    f.host.start();
    expect(f.state.parent).toBe('body');
  });

  it('a stack returning home after its toast hid does not re-measure, and dispose stops watching', () => {
    const f = fixture();
    f.openModal('gallery');
    f.host.start();
    f.host.stop();
    f.closeModal();
    expect(f.state.parent).toBe('body');
    expect(f.changes()).toBe(0);
    f.openModal('gallery');
    f.host.start();
    f.host.dispose();
    expect(f.watching()).toBe(false);
    expect(f.host.watching()).toBe(false);
  });

  it('repeated start and stop keep one watcher', () => {
    const f = fixture();
    f.host.start();
    f.host.start();
    expect(f.events.filter((event) => event === 'observe')).toHaveLength(1);
    f.host.stop();
    f.host.stop();
    expect(f.events.filter((event) => event === 'disconnect')).toHaveLength(1);
  });
});
