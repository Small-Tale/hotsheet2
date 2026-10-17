import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  contextMenuAnchor,
  type ContextPopupMenuElement,
  maintainContextPopupMenuAnchor,
  reanchorReplacedContextPopupMenus,
  viewportSafeContextMenuPosition,
  viewportSafePointerPosition,
} from './context-menu-position';

afterEach(() => vi.unstubAllGlobals());

/**
 * A fake Kerf PopupMenu root (Kerf UI 5.0.0-beta.106): `openPopupMenu` dispatches an open request the
 * register answers by marking the opener `aria-expanded="true"`, which `isPopupMenuOpen` reads.
 */
function fakeMenuRoot() {
  const opener = {
    expanded: false,
    getAttribute: (name: string) => (name === 'aria-expanded' ? String(opener.expanded) : null),
  };
  return {
    opener,
    querySelector: () => opener,
    dispatchEvent: (event: Event) => {
      if (event.type === 'kui-popup-menu-open-request') opener.expanded = true;
      if (event.type === 'kui-popup-menu-close-request') opener.expanded = false;
      return true;
    },
  };
}

describe('maintainContextPopupMenuAnchor', () => {
  it('restores lost anchors and replacement hosts but never reopens a dismissed menu', () => {
    let changed = () => {};
    const disconnect = vi.fn();
    class FakeMutationObserver {
      constructor(callback: () => void) {
        changed = callback;
      }
      observe() {}
      disconnect() {
        disconnect();
      }
    }
    const listeners = new Map<string, (event: Event) => void>();
    vi.stubGlobal('MutationObserver', FakeMutationObserver);
    vi.stubGlobal('document', {
      body: {},
      addEventListener: (name: string, handler: (event: Event) => void) => listeners.set(name, handler),
      removeEventListener: (name: string) => listeners.delete(name),
    });
    const makeMenu = () => {
      const properties = new Map<string, string>(),
        root = fakeMenuRoot();
      return {
        ...root,
        get open() {
          return root.opener.expanded;
        },
        set open(value: boolean) {
          root.opener.expanded = value;
        },
        isConnected: true,
        style: {
          getPropertyValue: (name: string) => properties.get(name) ?? '',
          setProperty: (name: string, value: string) => properties.set(name, value),
        },
        getAttribute: () => [...properties].map(([name, value]) => `${name}:${value}`).join(';') || null,
        properties,
      };
    };
    const first = makeMenu(),
      second = makeMenu();
    let current = first;
    const root = { querySelector: () => current } as unknown as ParentNode;
    const stop = maintainContextPopupMenuAnchor('[data-inspector-status-menu]', { x: 418, y: 267 }, () => true, root);
    expect(first.open).toBe(true);
    expect(first.getAttribute()).toContain('418px');
    first.properties.clear();
    changed();
    expect(first.getAttribute()).toContain('267px');
    current = second;
    changed();
    expect(second.open).toBe(true);
    second.open = false;
    changed();
    expect(second.open).toBe(false);
    expect(disconnect).toHaveBeenCalledOnce();
    expect(listeners.has('toggle')).toBe(false);
    stop();
  });
  it('stops observing once the observed menu panel closes itself (its popover toggle)', () => {
    const disconnect = vi.fn();
    vi.stubGlobal(
      'MutationObserver',
      class {
        observe() {}
        disconnect() {
          disconnect();
        }
      },
    );
    const listeners = new Map<string, (event: Event) => void>();
    vi.stubGlobal('document', {
      body: {},
      addEventListener: (name: string, handler: (event: Event) => void) => listeners.set(name, handler),
      removeEventListener: (name: string) => listeners.delete(name),
    });
    class FakeElement {
      readonly fake = true;
    }
    vi.stubGlobal('HTMLElement', FakeElement);
    const menu = Object.assign(new FakeElement(), fakeMenuRoot(), {
      isConnected: true,
      style: { setProperty: () => undefined },
      getAttribute: () => null,
      matches: (selector: string) => selector === '.kui-popup-menu',
    });
    const panel = Object.assign(new FakeElement(), {
      parentElement: menu,
      matches: (selector: string) => selector === '.kui-popup-menu__menu',
    });
    const root = { querySelector: () => menu } as unknown as ParentNode;
    maintainContextPopupMenuAnchor('[data-inspector-status-menu]', { x: 1, y: 2 }, () => true, root);
    expect(menu.opener.expanded).toBe(true);
    // An opening toggle is not a dismissal.
    listeners.get('toggle')!({ type: 'toggle', newState: 'open', target: panel } as unknown as Event);
    expect(disconnect).not.toHaveBeenCalled();
    listeners.get('toggle')!({ type: 'toggle', newState: 'closed', target: panel } as unknown as Event);
    expect(disconnect).toHaveBeenCalledOnce();
    expect(listeners.has('toggle')).toBe(false);
  });
});

describe('reanchorReplacedContextPopupMenus', () => {
  it('opens each new or re-rendered host once and resets after dismissal (HS2-H4JZDQ)', () => {
    // Kerf renders the context anchor into the menu's inline style (KF-FXGW0C), so a changed style
    // or a replacement host reopens; an unchanged one is left alone.
    const makeMenu = (style: string) => {
      const state = { style, opens: 0 };
      const menu = {
        querySelector: () => null,
        dispatchEvent: (event: Event) => {
          if (event.type === 'kui-popup-menu-open-request') state.opens += 1;
          return true;
        },
        getAttribute: (name: string) => (name === 'style' ? state.style : null),
      } as unknown as ContextPopupMenuElement;
      return { menu, state };
    };
    const first = makeMenu('anchor-a'),
      second = makeMenu('anchor-a'),
      opened = new Map<string, ContextPopupMenuElement>();
    let current = first.menu;
    const root = { querySelector: () => current } as unknown as ParentNode;

    reanchorReplacedContextPopupMenus(['ticket'], opened, root);
    expect(first.state.opens).toBe(1);
    reanchorReplacedContextPopupMenus(['ticket'], opened, root);
    expect(first.state.opens).toBe(1);
    first.state.style = 'anchor-b';
    reanchorReplacedContextPopupMenus(['ticket'], opened, root);
    expect(first.state.opens).toBe(2);

    current = second.menu;
    reanchorReplacedContextPopupMenus(['ticket'], opened, root);
    expect(second.state.opens).toBe(1);
    reanchorReplacedContextPopupMenus(['ticket'], opened, root);
    expect(second.state.opens).toBe(1);

    reanchorReplacedContextPopupMenus([], opened, root);
    expect(opened.size).toBe(0);
    reanchorReplacedContextPopupMenus(['ticket'], opened, root);
    expect(second.state.opens).toBe(2);
  });

  it('skips a surface whose menu is not rendered', () => {
    const opened = new Map<string, ContextPopupMenuElement>();
    reanchorReplacedContextPopupMenus(['ticket'], opened, { querySelector: () => null } as unknown as ParentNode);
    expect(opened.size).toBe(0);
  });
});

describe('viewportSafeContextMenuPosition', () => {
  it('keeps an ordinary pointer position unchanged', () => {
    expect(viewportSafeContextMenuPosition(200, 150, 1000, 700, { width: 224, height: 208 })).toEqual({
      x: 200,
      y: 150,
    });
  });
  it('flips the menu inward at the bottom-right viewport edge', () => {
    expect(viewportSafeContextMenuPosition(998, 698, 1000, 700, { width: 224, height: 208 })).toEqual({
      x: 768,
      y: 484,
    });
  });
  it('honors the safe margin at negative coordinates and tiny viewports', () => {
    expect(viewportSafeContextMenuPosition(-20, -30, 1000, 700, { width: 224, height: 208 })).toEqual({ x: 8, y: 8 });
    expect(viewportSafeContextMenuPosition(40, 30, 180, 120, { width: 224, height: 208 })).toEqual({ x: 8, y: 8 });
  });
});

describe('viewportSafePointerPosition', () => {
  it('preserves a pointer at the viewport edge for measured popup collision handling', () => {
    expect(viewportSafePointerPosition(998, 698, 1000, 700)).toEqual({ x: 998, y: 698 });
    expect(viewportSafePointerPosition(Number.NaN, Number.POSITIVE_INFINITY, 1000, 700)).toEqual({ x: 0, y: 0 });
  });
  it('only clamps coordinates which are actually outside the viewport', () => {
    expect(viewportSafePointerPosition(-20, 730, 1000, 700)).toEqual({ x: 0, y: 700 });
  });
});

describe('contextMenuAnchor', () => {
  it('passes finite points through and replaces missing coordinates with 0', () => {
    expect(contextMenuAnchor(12.5, 40)).toEqual({ x: 12.5, y: 40 });
    expect(contextMenuAnchor(Number.NaN, Number.NEGATIVE_INFINITY)).toEqual({ x: 0, y: 0 });
  });
});
