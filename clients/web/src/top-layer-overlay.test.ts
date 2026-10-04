import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  openModalDialogs,
  openTopLayerOverlays,
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
