import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  type ContextPopupMenuElement,
  maintainContextPopupMenuAnchor,
  reanchorReplacedContextPopupMenus,
  viewportSafeContextMenuPosition,
  viewportSafePointerPosition,
} from './context-menu-position';

afterEach(() => vi.unstubAllGlobals());

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
      const properties = new Map<string, string>();
      return {
        open: false,
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
    expect(listeners.has('wa-hide')).toBe(false);
    stop();
  });
});

describe('reanchorReplacedContextPopupMenus', () => {
  it('opens each new host at its original pointer once and resets after dismissal', () => {
    const makeMenu = () => {
      const properties = new Map<string, string>(),
        setProperty = vi.fn((name: string, value: string) => properties.set(name, value)),
        anchor = { contextAnchorX: '418', contextAnchorY: '267' };
      return {
        menu: {
          open: true,
          style: { setProperty },
          getAttribute: () =>
            properties.size ? [...properties].map(([name, value]) => `${name}:${value}`).join(';') : null,
          closest: () => ({ dataset: anchor }),
        } as unknown as ContextPopupMenuElement,
        setProperty,
        properties,
        anchor,
      };
    };
    const first = makeMenu(),
      second = makeMenu(),
      opened = new Map<string, ContextPopupMenuElement>();
    let current = first.menu;
    const root = { querySelector: () => current } as unknown as ParentNode;

    reanchorReplacedContextPopupMenus(['ticket'], opened, root);
    expect(first.setProperty).toHaveBeenCalledTimes(2);
    reanchorReplacedContextPopupMenus(['ticket'], opened, root);
    expect(first.setProperty).toHaveBeenCalledTimes(2);
    first.anchor.contextAnchorX = '512';
    reanchorReplacedContextPopupMenus(['ticket'], opened, root);
    expect(first.setProperty.mock.calls.at(-2)?.[1]).toBe('512px');
    expect(first.setProperty).toHaveBeenCalledTimes(4);

    current = second.menu;
    reanchorReplacedContextPopupMenus(['ticket'], opened, root);
    expect(second.setProperty.mock.calls.map(([, value]) => value)).toEqual(['418px', '267px']);

    second.properties.clear();
    reanchorReplacedContextPopupMenus(['ticket'], opened, root);
    expect(second.setProperty).toHaveBeenCalledTimes(4);
    reanchorReplacedContextPopupMenus(['ticket'], opened, root);
    expect(second.setProperty).toHaveBeenCalledTimes(4);
    reanchorReplacedContextPopupMenus([], opened, root);
    expect(opened.size).toBe(0);
    reanchorReplacedContextPopupMenus(['ticket'], opened, root);
    expect(second.setProperty).toHaveBeenCalledTimes(6);
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
  });
  it('only clamps coordinates which are actually outside the viewport', () => {
    expect(viewportSafePointerPosition(-20, 730, 1000, 700)).toEqual({ x: 0, y: 700 });
  });
});
