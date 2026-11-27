/**
 * The project strip yields optional width to the project tabs when they do not fit (HS2-RNR35H).
 * Kerf's TabBar shrinks a tab to its label floor and then scrolls the strip, which can scroll a lone
 * tab's attention badge out of view, so the shell gives room back in two stages while the tabs
 * overflow:
 *
 * 1. the `leading` Global Dashboards switcher folds its buttons into one menu (HS2-B0RJBY: Systems,
 *    HS2-DR0ZSQ, made it the strip's widest fixed group);
 * 2. the full "New ticket…" label in the `end` zone becomes the terminal rail's compact "Ticket…"
 *    launcher (and Settings → Commands its compact pills, HS2-MQPY61).
 *
 * Room returns in the reverse order: the launcher's full label first, then the dashboard buttons.
 *
 * The geometry comes only from Kerf's public `wireTabBars` `onFitChange` report, never from TabBar's
 * parts or the app's own measurement (HS2-A2CSNB, HS2-YATB6S). With `trailingPlacement="adjacent"` the
 * strip is only as wide as its tabs, so the room the strip's zones can grow into is `TabBarFit.endFree`,
 * which Kerf re-reports when only the bar resizes (KF-3A0DBM). As Kerf's TabBar guidance prescribes, the
 * app remembers each stage's full width when it yields and restores it only when `endFree` covers the
 * difference, so a strip that only fits because something yielded never flips back and forth.
 */
import type { TabBarFit } from '@kerfjs/ui/wire-tab-bars';

import { PROJECT_TAB_BAR_ID } from './components/project-tab-bar';

/** The parts of Kerf's `TabBarFit` report that decide the strip's yield stages. */
export type ProjectTabFitMeasure = Pick<
  TabBarFit,
  'overflowing' | 'endWidth' | 'endFree' | 'barWidth' | 'stripWidth' | 'trailingWidth'
>;

/** Hysteresis memory: each stage's width before it yielded. */
export interface ProjectTabFitMemory {
  /** The `end` zone's width at the full label when the tabs overflowed. */
  fullEndWidth?: number;
  /** The leading zone's extent with the dashboard buttons unfolded when the tabs overflowed. */
  fullLeadingWidth?: number;
}

/** What the strip has yielded to its tabs. */
export interface ProjectStripFit {
  /** The Global Dashboards buttons are folded into one menu (stage 1). */
  dashboardsFolded: boolean;
  /** The `end` launcher shows its compact label (stage 2). */
  actionCompact: boolean;
}

export const PROJECT_STRIP_FULL: ProjectStripFit = Object.freeze({ dashboardsFolded: false, actionCompact: false });

/**
 * The leading zone's extent plus the bar's fixed gaps, derived from the report's own widths: the bar's
 * inner width less the strip, the trailing and end zones, and the unused room. The gaps are the same
 * folded or unfolded, so the difference between two reports is the leading zone's change.
 */
export function projectStripLeadingExtent(measure: ProjectTabFitMeasure): number {
  return measure.barWidth - measure.stripWidth - measure.trailingWidth - measure.endWidth - measure.endFree;
}

/**
 * The strip's yield stages after this report. Each overflowing report yields one more stage, remembering
 * that stage's full width; each fitting report restores the last stage yielded once `endFree` covers the
 * width it would take back.
 */
export function projectStripFit(
  current: ProjectStripFit,
  measure: ProjectTabFitMeasure,
  memory: ProjectTabFitMemory,
): ProjectStripFit {
  if (measure.overflowing) {
    if (!current.dashboardsFolded) {
      memory.fullLeadingWidth = projectStripLeadingExtent(measure);
      return { dashboardsFolded: true, actionCompact: current.actionCompact };
    }
    if (!current.actionCompact) {
      // The report describes the full label, so its end zone is the width the full label needs.
      memory.fullEndWidth = measure.endWidth;
      return { dashboardsFolded: true, actionCompact: true };
    }
    return current;
  }
  if (current.actionCompact) {
    // Without a full-label measurement the full label cannot be priced.
    if (memory.fullEndWidth === undefined || measure.endFree < memory.fullEndWidth - measure.endWidth) return current;
    return { dashboardsFolded: current.dashboardsFolded, actionCompact: false };
  }
  if (current.dashboardsFolded) {
    if (
      memory.fullLeadingWidth === undefined ||
      measure.endFree < memory.fullLeadingWidth - projectStripLeadingExtent(measure)
    )
      return current;
    return PROJECT_STRIP_FULL;
  }
  return current;
}

export interface ProjectTabActionFit {
  /** Pass to `wireTabBars` `onFitChange`; reports for other bars are ignored. */
  onFit: (fit: TabBarFit) => void;
  /**
   * The desktop strip left (the shell went mobile): the next strip starts unfolded with the full label,
   * as a newly rendered strip would, and Kerf's first report for it decides again.
   */
  reset: () => void;
}

/**
 * Follow the desktop project TabBar's public fit reports and publish what the strip yields to its tabs.
 * Only Kerf's fit report drives it: no observers, timers, or requests.
 */
export function projectTabActionFit(onChange: (fit: ProjectStripFit) => void): ProjectTabActionFit {
  let current = PROJECT_STRIP_FULL;
  const memory: ProjectTabFitMemory = {};
  const publish = (next: ProjectStripFit) => {
    if (next.dashboardsFolded === current.dashboardsFolded && next.actionCompact === current.actionCompact) return;
    current = next;
    onChange(current);
  };
  return {
    onFit(fit) {
      if (fit.barId !== PROJECT_TAB_BAR_ID) return;
      publish(projectStripFit(current, fit, memory));
    },
    reset() {
      delete memory.fullEndWidth;
      delete memory.fullLeadingWidth;
      publish(PROJECT_STRIP_FULL);
    },
  };
}
