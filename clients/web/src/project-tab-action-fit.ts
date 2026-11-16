/**
 * The project strip's New ticket launcher gives its width to the project tabs when they do not fit
 * (HS2-RNR35H). Kerf's TabBar shrinks a tab to its label floor and then scrolls the strip, which can
 * scroll a lone tab's attention badge out of view; the full "New ticket…" label in the `end` zone is
 * the strip's widest optional content, so the shell swaps it for the terminal rail's compact
 * "Ticket…" launcher while the tabs overflow.
 *
 * The geometry comes only from Kerf's public `wireTabBars` `onFitChange` report, never from TabBar's
 * parts or the app's own measurement (HS2-A2CSNB, HS2-YATB6S). With `trailingPlacement="adjacent"` the
 * strip is only as wide as its tabs, so the room the `end` action can grow into is `TabBarFit.endFree`,
 * which Kerf re-reports when only the bar resizes (KF-3A0DBM). As Kerf's TabBar guidance prescribes, the
 * app remembers the full launcher's `endWidth` and swaps back only when `endFree` covers the difference.
 */
import type { TabBarFit } from '@kerfjs/ui/wire-tab-bars';

import { PROJECT_TAB_BAR_ID } from './components/project-tab-bar';

/** The parts of Kerf's `TabBarFit` report that decide the launcher. */
export type ProjectTabFitMeasure = Pick<TabBarFit, 'overflowing' | 'endWidth' | 'endFree'>;

/** Hysteresis memory: the `end` zone's width at the full label when the tabs overflowed. */
export interface ProjectTabFitMemory {
  fullEndWidth?: number;
}

/**
 * Whether the launcher should be compact after this report. Going compact needs a scrolling strip;
 * returning to the full label needs the bar's spare room to cover the full label's extra width, so a
 * strip that only fits because the label is compact never flips back and forth.
 */
export function projectTabActionCompact(
  compact: boolean,
  measure: ProjectTabFitMeasure,
  memory: ProjectTabFitMemory,
): boolean {
  if (!compact) {
    if (!measure.overflowing) return false;
    // The report describes the full label, so its end zone is the width the full label needs.
    memory.fullEndWidth = measure.endWidth;
    return true;
  }
  if (measure.overflowing) return true;
  // Without a full-label measurement (compact from the first report) the full label cannot be priced.
  if (memory.fullEndWidth === undefined) return true;
  return measure.endFree < memory.fullEndWidth - measure.endWidth;
}

export interface ProjectTabActionFit {
  /** Pass to `wireTabBars` `onFitChange`; reports for other bars are ignored. */
  onFit: (fit: TabBarFit) => void;
  /**
   * The desktop strip left (the shell went mobile): the next strip starts from the full label, as a
   * newly rendered strip would, and Kerf's first report for it decides again.
   */
  reset: () => void;
}

/**
 * Follow the desktop project TabBar's public fit reports and publish whether its launcher should be
 * compact. Only Kerf's fit report drives it: no observers, timers, or requests.
 */
export function projectTabActionFit(onCompact: (compact: boolean) => void): ProjectTabActionFit {
  let compact = false;
  const memory: ProjectTabFitMemory = {};
  const publish = (next: boolean) => {
    if (next === compact) return;
    compact = next;
    onCompact(compact);
  };
  return {
    onFit(fit) {
      if (fit.barId !== PROJECT_TAB_BAR_ID) return;
      publish(projectTabActionCompact(compact, fit, memory));
    },
    reset() {
      delete memory.fullEndWidth;
      publish(false);
    },
  };
}
