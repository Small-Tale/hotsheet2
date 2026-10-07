/** The workspace header uses fixed pixel thresholds. Observe its width without making every
 * mutation elsewhere in the app remeasure the header's computed CSS variables. */
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

export function wireWorkspaceToolbarVisibility(root: HTMLElement): () => void {
  let header: HTMLElement | null = null;
  const resize = new ResizeObserver(() => {
    refresh();
  });

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
    for (const item of header.querySelectorAll<HTMLElement>(ITEM)) {
      const hide = threshold(item, 'data-hide-below');
      const show = threshold(item, 'data-show-below');
      const hidden = workspaceToolbarHidden(contentWidth, hide, show);
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
  }

  function findHeader(): boolean {
    const next = root.querySelector<HTMLElement>(ITEM)?.closest<HTMLElement>('[data-component="toolbar"]') ?? null;
    if (next === header) return false;
    if (header) resize.unobserve(header);
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
  mutations.observe(root, { childList: true, subtree: true });
  findHeader();
  return () => {
    mutations.disconnect();
    resize.disconnect();
  };
}
