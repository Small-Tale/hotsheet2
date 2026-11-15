import { describe, expect, it } from 'vitest';

import {
  type TerminalRailMeasure,
  type TerminalRailMemory,
  terminalSearchCountCompact,
} from './terminal-search-count-fit';

const NARROW = 480;
const measure = (railWidth: number, endOverflow: number, countShown = true): TerminalRailMeasure => ({
  railWidth,
  narrowWidth: NARROW,
  endOverflow,
  countShown,
});

describe('terminal search count fit (HS2-HPDX81)', () => {
  it('keeps the full count on a wide rail whose end fits', () => {
    const memory: TerminalRailMemory = {};
    expect(terminalSearchCountCompact(false, measure(900, -12), memory)).toBe(false);
    expect(terminalSearchCountCompact(false, measure(900, 0), memory)).toBe(false);
    expect(memory.fullWidth).toBeUndefined();
  });

  it('is always compact on a phone-width rail, up to and including the threshold', () => {
    const memory: TerminalRailMemory = {};
    expect(terminalSearchCountCompact(false, measure(390, -20), memory)).toBe(true);
    expect(terminalSearchCountCompact(false, measure(NARROW, -20, false), memory)).toBe(true);
    expect(terminalSearchCountCompact(true, measure(NARROW, -20), memory)).toBe(true);
    // Leaving the phone width without an overflow memory returns to the full count.
    expect(terminalSearchCountCompact(true, measure(NARROW + 1, -20), memory)).toBe(false);
  });

  it('turns compact when a wider rail overflows with the count shown, and remembers the width it needed', () => {
    const memory: TerminalRailMemory = {};
    expect(terminalSearchCountCompact(false, measure(600, 35), memory)).toBe(true);
    expect(memory.fullWidth).toBe(635);
  });

  it('ignores an overflow while no count control is shown', () => {
    const memory: TerminalRailMemory = {};
    expect(terminalSearchCountCompact(false, measure(600, 35, false), memory)).toBe(false);
    expect(memory.fullWidth).toBeUndefined();
  });

  it('stays compact until the rail reaches the remembered width, so it never flips back and forth', () => {
    const memory: TerminalRailMemory = {};
    let compact = terminalSearchCountCompact(false, measure(600, 35), memory);
    // The compact rail fits now, but the full count would overflow it again.
    compact = terminalSearchCountCompact(compact, measure(600, -40, false), memory);
    expect(compact).toBe(true);
    compact = terminalSearchCountCompact(compact, measure(634, -40, false), memory);
    expect(compact).toBe(true);
    compact = terminalSearchCountCompact(compact, measure(635, -40, false), memory);
    expect(compact).toBe(false);
  });

  it('walks wide, narrow, and wide again, re-learning the width after a fresh overflow', () => {
    const memory: TerminalRailMemory = {};
    let compact = false;
    const step = (railWidth: number, endOverflow: number, countShown: boolean) =>
      (compact = terminalSearchCountCompact(compact, measure(railWidth, endOverflow, countShown), memory));
    expect(step(1200, -300, true)).toBe(false);
    expect(step(390, 80, true)).toBe(true);
    // The phone rail never records an overflow width, so widening restores the full count at once.
    expect(memory.fullWidth).toBeUndefined();
    expect(step(1200, -300, false)).toBe(false);
    // A longer count overflows a medium rail: compact, then full again only once it is wide enough.
    expect(step(560, 20, true)).toBe(true);
    expect(step(570, -60, false)).toBe(true);
    expect(step(580, -70, false)).toBe(false);
    // The same rail overflowing again re-learns the needed width instead of oscillating.
    expect(step(580, 10, true)).toBe(true);
    expect(memory.fullWidth).toBe(590);
    expect(step(585, -60, false)).toBe(true);
  });
});
