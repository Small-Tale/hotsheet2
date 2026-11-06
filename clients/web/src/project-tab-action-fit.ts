/**
 * The project strip's New ticket launcher gives its width to the project tabs when they do not fit
 * (HS2-RNR35H). Kerf's TabBar shrinks a tab to its label floor and then scrolls the strip, which can
 * scroll a lone tab's attention badge out of view; the full "New ticket…" label in the `end` zone is
 * the strip's widest optional content, so the shell swaps it for the terminal rail's compact
 * "Ticket…" launcher while the tabs overflow.
 */

/** One measurement of the desktop project TabBar. */
export interface ProjectTabFitMeasure {
  /** The tab strip's content width beyond its visible width (positive while the strip scrolls). */
  overflow: number;
  /** Unused width between the adjacent trailing zone and the far-edge `end` zone. */
  free: number;
  /** The `end` zone's current width. */
  endWidth: number;
}

/** Hysteresis memory: the `end` zone's width the last time it showed the full label. */
export interface ProjectTabFitMemory {
  fullEndWidth?: number;
}

/**
 * Whether the launcher should be compact after this measurement. Going compact needs a scrolling strip;
 * returning to the full label needs room for the remembered full-width `end` zone, so a strip that only
 * fits because the label is compact never flips back and forth.
 */
export function projectTabActionCompact(
  compact: boolean,
  measure: ProjectTabFitMeasure,
  memory: ProjectTabFitMemory,
): boolean {
  if (!compact) {
    memory.fullEndWidth = measure.endWidth;
    return measure.overflow > 1;
  }
  if (measure.overflow > 1) return true;
  // Without a full-width measurement (compact from the first frame) the full label cannot be priced.
  if (memory.fullEndWidth === undefined) return true;
  return measure.free + measure.endWidth < memory.fullEndWidth;
}

const BAR = '.project-tab-bar:not(.project-tab-bar--mobile) .kui-tab-bar';

function measureBar(bar: HTMLElement): ProjectTabFitMeasure | undefined {
  const tabs = bar.querySelector<HTMLElement>(':scope > .kui-tab-bar__tabs'),
    trailing = bar.querySelector<HTMLElement>(':scope > .kui-tab-bar__trailing'),
    end = bar.querySelector<HTMLElement>(':scope > .kui-tab-bar__end');
  if (!tabs || !end || bar.clientWidth === 0) return undefined;
  const gap = Number.parseFloat(getComputedStyle(bar).columnGap) || 0,
    before = (trailing ?? tabs).getBoundingClientRect().right,
    endBox = end.getBoundingClientRect();
  return {
    overflow: tabs.scrollWidth - tabs.clientWidth,
    free: Math.max(0, endBox.left - before - gap),
    endWidth: endBox.width,
  };
}

/**
 * Follow the desktop project TabBar's geometry and publish whether its launcher should be compact. Only
 * layout observers drive it: no timers and no requests. Returns a disposer.
 */
export function wireProjectTabActionFit(root: HTMLElement, onCompact: (compact: boolean) => void): () => void {
  let bar: HTMLElement | null = null,
    compact = false;
  const memory: ProjectTabFitMemory = {};
  const refresh = () => {
    if (!bar?.isConnected) return;
    const measure = measureBar(bar);
    if (!measure) return;
    const next = projectTabActionCompact(compact, measure, memory);
    if (next === compact) return;
    compact = next;
    onCompact(compact);
  };
  const resize = new ResizeObserver(refresh);
  let observed: HTMLElement[] = [];
  const observeBar = () => {
    bar = root.querySelector<HTMLElement>(BAR);
    const targets = bar
      ? [
          bar,
          ...bar.querySelectorAll<HTMLElement>(
            ':scope > :is(.kui-tab-bar__tabs, .kui-tab-bar__trailing, .kui-tab-bar__end)',
          ),
        ]
      : [];
    if (targets.length === observed.length && targets.every((item, index) => item === observed[index])) return;
    resize.disconnect();
    observed = targets;
    for (const target of targets) resize.observe(target);
    if (!bar) {
      // The desktop strip left (mobile, or a shell without it): the next strip starts from the full label.
      delete memory.fullEndWidth;
      if (compact) {
        compact = false;
        onCompact(false);
      }
    }
  };
  // A tab added to a strip already at its shrink limit grows only the scroll width, which no resize
  // reports; Kerf's overflow attribute on the strip changes instead. Structural changes re-find the bar.
  const mutations = new MutationObserver((records) => {
    observeBar();
    if (bar && records.some((record) => bar?.contains(record.target))) refresh();
  });
  mutations.observe(root, {
    childList: true,
    subtree: true,
    attributes: true,
    attributeFilter: ['data-scroll-overflow'],
  });
  observeBar();
  refresh();
  return () => {
    mutations.disconnect();
    resize.disconnect();
  };
}
