import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

import { expect, it } from 'vitest';

import { projectTabActionCompact, type ProjectTabFitMeasure, type ProjectTabFitMemory } from './project-tab-action-fit';

const measure = (overrides: Partial<ProjectTabFitMeasure>): ProjectTabFitMeasure => ({
  overflowing: false,
  barWidth: 900,
  stripWidth: 300,
  tabsWidth: 300,
  ...overrides,
});

// HS2-RNR35H: the launcher yields its full label only while the project tabs overflow.
it('stays full while the tabs fit and does not price spare room as overhead', () => {
  const memory: ProjectTabFitMemory = {};
  expect(projectTabActionCompact(false, measure({}), memory)).toBe(false);
  // An adjacent strip is as wide as its tabs, so the bar's spare room is not the full label's overhead.
  expect(memory.fullOverhead).toBeUndefined();
});

it('goes compact when Kerf reports overflow and remembers the full-label overhead', () => {
  const memory: ProjectTabFitMemory = {};
  expect(
    projectTabActionCompact(
      false,
      measure({ overflowing: true, barWidth: 600, stripWidth: 200, tabsWidth: 260 }),
      memory,
    ),
  ).toBe(true);
  expect(memory.fullOverhead).toBe(400);
});

it('stays compact until the strip would fit beside the full label, so it cannot oscillate (HS2-A2CSNB)', () => {
  const memory: ProjectTabFitMemory = { fullOverhead: 400 };
  // The compact label freed 45px, so the tabs fit, but the full label would take it back.
  expect(projectTabActionCompact(true, measure({ barWidth: 640, stripWidth: 260, tabsWidth: 260 }), memory)).toBe(true);
  // Still overflowing even at the compact label.
  expect(
    projectTabActionCompact(
      true,
      measure({ overflowing: true, barWidth: 600, stripWidth: 240, tabsWidth: 260 }),
      memory,
    ),
  ).toBe(true);
  // Exactly enough room for the tabs beside the full-label overhead returns to the full label.
  expect(projectTabActionCompact(true, measure({ barWidth: 660, stripWidth: 260, tabsWidth: 260 }), memory)).toBe(
    false,
  );
  // Closing a tab frees room without the bar growing.
  expect(projectTabActionCompact(true, measure({ barWidth: 640, stripWidth: 180, tabsWidth: 180 }), memory)).toBe(
    false,
  );
  expect(memory.fullOverhead).toBe(400);
});

it('walks wide to narrow to wide and refills the memory after a reset', () => {
  const memory: ProjectTabFitMemory = {};
  let compact = false;
  const step = (barWidth: number, overflowing = false) => {
    const tabsWidth = 260,
      overhead = compact ? 355 : 400;
    compact = projectTabActionCompact(
      compact,
      { overflowing, barWidth, stripWidth: Math.min(tabsWidth, barWidth - overhead), tabsWidth },
      memory,
    );
    return compact;
  };
  expect(step(1000)).toBe(false);
  expect(step(640, true)).toBe(true);
  expect(step(650)).toBe(true);
  expect(step(700)).toBe(false);
  expect(step(600, true)).toBe(true);
  // A strip remounted while compact (memory reset) keeps the compact label until it can price the full one.
  delete memory.fullOverhead;
  expect(step(1200)).toBe(true);
});

it('HS2-A2CSNB reads TabBar fit from the public signal, not private TabBar parts', () => {
  const source = readFileSync(resolve(import.meta.dirname, 'project-tab-action-fit.ts'), 'utf8');
  expect(source).not.toMatch(/kui-tab-bar__/);
  expect(source).not.toMatch(/data-scroll-overflow/);
  expect(source).toContain('TabBarFit');
});
