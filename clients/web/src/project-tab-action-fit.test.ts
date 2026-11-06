import { expect, it } from 'vitest';

import { projectTabActionCompact, type ProjectTabFitMemory } from './project-tab-action-fit';

// HS2-RNR35H: the launcher yields its full label only while the project tabs overflow.
it('stays full while the tabs fit and remembers the full end-zone width', () => {
  const memory: ProjectTabFitMemory = {};
  expect(projectTabActionCompact(false, { overflow: 0, free: 120, endWidth: 178 }, memory)).toBe(false);
  expect(memory.fullEndWidth).toBe(178);
  // Sub-pixel rounding is not overflow.
  expect(projectTabActionCompact(false, { overflow: 1, free: 0, endWidth: 178 }, memory)).toBe(false);
});

it('goes compact when the strip scrolls', () => {
  const memory: ProjectTabFitMemory = {};
  expect(projectTabActionCompact(false, { overflow: 22, free: 0, endWidth: 178 }, memory)).toBe(true);
  expect(memory.fullEndWidth).toBe(178);
});

it('stays compact until the full label fits again, so it cannot oscillate', () => {
  const memory: ProjectTabFitMemory = { fullEndWidth: 178 };
  // Compact freed 45px; the tabs now fit with 23px spare, which the full label would take back and more.
  expect(projectTabActionCompact(true, { overflow: 0, free: 23, endWidth: 133 }, memory)).toBe(true);
  // Still overflowing even at the compact label.
  expect(projectTabActionCompact(true, { overflow: 4, free: 0, endWidth: 133 }, memory)).toBe(true);
  // Exactly enough room for the full end zone returns to the full label.
  expect(projectTabActionCompact(true, { overflow: 0, free: 45, endWidth: 133 }, memory)).toBe(false);
  // The remembered width is updated only while full.
  expect(memory.fullEndWidth).toBe(178);
});

it('walks wide to narrow to wide and refills the memory after a reset', () => {
  const memory: ProjectTabFitMemory = {};
  let compact = false;
  const step = (overflow: number, free: number) => {
    compact = projectTabActionCompact(compact, { overflow, free, endWidth: compact ? 133 : 178 }, memory);
    return compact;
  };
  expect(step(0, 300)).toBe(false);
  expect(step(22, 0)).toBe(true);
  expect(step(0, 10)).toBe(true);
  expect(step(0, 200)).toBe(false);
  expect(step(30, 0)).toBe(true);
  // A strip remounted while compact (memory reset) keeps the compact label until it can price the full one.
  delete memory.fullEndWidth;
  expect(step(0, 500)).toBe(true);
});
