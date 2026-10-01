import { afterEach, describe, expect, it, vi } from 'vitest';

import { openTopLayerOverlays, TOP_LAYER_OVERLAY_ATTRIBUTE, wireTopLayerOverlays } from './top-layer-overlay';

/** A marked manual-popover overlay stand-in whose open state the test controls (no DOM in unit tests). */
function overlay({ open = false, connected = true } = {}) {
  let isOpen = open;
  const element = {
    isConnected: connected,
    matches: (selector: string) => selector === ':popover-open' && isOpen,
    showPopover: vi.fn(() => {
      isOpen = true;
    }),
  };
  return element;
}

type FakeOverlay = ReturnType<typeof overlay>;

function root(overlays: FakeOverlay[]) {
  const selectors: string[] = [];
  return {
    selectors,
    overlays,
    querySelectorAll(selector: string) {
      selectors.push(selector);
      return this.overlays;
    },
  };
}

describe('top-layer overlays (HS2-Z9PQSC)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('opens marked, closed, connected overlays once and skips open or detached ones', () => {
    const closed = overlay(),
      alreadyOpen = overlay({ open: true }),
      detached = overlay({ connected: false }),
      container = root([closed, alreadyOpen, detached]);
    openTopLayerOverlays(container as unknown as ParentNode);
    openTopLayerOverlays(container as unknown as ParentNode);
    expect(container.selectors[0]).toBe(`[${TOP_LAYER_OVERLAY_ATTRIBUTE}][popover]`);
    expect(closed.showPopover).toHaveBeenCalledTimes(1);
    expect(alreadyOpen.showPopover).not.toHaveBeenCalled();
    expect(detached.showPopover).not.toHaveBeenCalled();
  });

  it('opens overlays that render after wiring, again after a remove-and-refill, and stops after disposal', () => {
    let notify: (() => void) | undefined;
    const disconnect = vi.fn(() => {
      notify = undefined;
    });
    vi.stubGlobal(
      'MutationObserver',
      class {
        constructor(callback: () => void) {
          notify = callback;
        }
        observe = vi.fn();
        disconnect = disconnect;
      },
    );
    const container = root([]),
      dispose = wireTopLayerOverlays(container as unknown as HTMLElement),
      first = overlay();
    container.overlays = [first];
    notify?.();
    expect(first.showPopover).toHaveBeenCalledTimes(1);
    const second = overlay();
    container.overlays = [second];
    notify?.();
    expect(second.showPopover).toHaveBeenCalledTimes(1);
    dispose();
    expect(disconnect).toHaveBeenCalledTimes(1);
    const third = overlay();
    container.overlays = [third];
    notify?.();
    expect(third.showPopover).not.toHaveBeenCalled();
  });
});
