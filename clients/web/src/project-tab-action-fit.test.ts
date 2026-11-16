import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import type { TabBarFit } from '@kerfjs/ui/wire-tab-bars';
import { expect, it } from 'vitest';

import { PROJECT_TAB_BAR_ID } from './components/project-tab-bar';
import {
  projectTabActionCompact,
  projectTabActionFit,
  type ProjectTabFitMeasure,
  type ProjectTabFitMemory,
} from './project-tab-action-fit';

// The full "New ticket…" launcher is 145px wide and the compact "Ticket…" one 100px, so the full label
// needs 45px more than the compact one.
const FULL = 145,
  COMPACT = 100;

const measure = (overrides: Partial<ProjectTabFitMeasure>): ProjectTabFitMeasure => ({
  overflowing: false,
  endWidth: FULL,
  endFree: 300,
  ...overrides,
});

// HS2-RNR35H: the launcher yields its full label only while the project tabs overflow.
it('stays full while the tabs fit and does not remember a width', () => {
  const memory: ProjectTabFitMemory = {};
  expect(projectTabActionCompact(false, measure({}), memory)).toBe(false);
  expect(memory.fullEndWidth).toBeUndefined();
});

it('goes compact when Kerf reports overflow and remembers the full launcher width', () => {
  const memory: ProjectTabFitMemory = {};
  expect(projectTabActionCompact(false, measure({ overflowing: true, endFree: 0 }), memory)).toBe(true);
  expect(memory.fullEndWidth).toBe(FULL);
});

it('stays compact until endFree covers the full label, so it cannot oscillate (HS2-A2CSNB, HS2-YATB6S)', () => {
  const memory: ProjectTabFitMemory = { fullEndWidth: FULL };
  // The compact label freed room, so the tabs fit, but the full label would take 45px back.
  expect(projectTabActionCompact(true, measure({ endWidth: COMPACT, endFree: 44 }), memory)).toBe(true);
  // Still overflowing even at the compact label.
  expect(projectTabActionCompact(true, measure({ overflowing: true, endWidth: COMPACT, endFree: 0 }), memory)).toBe(
    true,
  );
  // Exactly enough room for the full label returns to it.
  expect(projectTabActionCompact(true, measure({ endWidth: COMPACT, endFree: 45 }), memory)).toBe(false);
  // Closing a tab frees room without the bar growing; Kerf reports it as endFree as well.
  expect(projectTabActionCompact(true, measure({ endWidth: COMPACT, endFree: 125 }), memory)).toBe(false);
  expect(memory.fullEndWidth).toBe(FULL);
});

it('walks wide to narrow to wide and refills the memory after a reset', () => {
  const memory: ProjectTabFitMemory = {};
  let compact = false;
  // Everything but the end zone takes 400px of the bar while the tabs fit at their natural width.
  const step = (barWidth: number) => {
    const endWidth = compact ? COMPACT : FULL,
      spare = barWidth - 400 - endWidth;
    compact = projectTabActionCompact(
      compact,
      { overflowing: spare < 0, endWidth, endFree: Math.max(0, spare) },
      memory,
    );
    return compact;
  };
  expect(step(1000)).toBe(false);
  expect(step(540)).toBe(true);
  // 30px of spare room beside the compact label is less than the full label's extra 45px.
  expect(step(530)).toBe(true);
  expect(step(545)).toBe(false);
  expect(step(530)).toBe(true);
  // A strip remounted while compact (memory reset) keeps the compact label until it can price the full one.
  delete memory.fullEndWidth;
  expect(step(1200)).toBe(true);
});

it('publishes only changes, and only for the desktop project bar', () => {
  const published: boolean[] = [];
  const { onFit, reset } = projectTabActionFit((compact) => published.push(compact));
  const fit = (overrides: Partial<TabBarFit>): TabBarFit => ({
    barId: PROJECT_TAB_BAR_ID,
    overflowing: false,
    stripWidth: 300,
    tabsWidth: 300,
    free: 0,
    trailingWidth: 40,
    endWidth: FULL,
    barWidth: 900,
    endFree: 300,
    ...overrides,
  });
  onFit(fit({ barId: 'terminal-drawer', overflowing: true }));
  onFit(fit({}));
  expect(published).toEqual([]);
  onFit(fit({ overflowing: true, endFree: 0 }));
  onFit(fit({ overflowing: true, endWidth: COMPACT, endFree: 0 }));
  onFit(fit({ endWidth: COMPACT, endFree: 20 }));
  onFit(fit({ endWidth: COMPACT, endFree: 60 }));
  expect(published).toEqual([true, false]);
  // HS2-YATB6S: the strip leaves for mobile while compact; the next strip starts from the full label,
  // and the forgotten width is remembered afresh at its next overflow.
  onFit(fit({ overflowing: true, endFree: 0 }));
  reset();
  reset();
  expect(published).toEqual([true, false, true, false]);
  onFit(fit({ endFree: 10 }));
  onFit(fit({ overflowing: true, endFree: 0 }));
  onFit(fit({ endWidth: COMPACT, endFree: 44 }));
  onFit(fit({ endWidth: COMPACT, endFree: 45 }));
  expect(published).toEqual([true, false, true, false, true, false]);
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
