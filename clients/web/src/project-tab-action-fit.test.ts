import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import type { TabBarFit } from '@kerfjs/ui/wire-tab-bars';
import { expect, it } from 'vitest';

import { PROJECT_TAB_BAR_ID } from './components/project-tab-bar';
import {
  PROJECT_STRIP_FULL,
  type ProjectStripFit,
  projectStripFit,
  projectStripLeadingExtent,
  projectTabActionFit,
  type ProjectTabFitMeasure,
  type ProjectTabFitMemory,
} from './project-tab-action-fit';

// The full "New ticket…" launcher is 145px wide and the compact "Ticket…" one 100px, so the full label
// needs 45px more than the compact one. The Global Dashboards group is 120px with its three buttons and
// 40px folded into one menu, so unfolding needs 80px.
const FULL = 145,
  COMPACT = 100,
  DASHBOARDS = 120,
  FOLDED = 40,
  TRAILING = 40,
  TABS = 400;

const FOLDED_ONLY: ProjectStripFit = { dashboardsFolded: true, actionCompact: false },
  BOTH: ProjectStripFit = { dashboardsFolded: true, actionCompact: true };

/** Kerf's report for a bar of `barWidth` whose tabs want `tabs` px, in the given yield stage. */
const report = (barWidth: number, fit: ProjectStripFit, tabs = TABS): ProjectTabFitMeasure => {
  const leading = fit.dashboardsFolded ? FOLDED : DASHBOARDS,
    endWidth = fit.actionCompact ? COMPACT : FULL,
    room = barWidth - leading - TRAILING - endWidth;
  return {
    overflowing: tabs > room,
    stripWidth: Math.min(tabs, room),
    endFree: Math.max(0, room - tabs),
    endWidth,
    trailingWidth: TRAILING,
    barWidth,
  };
};

it('derives the leading extent from the public report alone', () => {
  expect(projectStripLeadingExtent(report(1000, PROJECT_STRIP_FULL))).toBe(DASHBOARDS);
  expect(projectStripLeadingExtent(report(500, BOTH))).toBe(FOLDED);
  // Overflowing: the strip shrinks to its room and endFree is 0, so the extent still holds.
  expect(projectStripLeadingExtent(report(600, PROJECT_STRIP_FULL))).toBe(DASHBOARDS);
});

// HS2-RNR35H, HS2-B0RJBY: nothing yields while the tabs fit.
it('stays full while the tabs fit and does not remember a width', () => {
  const memory: ProjectTabFitMemory = {};
  expect(projectStripFit(PROJECT_STRIP_FULL, report(1000, PROJECT_STRIP_FULL), memory)).toEqual(PROJECT_STRIP_FULL);
  expect(memory).toEqual({});
});

it('folds the dashboards before the launcher compacts, one stage per overflowing report', () => {
  const memory: ProjectTabFitMemory = {};
  // 690px: the full strip overflows by 15px; folding frees 80px, so the launcher keeps its label.
  const folded = projectStripFit(PROJECT_STRIP_FULL, report(690, PROJECT_STRIP_FULL), memory);
  expect(folded).toEqual(FOLDED_ONLY);
  expect(memory).toEqual({ fullLeadingWidth: DASHBOARDS });
  expect(projectStripFit(folded, report(690, folded), memory)).toEqual(FOLDED_ONLY);
  // 600px: still overflowing once folded, so the launcher compacts next and remembers its full width.
  const compact = projectStripFit(folded, report(600, folded), memory);
  expect(compact).toEqual(BOTH);
  expect(memory).toEqual({ fullLeadingWidth: DASHBOARDS, fullEndWidth: FULL });
  // Overflowing at both stages yields nothing more.
  expect(projectStripFit(compact, report(560, compact), memory)).toBe(compact);
});

it('restores the launcher before the dashboards, each only once endFree covers it (HS2-A2CSNB, HS2-YATB6S)', () => {
  const memory: ProjectTabFitMemory = { fullLeadingWidth: DASHBOARDS, fullEndWidth: FULL };
  // 44px spare beside the compact label is less than the full label's extra 45px.
  expect(projectStripFit(BOTH, report(624, BOTH), memory)).toEqual(BOTH);
  expect(projectStripFit(BOTH, report(625, BOTH), memory)).toEqual(FOLDED_ONLY);
  // Folded with the full label: 79px spare is less than the unfolded group's extra 80px.
  expect(projectStripFit(FOLDED_ONLY, report(704, FOLDED_ONLY), memory)).toEqual(FOLDED_ONLY);
  expect(projectStripFit(FOLDED_ONLY, report(705, FOLDED_ONLY), memory)).toEqual(PROJECT_STRIP_FULL);
  // Closing a tab frees room without the bar growing; Kerf reports it as endFree as well.
  expect(projectStripFit(BOTH, report(600, BOTH, 200), memory)).toEqual(FOLDED_ONLY);
  expect(projectStripFit(FOLDED_ONLY, report(600, FOLDED_ONLY, 200), memory)).toEqual(PROJECT_STRIP_FULL);
});

it('without a remembered width a stage cannot be priced and stays yielded', () => {
  expect(projectStripFit(BOTH, report(2000, BOTH), {})).toEqual(BOTH);
  expect(projectStripFit(FOLDED_ONLY, report(2000, FOLDED_ONLY), {})).toEqual(FOLDED_ONLY);
});

it('walks wide to narrow to wide twice without oscillating, then refills after a reset', () => {
  const memory: ProjectTabFitMemory = {};
  let fit: ProjectStripFit = PROJECT_STRIP_FULL;
  // Each step settles: Kerf re-reports on the next frame after the strip's content changes.
  const step = (barWidth: number, tabs = TABS) => {
    for (let frame = 0; frame < 4; frame += 1) fit = projectStripFit(fit, report(barWidth, fit, tabs), memory);
    return fit;
  };
  for (const pass of ['first', 'second']) {
    expect(step(1000), pass).toEqual(PROJECT_STRIP_FULL);
    expect(step(690), pass).toEqual(FOLDED_ONLY);
    expect(step(600), pass).toEqual(BOTH);
    expect(step(620), pass).toEqual(BOTH);
    expect(step(640), pass).toEqual(FOLDED_ONLY);
    expect(step(700), pass).toEqual(FOLDED_ONLY);
    expect(step(1000), pass).toEqual(PROJECT_STRIP_FULL);
  }
  // Interleaved: a tab opens at a fitting width, then closes again.
  expect(step(720)).toEqual(PROJECT_STRIP_FULL);
  expect(step(720, 520)).toEqual(BOTH);
  expect(step(720)).toEqual(PROJECT_STRIP_FULL);
  // A strip remounted while yielded (memory reset) keeps its stages until it can price them.
  delete memory.fullEndWidth;
  delete memory.fullLeadingWidth;
  fit = BOTH;
  expect(step(1200)).toEqual(BOTH);
});

it('publishes only changes, and only for the desktop project bar', () => {
  const published: ProjectStripFit[] = [];
  const { onFit, reset } = projectTabActionFit((fit) => published.push(fit));
  const fit = (barWidth: number, stage: ProjectStripFit, tabs = TABS): TabBarFit => ({
    barId: PROJECT_TAB_BAR_ID,
    tabsWidth: tabs,
    free: 0,
    ...report(barWidth, stage, tabs),
  });
  onFit({ ...fit(500, PROJECT_STRIP_FULL), barId: 'terminal-drawer' });
  onFit(fit(1000, PROJECT_STRIP_FULL));
  expect(published).toEqual([]);
  onFit(fit(600, PROJECT_STRIP_FULL));
  onFit(fit(600, FOLDED_ONLY));
  onFit(fit(600, BOTH));
  onFit(fit(1000, BOTH));
  onFit(fit(1000, FOLDED_ONLY));
  onFit(fit(1000, PROJECT_STRIP_FULL));
  expect(published).toEqual([FOLDED_ONLY, BOTH, FOLDED_ONLY, PROJECT_STRIP_FULL]);
  // HS2-YATB6S: the strip leaves for mobile while yielded; the next strip starts full, and the
  // forgotten widths are remembered afresh at its next overflow.
  onFit(fit(600, PROJECT_STRIP_FULL));
  reset();
  reset();
  expect(published.slice(4)).toEqual([FOLDED_ONLY, PROJECT_STRIP_FULL]);
  onFit(fit(600, PROJECT_STRIP_FULL));
  onFit(fit(600, FOLDED_ONLY));
  onFit(fit(1000, BOTH));
  onFit(fit(1000, FOLDED_ONLY));
  expect(published.slice(6)).toEqual([FOLDED_ONLY, BOTH, FOLDED_ONLY, PROJECT_STRIP_FULL]);
});

it('HS2-A2CSNB reads TabBar fit from the public report, not private TabBar parts or app measurement', () => {
  const source = readFileSync(resolve(import.meta.dirname, 'project-tab-action-fit.ts'), 'utf8');
  expect(source).not.toMatch(/kui-tab-bar__/);
  expect(source).not.toMatch(/data-scroll-overflow/);
  // HS2-YATB6S: Kerf's endFree replaces the app's own root-width observers.
  expect(source).not.toMatch(/ResizeObserver|MutationObserver|getComputedStyle|clientWidth/);
  expect(source).toContain('TabBarFit');
  expect(source).toContain('endFree');
});
