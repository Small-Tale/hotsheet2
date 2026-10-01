/**
 * Recovery for a Web Awesome 3.12 `wa-dropdown-item` submenu race (HS2-GV7A43, upstream
 * `KF-A388BJ`). `closeSubmenu()` clears `submenuOpen`, then awaits its hide animation before
 * setting the submenu `hidden`. When `openSubmenu()` runs during that animation — reopening the
 * menu right after an Escape close, or a breakpoint re-render while it is open — the late hide
 * lands after the open. The item is left with `submenuOpen === true` over a hidden submenu, and
 * because hover and ArrowRight only set `submenuOpen = true` again, Lit sees no change and the
 * submenu never reopens.
 */
export interface SubmenuItem extends HTMLElement {
  submenuOpen: boolean;
  hasSubmenu: boolean;
  readonly submenuElement?: HTMLElement | null;
  openSubmenu(): Promise<void>;
}

const watched = new WeakSet<HTMLElement>();

function isSubmenuItem(element: Element): element is SubmenuItem {
  return element.localName === 'wa-dropdown-item' && Boolean((element as Partial<SubmenuItem>).hasSubmenu);
}

/** Clear a stale open flag over a hidden submenu so the next hover or ArrowRight opens it. */
export function resetStaleSubmenu(item: SubmenuItem): boolean {
  if (!item.submenuOpen || !item.submenuElement?.hidden) return false;
  item.submenuOpen = false;
  return true;
}

/** Re-open a submenu whose late hide landed after it was opened again. */
export function reopenRacedSubmenu(item: SubmenuItem, dropdownOpen: boolean): boolean {
  if (!dropdownOpen || !item.isConnected || !item.submenuOpen || !item.submenuElement?.hidden) return false;
  void item.openSubmenu();
  return true;
}

/**
 * Repair every submenu item in a dropdown that is about to show, and watch each submenu so a
 * late hide that races a reopen is undone. Safe to call on every show.
 */
export function repairDropdownSubmenus(dropdown: HTMLElement & { open?: boolean }): void {
  // A descendant selector keeps the items typed as plain `Element`s, because an item's
  // `submenuElement` (a `@query` result) is null until its first render despite its declared type.
  for (const element of dropdown.querySelectorAll(':scope wa-dropdown-item')) {
    if (!isSubmenuItem(element)) continue;
    resetStaleSubmenu(element);
    const submenu = element.submenuElement;
    if (!submenu || watched.has(submenu)) continue;
    watched.add(submenu);
    new MutationObserver(() => {
      reopenRacedSubmenu(element, Boolean(element.closest<HTMLElement & { open?: boolean }>('wa-dropdown')?.open));
    }).observe(submenu, { attributes: true, attributeFilter: ['hidden'] });
  }
}
