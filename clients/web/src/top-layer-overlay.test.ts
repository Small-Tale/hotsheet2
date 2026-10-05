import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  dismissLiftedOverlay,
  openModalDialogs,
  openTopLayerOverlays,
  TOP_LAYER_DISMISS_ATTRIBUTE,
  TOP_LAYER_OVERLAY_ATTRIBUTE,
  wireTopLayerOverlays,
} from './top-layer-overlay';

type Layer = 'closed' | 'popover' | 'modal';

/**
 * A marked overlay stand-in whose top-layer state the test controls (no DOM in unit tests). `dialog`
 * gives it the `showModal()`/`close()` of a `<dialog>`; a plain element only has the popover API.
 */
function overlay({
  layer = 'closed',
  connected = true,
  dialog = false,
}: { layer?: Layer; connected?: boolean; dialog?: boolean } = {}) {
  const element = {
    layer,
    inert: false,
    /** Whether the overlay was `inert` at each `showPopover()`, when a `<dialog>` would focus a child. */
    inertAtShow: [] as boolean[],
    calls: [] as string[],
    isConnected: connected,
    localName: dialog ? 'dialog' : 'div',
    hasAttribute: (name: string) => name === TOP_LAYER_OVERLAY_ATTRIBUTE,
    focus: vi.fn(() => {
      element.calls.push('focus');
    }),
    matches: (selector: string) =>
      (selector === ':popover-open' && element.layer === 'popover') ||
      (selector === ':modal' && element.layer === 'modal'),
    showPopover: vi.fn(() => {
      element.inertAtShow.push(element.inert);
      element.calls.push('showPopover');
      element.layer = 'popover';
    }),
    hidePopover: vi.fn(() => {
      element.calls.push('hidePopover');
      element.layer = 'closed';
    }),
    ...(dialog
      ? {
          showModal: vi.fn(() => {
            element.calls.push('showModal');
            element.layer = 'modal';
          }),
          close: vi.fn(() => {
            element.calls.push('close');
            element.layer = 'closed';
          }),
        }
      : {}),
  };
  return element;
}

/** A foreign modal: a native `<dialog>` or a Web Awesome host whose dialog is in its shadow root. */
function modalHost({ kind = 'wa-dialog', modal = true } = {}) {
  const dialog = { matches: (selector: string) => selector === ':modal' && host.modal };
  const host = {
    modal,
    localName: kind,
    hasAttribute: () => false,
    matches: (selector: string) => selector === ':modal' && host.modal,
    shadowRoot: kind === 'dialog' ? null : { querySelector: () => dialog },
  };
  return host;
}

type FakeOverlay = ReturnType<typeof overlay>;
type FakeModal = ReturnType<typeof modalHost>;

function root(overlays: FakeOverlay[], modals: FakeModal[] = []) {
  const selectors: string[] = [],
    listeners = new Map<string, ((event: unknown) => void)[]>();
  return {
    selectors,
    overlays,
    modals,
    listeners,
    ownerDocument: { activeElement: null as Element | null },
    querySelectorAll(selector: string) {
      selectors.push(selector);
      return selector.includes(TOP_LAYER_OVERLAY_ATTRIBUTE) ? this.overlays : [...this.overlays, ...this.modals];
    },
    addEventListener: vi.fn((type: string, listener: (event: unknown) => void) => {
      listeners.set(type, [...(listeners.get(type) ?? []), listener]);
    }),
    removeEventListener: vi.fn((type: string, listener: (event: unknown) => void) => {
      listeners.set(
        type,
        (listeners.get(type) ?? []).filter((candidate) => candidate !== listener),
      );
    }),
    dispatch(type: string, event: unknown = {}) {
      for (const listener of listeners.get(type) ?? []) listener(event);
    },
  };
}

function open(container: ReturnType<typeof root>) {
  openTopLayerOverlays(container as unknown as ParentNode);
}

describe('top-layer overlays (HS2-Z9PQSC)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('opens marked, closed, connected overlays once and skips open or detached ones', () => {
    const closed = overlay(),
      alreadyOpen = overlay({ layer: 'popover' }),
      detached = overlay({ connected: false }),
      container = root([closed, alreadyOpen, detached]);
    open(container);
    open(container);
    expect(container.selectors[0]).toBe(`[${TOP_LAYER_OVERLAY_ATTRIBUTE}][popover]`);
    expect(closed.showPopover).toHaveBeenCalledTimes(1);
    expect(alreadyOpen.showPopover).not.toHaveBeenCalled();
    expect(detached.showPopover).not.toHaveBeenCalled();
  });

  it('opens overlays that render after wiring, again after a remove-and-refill, and stops after disposal', () => {
    let notify: (() => void) | undefined;
    const observe = vi.fn(),
      disconnect = vi.fn(() => {
        notify = undefined;
      });
    vi.stubGlobal(
      'MutationObserver',
      class {
        constructor(callback: () => void) {
          notify = callback;
        }
        observe = observe;
        disconnect = disconnect;
      },
    );
    const container = root([]),
      dispose = wireTopLayerOverlays(container as unknown as HTMLElement),
      first = overlay();
    // A Web Awesome modal toggles only its host's `open` attribute, so the watcher observes it too.
    expect(observe).toHaveBeenCalledWith(container, {
      childList: true,
      subtree: true,
      attributes: true,
      attributeFilter: ['open'],
    });
    container.overlays = [first];
    notify?.();
    expect(first.showPopover).toHaveBeenCalledTimes(1);
    const second = overlay();
    container.overlays = [second];
    notify?.();
    expect(second.showPopover).toHaveBeenCalledTimes(1);
    dispose();
    expect(disconnect).toHaveBeenCalledTimes(1);
    for (const listeners of container.listeners.values()) expect(listeners).toHaveLength(0);
    const third = overlay();
    container.overlays = [third];
    notify?.();
    expect(third.showPopover).not.toHaveBeenCalled();
  });
});

describe('top-layer overlays above modal dialogs (HS2-MAE27T)', () => {
  it('defers a permission lift while a shadow-root form field has focus, then lifts after focus leaves', async () => {
    vi.stubGlobal(
      'MutationObserver',
      class {
        observe = vi.fn();
        disconnect = vi.fn();
      },
    );
    const popup = overlay({ dialog: true }),
      container = root([popup], [modalHost()]),
      input = { matches: (selector: string) => selector.includes('input'), isContentEditable: false },
      host = {
        matches: () => false,
        isContentEditable: false,
        shadowRoot: { activeElement: input },
      };
    container.ownerDocument.activeElement = host as unknown as Element;
    const dispose = wireTopLayerOverlays(container as unknown as HTMLElement);
    expect(popup.calls).toEqual([]);
    container.dispatch('focusout');
    await Promise.resolve();
    expect(popup.calls).toEqual([]);
    container.ownerDocument.activeElement = null;
    container.dispatch('focusout');
    await Promise.resolve();
    expect(popup.calls).toEqual(['showModal', 'focus']);
    dispose();
    vi.unstubAllGlobals();
  });

  it('does not re-lift an existing popup over a newer modal while its text field is focused', () => {
    const popup = overlay({ dialog: true }),
      container = root([popup], [modalHost()]);
    open(container);
    const initialCalls = popup.calls.length;
    container.modals.push(modalHost({ kind: 'dialog' }));
    container.ownerDocument.activeElement = {
      matches: (selector: string) => selector.includes('textarea'),
      isContentEditable: false,
      shadowRoot: null,
    } as unknown as Element;
    open(container);
    expect(popup.calls).toHaveLength(initialCalls);
    container.ownerDocument.activeElement = null;
    open(container);
    expect(popup.calls.slice(initialCalls)).toEqual(['close', 'showModal', 'focus']);
  });

  it('finds open native and Web Awesome modal dialogs but never a marked overlay', () => {
    const lifted = overlay({ dialog: true, layer: 'modal' }),
      native = modalHost({ kind: 'dialog' }),
      drawer = modalHost({ kind: 'wa-drawer' }),
      closedNative = modalHost({ kind: 'dialog', modal: false }),
      container = root([lifted], [native, drawer, closedNative]);
    expect(openModalDialogs(container as unknown as ParentNode)).toEqual([native, drawer]);
    expect(container.selectors).toEqual(['dialog, wa-dialog[open], wa-drawer[open]']);
  });

  it('walks popover → modal → re-lifted modal → popover as modals open, stack, and close', () => {
    const popup = overlay({ dialog: true }),
      container = root([popup]);
    // No modal: an ordinary, non-blocking popover.
    open(container);
    expect(popup.layer).toBe('popover');
    // The Create ticket dialog opens: hide the popover and lift the popup as a modal above it.
    const composer = modalHost();
    container.modals = [composer];
    open(container);
    expect(popup.calls).toEqual(['showPopover', 'hidePopover', 'showModal', 'focus']);
    // Repeated mutations while the same modal stays open never re-lift (no focus churn).
    open(container);
    open(container);
    expect(popup.calls).toHaveLength(4);
    // A newer modal opens above the popup: close and re-lift so the popup stays topmost.
    const nested = modalHost({ kind: 'dialog' });
    container.modals = [composer, nested];
    open(container);
    expect(popup.calls.slice(4)).toEqual(['close', 'showModal', 'focus']);
    // The newer modal closes: the popup already sits above the remaining one.
    container.modals = [composer];
    open(container);
    expect(popup.calls).toHaveLength(7);
    // The last modal closes: return to a non-blocking popover.
    container.modals = [];
    open(container);
    expect(popup.calls.slice(7)).toEqual(['close', 'showPopover']);
    expect(popup.layer).toBe('popover');
    // Empty-then-refill: a fresh modal lifts the popup again rather than trusting the stale record.
    container.modals = [modalHost()];
    open(container);
    expect(popup.calls.slice(9)).toEqual(['hidePopover', 'showModal', 'focus']);
  });

  it('lifts a popup that first renders while a modal is already open, without a popover detour', () => {
    const popup = overlay({ dialog: true }),
      container = root([popup], [modalHost()]);
    open(container);
    expect(popup.calls).toEqual(['showModal', 'focus']);
  });

  it('opens a non-modal popup inert so it never takes focus, then restores its inert state (HS2-HZK70N)', () => {
    const popup = overlay({ dialog: true }),
      alreadyInert = overlay({ dialog: true }),
      container = root([popup, alreadyInert]);
    alreadyInert.inert = true;
    open(container);
    expect(popup.calls).toEqual(['showPopover']);
    expect(popup.inertAtShow).toEqual([true]);
    expect(popup.inert).toBe(false);
    expect(alreadyInert.inertAtShow).toEqual([true]);
    expect(alreadyInert.inert).toBe(true);
    expect(popup.focus).not.toHaveBeenCalled();
  });

  it('keeps a non-dialog overlay a popover even over a modal', () => {
    const magnified = overlay(),
      container = root([magnified], [modalHost()]);
    open(container);
    expect(magnified.layer).toBe('popover');
    expect(container.selectors).toEqual([`[${TOP_LAYER_OVERLAY_ATTRIBUTE}][popover]`]);
  });

  it('reopens after Web Awesome lifecycle events and a forced close, and blocks Escape cancel', async () => {
    vi.stubGlobal(
      'MutationObserver',
      class {
        observe = vi.fn();
        disconnect = vi.fn();
      },
    );
    const popup = overlay({ dialog: true }),
      container = root([popup]),
      dispose = wireTopLayerOverlays(container as unknown as HTMLElement);
    expect(popup.layer).toBe('popover');
    // `wa-show` fires just before Web Awesome calls showModal(), so the lift waits a microtask.
    container.modals = [modalHost()];
    container.dispatch('wa-show');
    expect(popup.layer).toBe('popover');
    await Promise.resolve();
    expect(popup.layer).toBe('modal');
    const cancelOverlay = { target: popup, preventDefault: vi.fn() },
      cancelOther = { target: { hasAttribute: () => false }, preventDefault: vi.fn() };
    container.dispatch('cancel', cancelOverlay);
    container.dispatch('cancel', cancelOther);
    expect(cancelOverlay.preventDefault).toHaveBeenCalledTimes(1);
    expect(cancelOther.preventDefault).not.toHaveBeenCalled();
    // A forced close (repeated Escape) still reopens the popup above the modal.
    popup.layer = 'closed';
    container.dispatch('close');
    await Promise.resolve();
    expect(popup.layer).toBe('modal');
    // The modal finishes hiding: back to a popover.
    container.modals = [];
    container.dispatch('wa-after-hide');
    await Promise.resolve();
    expect(popup.layer).toBe('popover');
    dispose();
    vi.unstubAllGlobals();
  });
});

describe('Escape over a lifted overlay (HS2-S8K9BG)', () => {
  function key(init: { key?: string; isComposing?: boolean } = {}) {
    return {
      key: init.key ?? 'Escape',
      isComposing: init.isComposing ?? false,
      preventDefault: vi.fn(),
      stopPropagation: vi.fn(),
    };
  }
  function withDismiss(element: FakeOverlay) {
    const dismiss = { click: vi.fn() },
      selectors: string[] = [];
    Object.assign(element, {
      querySelector: (selector: string) => {
        selectors.push(selector);
        return dismiss;
      },
    });
    return { dismiss, selectors };
  }
  const dismissKey = (container: ReturnType<typeof root>, event: ReturnType<typeof key>) =>
    dismissLiftedOverlay(container as unknown as ParentNode, event as unknown as KeyboardEvent);

  it('swallows Escape and activates the dismiss control of the topmost lifted overlay', () => {
    const below = overlay({ dialog: true, layer: 'modal' }),
      top = overlay({ dialog: true, layer: 'modal' }),
      belowDismiss = withDismiss(below),
      topDismiss = withDismiss(top),
      event = key();
    expect(dismissKey(root([below, top]), event)).toBe(true);
    // Stopped before Web Awesome's document handler closes the modal beneath, and before `cancel`.
    expect(event.preventDefault).toHaveBeenCalledTimes(1);
    expect(event.stopPropagation).toHaveBeenCalledTimes(1);
    expect(topDismiss.selectors).toEqual([`[${TOP_LAYER_DISMISS_ATTRIBUTE}]`]);
    expect(topDismiss.dismiss.click).toHaveBeenCalledTimes(1);
    expect(belowDismiss.dismiss.click).not.toHaveBeenCalled();
  });

  it('still swallows Escape for a lifted overlay without a dismiss control', () => {
    const lifted = overlay({ dialog: true, layer: 'modal' }),
      event = key();
    Object.assign(lifted, { querySelector: () => null });
    expect(dismissKey(root([lifted]), event)).toBe(true);
    expect(event.stopPropagation).toHaveBeenCalledTimes(1);
  });

  it('leaves Escape alone when no overlay is lifted, and other keys or IME composition always', () => {
    const popover = overlay({ dialog: true, layer: 'popover' }),
      { dismiss } = withDismiss(popover),
      idle = key();
    // A plain popover never blocks a modal, so Escape still closes whatever modal is open.
    expect(dismissKey(root([popover]), idle)).toBe(false);
    expect(idle.preventDefault).not.toHaveBeenCalled();
    expect(idle.stopPropagation).not.toHaveBeenCalled();
    const lifted = overlay({ dialog: true, layer: 'modal' }),
      liftedDismiss = withDismiss(lifted),
      enter = key({ key: 'Enter' }),
      composing = key({ isComposing: true });
    expect(dismissKey(root([lifted]), enter)).toBe(false);
    expect(dismissKey(root([lifted]), composing)).toBe(false);
    expect(composing.stopPropagation).not.toHaveBeenCalled();
    expect(dismiss.click).not.toHaveBeenCalled();
    expect(liftedDismiss.dismiss.click).not.toHaveBeenCalled();
  });

  it('routes keydown through a capture listener and removes it on disposal', () => {
    vi.stubGlobal(
      'MutationObserver',
      class {
        observe = vi.fn();
        disconnect = vi.fn();
      },
    );
    const lifted = overlay({ dialog: true, layer: 'modal' }),
      { dismiss } = withDismiss(lifted),
      container = root([lifted], [modalHost()]),
      dispose = wireTopLayerOverlays(container as unknown as HTMLElement);
    expect(container.addEventListener).toHaveBeenCalledWith('keydown', expect.any(Function), true);
    container.dispatch('keydown', key());
    expect(dismiss.click).toHaveBeenCalledTimes(1);
    dispose();
    expect(container.removeEventListener).toHaveBeenCalledWith('keydown', expect.any(Function), true);
    container.dispatch('keydown', key());
    expect(dismiss.click).toHaveBeenCalledTimes(1);
    vi.unstubAllGlobals();
  });
});
