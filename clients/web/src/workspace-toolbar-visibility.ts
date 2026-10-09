/** Observe the workspace header's width without remeasuring on mutations elsewhere in the app. */
const ITEM = '[data-hide-below], [data-show-below]';
const STATE = 'data-toolbar-visibility-state';
const HIDDEN = 'data-toolbar-width-hidden';

function threshold(item: Element, name: string): number | undefined {
  const value = item.getAttribute(name);
  if (value === null) return undefined;
  const match = /^(?:0|[1-9]\d*)(?:\.\d+)?px$/.exec(value);
  return match ? Number.parseFloat(value) : undefined;
}

export function workspaceToolbarHidden(width: number, hide?: number, show?: number): boolean {
  return (hide !== undefined && width < hide) || (show !== undefined && width >= show);
}

interface WorkspaceSearchSizing {
  width: number;
  expandedWidth: string;
}

/** Keep measured widths on the stable mount root so a Kerf morph cannot reset the slot. */
export function applyWorkspaceSearchSizing(
  style: Pick<CSSStyleDeclaration, 'getPropertyValue' | 'setProperty'>,
  sizing: WorkspaceSearchSizing,
): boolean {
  let changed = false;
  if (style.getPropertyValue('--hs-workspace-search-expanded-width') !== sizing.expandedWidth) {
    style.setProperty('--hs-workspace-search-expanded-width', sizing.expandedWidth);
    changed = true;
  }
  const width = `${sizing.width}px`;
  if (style.getPropertyValue('--hs-workspace-search-slot-width') !== width) {
    style.setProperty('--hs-workspace-search-slot-width', width);
    changed = true;
  }
  return changed;
}

/** Keep the editor usable while retaining every action that fits at its rendered width. */
export function workspaceSearchFit(
  width: number,
  title: number,
  groups: readonly number[],
  more: number,
  gap: number,
  minimumSearch = 240,
): boolean[] {
  const visible = [true, ...groups.map(() => true)];
  const needed = () => {
    const shown = visible.slice(1).filter(Boolean).length;
    const overflow = visible.some((item) => !item);
    return (
      (visible[0] ? title : 0) +
      gap +
      groups.reduce((sum, item, index) => sum + (visible[index + 1] ? item + gap : 0), 0) +
      minimumSearch +
      (overflow ? more + gap : 0) +
      // Leave room for the search slot's own border and fractional pixel rounding.
      (shown ? 10 : 8)
    );
  };
  for (let index = visible.length - 1; index >= 0; index -= 1) {
    if (needed() <= width) break;
    visible[index] = false;
  }
  return visible;
}

export function wireWorkspaceToolbarVisibility(
  root: HTMLElement,
  selector = '.workspace-header[data-component="toolbar"], [data-component="toolbar"][aria-label="Workspace toolbar"]',
): () => void {
  let header: HTMLElement | null = null;
  let observedSearchSlot: HTMLElement | null = null;
  const measuredWidths = new WeakMap<HTMLElement, { width: number; sizing: string | null }>();
  const resize = new ResizeObserver(() => {
    refresh();
  });

  function setHidden(item: HTMLElement, hidden: boolean) {
    const value = hidden ? 'true' : null;
    let state = item.querySelector<HTMLElement>(`:scope > [${STATE}]`);
    if (!state) {
      state = document.createElement('span');
      state.setAttribute(STATE, '');
      state.setAttribute('data-morph-preserve', '');
      state.setAttribute('aria-hidden', 'true');
      state.hidden = true;
      item.append(state);
    }
    if (value === null) {
      if (item.hasAttribute(HIDDEN)) item.removeAttribute(HIDDEN);
      if (state.hasAttribute(HIDDEN)) state.removeAttribute(HIDDEN);
    } else {
      if (item.getAttribute(HIDDEN) !== value) item.setAttribute(HIDDEN, value);
      if (state.getAttribute(HIDDEN) !== value) state.setAttribute(HIDDEN, value);
    }
  }

  function refresh() {
    if (!header?.isConnected) return;
    const style = getComputedStyle(header);
    const width = header.getBoundingClientRect().width;
    const contentWidth = Math.max(
      0,
      width -
        (Number.parseFloat(style.paddingLeft) || 0) -
        (Number.parseFloat(style.paddingRight) || 0) -
        (Number.parseFloat(style.borderLeftWidth) || 0) -
        (Number.parseFloat(style.borderRightWidth) || 0),
    );
    const searchOpen = header.querySelector('.workspace-header__search-actions[data-search-open="true"]');
    const searchSlot = searchOpen instanceof HTMLElement ? searchOpen : null;
    if (searchSlot !== observedSearchSlot) {
      if (observedSearchSlot) resize.unobserve(observedSearchSlot);
      observedSearchSlot = searchSlot;
      if (observedSearchSlot) resize.observe(observedSearchSlot);
    }
    const identity = header.querySelector<HTMLElement>('.workspace-header__identity');
    const groups = [
      header.querySelector<HTMLElement>('.view-mode-switcher'),
      header.querySelector<HTMLElement>('.workspace-header__sort-group'),
      header.querySelector<HTMLElement>('.workspace-header__utility-group'),
    ];
    const more = header.querySelector<HTMLElement>('.workspace-header__overflow-group');
    const dynamic = Boolean(searchOpen && groups.every(Boolean) && more);
    const fitItems = dynamic ? [...(identity ? [identity] : []), ...(groups as HTMLElement[]), more!] : [];
    const rail = header.classList.contains('terminal-ticket-rail__controls');
    const items = new Set<HTMLElement>([
      ...header.querySelectorAll<HTMLElement>(ITEM),
      ...[identity, ...groups, more].filter((item): item is HTMLElement => Boolean(item)),
    ]);
    for (const item of items) {
      if (fitItems.includes(item)) {
        if (item !== identity && measuredWidths.get(item)?.sizing !== item.getAttribute('data-sizing')) {
          measuredWidths.delete(item);
          setHidden(item, false);
        }
        continue;
      }
      if (item === identity) item.setAttribute('data-hide-below', '224px');
      else if (item === groups[0]) {
        if (rail) item.removeAttribute('data-hide-below');
        else item.setAttribute('data-hide-below', '176px');
      } else if (item === groups[1]) {
        if (rail) item.removeAttribute('data-hide-below');
        else item.setAttribute('data-hide-below', '416px');
      } else if (item === groups[2]) {
        if (rail) item.removeAttribute('data-hide-below');
        else item.setAttribute('data-hide-below', '480px');
      } else if (item === more) item.setAttribute('data-show-below', rail ? '0px' : '480px');
      const hide = threshold(item, 'data-hide-below');
      const show = threshold(item, 'data-show-below');
      setHidden(item, workspaceToolbarHidden(contentWidth, hide, show));
    }
    if (dynamic) {
      const itemWidth = (item: HTMLElement): number => {
        const visibleWidth = item.getBoundingClientRect().width;
        if (visibleWidth > 0)
          measuredWidths.set(item, { width: visibleWidth, sizing: item.getAttribute('data-sizing') });
        return measuredWidths.get(item)?.width ?? visibleWidth;
      };
      const titleText = identity?.querySelector<HTMLElement>('.kui-toolbar-text__text');
      const titleStyle = identity ? getComputedStyle(identity) : null;
      let titleWidth = 0;
      if (identity && titleText && titleStyle) {
        const probe = document.createElement('span');
        const textStyle = getComputedStyle(titleText);
        probe.textContent = titleText.textContent;
        probe.style.cssText = `position:fixed;visibility:hidden;width:max-content;white-space:nowrap;font:${textStyle.font};letter-spacing:${textStyle.letterSpacing}`;
        document.body.append(probe);
        titleWidth =
          probe.getBoundingClientRect().width +
          (Number.parseFloat(titleStyle.paddingLeft) || 0) +
          (Number.parseFloat(titleStyle.paddingRight) || 0);
        probe.remove();
      }
      const trailing = header.querySelector<HTMLElement>('.kui-toolbar__trailing');
      const gap = Number.parseFloat(getComputedStyle(trailing ?? header).columnGap) || 8;
      const leading = header.querySelector<HTMLElement>('.kui-toolbar__leading');
      const extraWidth = rail
        ? 0
        : [
            ...(leading ? Array.from(leading.children).filter((item) => item !== identity) : []),
            ...(trailing
              ? Array.from(trailing.children).filter(
                  (item) => !groups.includes(item as HTMLElement) && item !== searchSlot,
                )
              : []),
          ].reduce((sum, item) => {
            const width = item.getBoundingClientRect().width;
            return sum + (width > 0 ? width + gap : 0);
          }, 0);
      const fitWidth = Math.max(0, contentWidth - extraWidth);
      const groupWidths = groups.map((item) => itemWidth(item!));
      const moreWidth = itemWidth(more!);
      // The rail's full-width view tabs own row one; only sort and actions share row two
      // with search. Keep the view visible instead of charging its width to search.
      const fitted = workspaceSearchFit(
        fitWidth,
        rail ? 0 : titleWidth,
        rail ? groupWidths.slice(1) : groupWidths,
        moreWidth,
        gap,
      );
      const visible = rail ? [true, true, ...fitted.slice(1)] : fitted;
      const availableSearch =
        fitWidth -
        (rail ? 0 : visible[0] ? titleWidth : 0) -
        gap -
        groupWidths.reduce(
          (sum, width, index) => sum + (rail && index === 0 ? 0 : visible[index + 1] ? width + gap : 0),
          0,
        ) -
        (visible.every(Boolean) ? 0 : moreWidth + gap);
      const expandedWidth = `${Math.max(0, availableSearch - 8)}px`;
      const slotWidth = Math.max(0, availableSearch - 8 + (visible.every(Boolean) ? 0 : moreWidth + gap));
      applyWorkspaceSearchSizing(root.style, { width: slotWidth, expandedWidth });
      if (identity) {
        identity.setAttribute('data-hide-below', `${visible[0] ? 0 : contentWidth + 1}px`);
        setHidden(identity, !visible[0]);
      }
      groups.forEach((item, index) => {
        item!.setAttribute('data-hide-below', `${visible[index + 1] ? 0 : contentWidth + 1}px`);
        setHidden(item!, !visible[index + 1]);
      });
      more!.setAttribute('data-show-below', `${visible.every(Boolean) ? 0 : contentWidth + 1}px`);
      setHidden(more!, visible.every(Boolean));
    } else {
      root.style.removeProperty('--hs-workspace-search-expanded-width');
      root.style.removeProperty('--hs-workspace-search-slot-width');
    }
  }

  function findHeader(): boolean {
    const next = root.querySelector<HTMLElement>(selector);
    if (next === header) return false;
    if (header) resize.unobserve(header);
    if (observedSearchSlot) resize.unobserve(observedSearchSlot);
    observedSearchSlot = null;
    header = next;
    if (header) {
      resize.observe(header);
      refresh();
    }
    return true;
  }

  const mutations = new MutationObserver((records) => {
    if (
      records.some(
        (record) =>
          record.target === root ||
          (record.target instanceof Element && header?.contains(record.target)) ||
          [...record.addedNodes, ...record.removedNodes].some(
            (node) => node instanceof Element && (node.matches(ITEM) || Boolean(node.querySelector(ITEM))),
          ),
      ) &&
      !findHeader()
    )
      refresh();
  });
  // Kerf may morph the app-rendered search slot after a query edit, dropping its inline width
  // without changing the slot's layout box. The separate observer watches only that slot.
  mutations.observe(root, { childList: true, attributes: true, attributeFilter: ['data-search-open'], subtree: true });
  findHeader();
  return () => {
    mutations.disconnect();
    resize.disconnect();
    root.style.removeProperty('--hs-workspace-search-expanded-width');
    root.style.removeProperty('--hs-workspace-search-slot-width');
  };
}

/** The ticket rail shares the measured search policy while keeping its own observer lifecycle. */
export function wireTicketRailToolbarVisibility(root: HTMLElement): () => void {
  return wireWorkspaceToolbarVisibility(root, '.terminal-ticket-rail__controls[data-component="toolbar"]');
}
