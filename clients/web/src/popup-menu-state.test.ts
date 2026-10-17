import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { closedPopupMenuRoot, isPopupMenuOpen, POPUP_MENU_PANEL } from './popup-menu-state';

/** Unit tests run without a DOM; a stand-in class lets `instanceof HTMLElement` checks run. */
class FakeElement {
  readonly fake = true;
}

/** A fake rendered Kerf PopupMenu root (Kerf UI 5.0.0-beta.106) with its opener and panel. */
function menuRoot(expanded: string | null) {
  const opener = { getAttribute: (name: string) => (name === 'aria-expanded' ? expanded : null) };
  const root = Object.assign(new FakeElement(), {
    querySelector: vi.fn((selector: string) => (selector.includes('kui-popup-menu__trigger') ? opener : null)),
    matches: (selector: string) => selector === '.kui-popup-menu',
  });
  const panel = Object.assign(new FakeElement(), {
    parentElement: root,
    matches: (selector: string) => selector === POPUP_MENU_PANEL,
  });
  return { root, panel };
}
const toggle = (newState: string, target: unknown, type = 'toggle') => ({ type, newState, target }) as unknown as Event;

describe('isPopupMenuOpen (HS2-BH7NZ4)', () => {
  it("reads the opener's aria-expanded, for both a trigger and a context anchor", () => {
    expect(isPopupMenuOpen(menuRoot('true').root as unknown as Element)).toBe(true);
    expect(isPopupMenuOpen(menuRoot('false').root as unknown as Element)).toBe(false);
    // A context anchor carries no aria-expanded until Kerf first opens the menu.
    expect(isPopupMenuOpen(menuRoot(null).root as unknown as Element)).toBe(false);
    const { root } = menuRoot('true');
    isPopupMenuOpen(root as unknown as Element);
    const selector = root.querySelector.mock.calls[0][0];
    expect(selector).toContain(':scope > .kui-popup-menu__trigger');
    expect(selector).toContain(':scope > .kui-popup-menu__context-anchor');
  });

  it('is closed when the root has no opener at all', () => {
    expect(isPopupMenuOpen({ querySelector: () => null } as unknown as Element)).toBe(false);
  });
});

describe('closedPopupMenuRoot (HS2-BH7NZ4)', () => {
  beforeEach(() => {
    vi.stubGlobal('HTMLElement', FakeElement);
  });
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("returns the root when its top-level panel's popover closes", () => {
    const { root, panel } = menuRoot('false');
    expect(closedPopupMenuRoot(toggle('closed', panel))).toBe(root);
  });

  it('ignores an opening toggle and any other event type', () => {
    const { panel } = menuRoot('true');
    expect(closedPopupMenuRoot(toggle('open', panel))).toBeUndefined();
    expect(closedPopupMenuRoot(toggle('closed', panel, 'beforetoggle'))).toBeUndefined();
  });

  it('ignores a submenu closing inside the menu (its parent is an item host, not the root)', () => {
    const host = Object.assign(new FakeElement(), { matches: () => false });
    const submenu = Object.assign(new FakeElement(), {
      parentElement: host,
      matches: (selector: string) => selector === POPUP_MENU_PANEL,
    });
    expect(closedPopupMenuRoot(toggle('closed', submenu))).toBeUndefined();
  });

  it('ignores toggles from other popovers and non-element targets', () => {
    const other = Object.assign(new FakeElement(), { parentElement: null, matches: () => false });
    expect(closedPopupMenuRoot(toggle('closed', other))).toBeUndefined();
    expect(closedPopupMenuRoot(toggle('closed', { matches: () => true }))).toBeUndefined();
    const orphan = Object.assign(new FakeElement(), {
      parentElement: null,
      matches: (selector: string) => selector === POPUP_MENU_PANEL,
    });
    expect(closedPopupMenuRoot(toggle('closed', orphan))).toBeUndefined();
  });
});
