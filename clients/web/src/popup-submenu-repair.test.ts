import { afterEach, describe, expect, it, vi } from 'vitest';

import {
  reopenRacedSubmenu,
  repairDropdownSubmenus,
  resetStaleSubmenu,
  type SubmenuItem,
} from './popup-submenu-repair';

function fakeItem(state: { open: boolean; hidden: boolean; hasSubmenu?: boolean }) {
  const submenu = { hidden: state.hidden } as HTMLElement,
    dropdown = { open: true };
  const openSubmenu = vi.fn(() => {
    submenu.hidden = false;
    return Promise.resolve();
  });
  const item = {
    localName: 'wa-dropdown-item',
    hasSubmenu: state.hasSubmenu ?? true,
    submenuOpen: state.open,
    submenuElement: submenu,
    isConnected: true,
    openSubmenu,
    closest: () => dropdown,
  };
  return { item: item as unknown as SubmenuItem, submenu, dropdown, openSubmenu };
}

describe('Web Awesome submenu race repair (HS2-GV7A43)', () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it('clears only a stale open flag over a hidden submenu', () => {
    const stale = fakeItem({ open: true, hidden: true });
    expect(resetStaleSubmenu(stale.item)).toBe(true);
    expect(stale.item.submenuOpen).toBe(false);
    for (const state of [
      { open: true, hidden: false },
      { open: false, hidden: true },
      { open: false, hidden: false },
    ]) {
      const { item } = fakeItem(state);
      expect(resetStaleSubmenu(item)).toBe(false);
      expect(item.submenuOpen).toBe(state.open);
    }
  });

  it('reopens a submenu whose late hide landed after a reopen, only while the dropdown is open', () => {
    const raced = fakeItem({ open: true, hidden: true });
    expect(reopenRacedSubmenu(raced.item, false)).toBe(false);
    expect(raced.openSubmenu).not.toHaveBeenCalled();
    expect(reopenRacedSubmenu(raced.item, true)).toBe(true);
    expect(raced.openSubmenu).toHaveBeenCalledOnce();
    // A legitimate close clears submenuOpen before hiding, so it is left alone.
    const closed = fakeItem({ open: false, hidden: true });
    expect(reopenRacedSubmenu(closed.item, true)).toBe(false);
  });

  it('repairs on every show and watches each submenu once across a close, race, and reopen sequence', () => {
    const observers: Array<{ callback: () => void; target: unknown }> = [];
    vi.stubGlobal(
      'MutationObserver',
      class {
        constructor(private readonly callback: () => void) {}
        observe(target: unknown) {
          observers.push({ callback: this.callback, target });
        }
      },
    );
    const withSubmenu = fakeItem({ open: true, hidden: true }),
      plain = fakeItem({ open: false, hidden: false, hasSubmenu: false }),
      dropdown = { querySelectorAll: () => [withSubmenu.item, plain.item] } as unknown as HTMLElement;

    // Show 1: the stale flag left by a breakpoint re-render is cleared; one watcher is attached.
    repairDropdownSubmenus(dropdown);
    expect(withSubmenu.item.submenuOpen).toBe(false);
    expect(observers.map((observer) => observer.target)).toEqual([withSubmenu.submenu]);

    // Hover reopens, then the earlier close's late hide lands: the watcher reopens it.
    withSubmenu.item.submenuOpen = true;
    withSubmenu.submenu.hidden = true;
    observers[0].callback();
    expect(withSubmenu.openSubmenu).toHaveBeenCalledOnce();
    expect(withSubmenu.submenu.hidden).toBe(false);

    // The dropdown closes: Web Awesome clears the flag first, so the hide is not undone.
    withSubmenu.dropdown.open = false;
    withSubmenu.item.submenuOpen = false;
    withSubmenu.submenu.hidden = true;
    observers[0].callback();
    expect(withSubmenu.openSubmenu).toHaveBeenCalledOnce();

    // Show 2: nothing stale, and no second watcher for the same submenu.
    withSubmenu.dropdown.open = true;
    repairDropdownSubmenus(dropdown);
    expect(withSubmenu.item.submenuOpen).toBe(false);
    expect(observers).toHaveLength(1);
  });
});
