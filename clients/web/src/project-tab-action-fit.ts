/**
 * The project strip's New ticket launcher gives its width to the project tabs when they do not fit
 * (HS2-RNR35H). Kerf's TabBar shrinks a tab to its label floor and then scrolls the strip, which can
 * scroll a lone tab's attention badge out of view; the full "New ticket…" label in the `end` zone is
 * the strip's widest optional content, so the shell swaps it for the terminal rail's compact
 * "Ticket…" launcher while the tabs overflow.
 *
 * The geometry comes from Kerf's public `wireTabBars` `onFitChange` report plus the TabBar root's own
 * width, never from TabBar's private parts (HS2-A2CSNB). With `trailingPlacement="adjacent"` the strip
 * is only as wide as its tabs, so `TabBarFit.free` cannot price the bar's spare room; instead the app
 * remembers how much of the bar everything except the strip took when the full label overflowed.
 */
import type { TabBarFit } from '@kerfjs/ui/wire-tab-bars';

import { PROJECT_TAB_BAR_ID } from './components/project-tab-bar';

/** One measurement of the desktop project TabBar. */
export interface ProjectTabFitMeasure {
  /** Kerf reports the tabs need more room than the strip has (the strip scrolls). */
  overflowing: boolean;
  /** The TabBar root's inline content width. */
  barWidth: number;
  /** Kerf's inline room the strip gives its tabs. */
  stripWidth: number;
  /** Kerf's inline extent the tabs occupy. */
  tabsWidth: number;
}

/** Hysteresis memory: the bar width everything but the strip took when the full label overflowed. */
export interface ProjectTabFitMemory {
  fullOverhead?: number;
}

/**
 * Whether the launcher should be compact after this measurement. Going compact needs a scrolling strip;
 * returning to the full label needs the bar to give the strip room for its tabs beside the remembered
 * full-label overhead, so a strip that only fits because the label is compact never flips back and forth.
 */
export function projectTabActionCompact(
  compact: boolean,
  measure: ProjectTabFitMeasure,
  memory: ProjectTabFitMemory,
): boolean {
  if (!compact) {
    if (!measure.overflowing) return false;
    // An overflowing strip fills the bar, so the rest of the bar is the full label's fixed overhead.
    memory.fullOverhead = measure.barWidth - measure.stripWidth;
    return true;
  }
  if (measure.overflowing) return true;
  // Without a full-label measurement (compact from the first frame) the full label cannot be priced.
  if (memory.fullOverhead === undefined) return true;
  return measure.barWidth - memory.fullOverhead < measure.tabsWidth;
}

const BAR = `.project-tab-bar:not(.project-tab-bar--mobile) [data-component="tab-bar"][data-tab-bar-id="${PROJECT_TAB_BAR_ID}"]`;

function contentWidth(element: HTMLElement) {
  const style = getComputedStyle(element);
  return (
    element.clientWidth - (Number.parseFloat(style.paddingLeft) || 0) - (Number.parseFloat(style.paddingRight) || 0)
  );
}

export interface ProjectTabActionFit {
  /** Pass to `wireTabBars` `onFitChange`; reports for other bars are ignored. */
  onFit: (fit: TabBarFit) => void;
  dispose: () => void;
}

/**
 * Follow the desktop project TabBar's public fit reports and root width and publish whether its launcher
 * should be compact. Only Kerf's fit report and layout observers drive it: no timers and no requests.
 */
export function wireProjectTabActionFit(root: HTMLElement, onCompact: (compact: boolean) => void): ProjectTabActionFit {
  let bar: HTMLElement | null = null,
    fit: TabBarFit | undefined,
    compact = false;
  const memory: ProjectTabFitMemory = {};
  const refresh = () => {
    if (!bar?.isConnected || !fit || bar.clientWidth === 0) return;
    const next = projectTabActionCompact(
      compact,
      {
        overflowing: fit.overflowing,
        barWidth: contentWidth(bar),
        stripWidth: fit.stripWidth,
        tabsWidth: fit.tabsWidth,
      },
      memory,
    );
    if (next === compact) return;
    compact = next;
    onCompact(compact);
  };
  // The strip keeps its width while the bar grows beside it, which Kerf's zone observers do not report.
  const resize = new ResizeObserver(refresh);
  const observeBar = () => {
    const next = root.querySelector<HTMLElement>(BAR);
    if (next === bar) return;
    resize.disconnect();
    bar = next;
    fit = undefined;
    if (bar) {
      resize.observe(bar);
      return;
    }
    // The desktop strip left (mobile, or a shell without it): the next strip starts from the full label.
    delete memory.fullOverhead;
    if (compact) {
      compact = false;
      onCompact(false);
    }
  };
  const mutations = new MutationObserver(observeBar);
  mutations.observe(root, { childList: true, subtree: true });
  observeBar();
  return {
    onFit(report) {
      if (report.barId !== PROJECT_TAB_BAR_ID) return;
      observeBar();
      fit = report;
      refresh();
    },
    dispose() {
      mutations.disconnect();
      resize.disconnect();
    },
  };
}
