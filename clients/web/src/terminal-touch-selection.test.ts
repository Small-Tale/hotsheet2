import { describe, expect, it, vi } from 'vitest';

import {
  createTouchSelectionController,
  terminalCellAt,
  terminalExtendedRange,
  type TerminalGridGeometry,
  terminalSelectSpan,
  terminalWordAt,
  touchSelectionEdge,
  type TouchSelectionTerminal,
} from './terminal-touch-selection';

// A 10-column, 4-row grid of 8×16 px cells at (100, 200), scrolled so buffer line 20 is on top.
const geometry: TerminalGridGeometry = {
  rect: { left: 100, top: 200, width: 80, height: 64 },
  cols: 10,
  rows: 4,
  viewportY: 20,
};
const at = (col: number, row: number) => ({ x: 100 + col * 8 + 4, y: 200 + (row - 20) * 16 + 8 });

describe('terminal touch selection geometry (HS2-EYR96N)', () => {
  it('maps a point to its buffer cell, including scrollback offset, and clamps to the grid', () => {
    expect(terminalCellAt(at(3, 21), geometry)).toEqual({ col: 3, row: 21 });
    expect(terminalCellAt({ x: 100, y: 200 }, geometry)).toEqual({ col: 0, row: 20 });
    expect(terminalCellAt({ x: 10, y: 10 }, geometry)).toEqual({ col: 0, row: 20 });
    expect(terminalCellAt({ x: 999, y: 999 }, geometry)).toEqual({ col: 9, row: 23 });
    // A CSS-scaled terminal reports a scaled rect, so cells scale with it.
    expect(
      terminalCellAt({ x: 100 + 3 * 16 + 1, y: 200 }, { ...geometry, rect: { ...geometry.rect, width: 160 } }),
    ).toEqual({
      col: 3,
      row: 20,
    });
  });

  it('finds the word under a column and nothing on separators or blank cells', () => {
    const line = 'GNU nano (8.4)   ';
    expect(terminalWordAt(line, 0)).toEqual({ start: 0, end: 2 });
    expect(terminalWordAt(line, 5)).toEqual({ start: 4, end: 7 });
    expect(terminalWordAt(line, 10)).toEqual({ start: 10, end: 12 });
    expect(terminalWordAt(line, 3)).toBeUndefined();
    expect(terminalWordAt(line, 9)).toBeUndefined();
    expect(terminalWordAt(line, 15)).toBeUndefined();
    expect(terminalWordAt(line, 40)).toBeUndefined();
    expect(terminalWordAt('a/b-c.d', 3)).toEqual({ start: 0, end: 6 });
  });

  it('extends from the anchored word in either direction and across rows', () => {
    const anchor = { start: { col: 4, row: 21 }, end: { col: 7, row: 21 } };
    expect(terminalExtendedRange(anchor, { col: 9, row: 21 }, 10)).toEqual({
      start: anchor.start,
      end: { col: 9, row: 21 },
    });
    expect(terminalExtendedRange(anchor, { col: 0, row: 21 }, 10)).toEqual({
      start: { col: 0, row: 21 },
      end: anchor.end,
    });
    expect(terminalExtendedRange(anchor, { col: 5, row: 21 }, 10)).toBe(anchor);
    expect(terminalExtendedRange(anchor, { col: 2, row: 23 }, 10)).toEqual({
      start: anchor.start,
      end: { col: 2, row: 23 },
    });
    expect(terminalExtendedRange(anchor, { col: 8, row: 20 }, 10)).toEqual({
      start: { col: 8, row: 20 },
      end: anchor.end,
    });
  });

  it('converts an inclusive range into xterm select arguments', () => {
    expect(terminalSelectSpan({ start: { col: 4, row: 21 }, end: { col: 7, row: 21 } }, 10)).toEqual({
      col: 4,
      row: 21,
      length: 4,
    });
    expect(terminalSelectSpan({ start: { col: 8, row: 20 }, end: { col: 1, row: 22 } }, 10)).toEqual({
      col: 8,
      row: 20,
      length: 14,
    });
  });
});

function fakeTerminal(lines: Record<number, string>, rendered = true) {
  const terminal = {
    cols: 10,
    geometry: vi.fn(() => (rendered ? geometry : undefined)),
    lineText: (row: number) => lines[row] ?? '',
    select: vi.fn(),
    clearSelection: vi.fn(),
  } satisfies TouchSelectionTerminal;
  return terminal;
}

describe('touch selection controller (HS2-EYR96N)', () => {
  it('walks hold → drag forward → drag back → lift → new hold → clear', () => {
    const terminal = fakeTerminal({ 21: 'GNU nano 8.4', 22: 'next line' }),
      selection = createTouchSelectionController(terminal);
    // Hold on "nano": that word alone.
    expect(selection.begin(at(5, 21))).toBe(true);
    expect(selection.selecting).toBe(true);
    expect(terminal.select).toHaveBeenLastCalledWith({ col: 4, row: 21, length: 4 });
    // Drag forward onto the next row, then back before the word: the word always stays included.
    selection.extend(at(3, 22));
    expect(terminal.select).toHaveBeenLastCalledWith({ col: 4, row: 21, length: 10 });
    selection.extend(at(0, 21));
    expect(terminal.select).toHaveBeenLastCalledWith({ col: 0, row: 21, length: 8 });
    // Lift keeps the selection for the menu but stops extending.
    selection.finish();
    expect(selection.selecting).toBe(false);
    selection.extend(at(9, 22));
    expect(terminal.select).toHaveBeenCalledTimes(3);
    expect(terminal.clearSelection).not.toHaveBeenCalled();
    // A new hold anchors afresh, on "next".
    expect(selection.begin(at(1, 22))).toBe(true);
    expect(terminal.select).toHaveBeenLastCalledWith({ col: 0, row: 22, length: 4 });
    selection.clear();
    expect(selection.selecting).toBe(false);
    expect(terminal.clearSelection).toHaveBeenCalledTimes(1);
  });

  it('selects nothing for a hold on a blank cell, and drops an earlier selection', () => {
    const terminal = fakeTerminal({ 21: 'GNU nano 8.4' }),
      selection = createTouchSelectionController(terminal);
    selection.begin(at(5, 21));
    expect(selection.begin(at(9, 23))).toBe(false);
    expect(selection.selecting).toBe(false);
    expect(terminal.clearSelection).toHaveBeenCalledTimes(1);
    selection.extend(at(5, 21));
    expect(terminal.select).toHaveBeenCalledTimes(1);
  });

  it('does nothing before the terminal renders', () => {
    const terminal = fakeTerminal({ 21: 'GNU nano 8.4' }, false),
      selection = createTouchSelectionController(terminal);
    expect(selection.begin(at(5, 21))).toBe(false);
    selection.extend(at(6, 21));
    expect(terminal.select).not.toHaveBeenCalled();
    expect(terminal.clearSelection).not.toHaveBeenCalled();
  });
});

describe('touch selection edge auto-scroll (HS2-4BARC8)', () => {
  it('reports the edge band, its direction, and a depth that saturates past the edge', () => {
    // 4 rows of 16px from y=200: the bands are y<216 (top) and y>248 (bottom).
    expect(touchSelectionEdge(230, geometry)).toEqual({ direction: 0, depth: 0 });
    expect(touchSelectionEdge(212, geometry)).toEqual({ direction: -1, depth: 4 / 64 });
    expect(touchSelectionEdge(100, geometry)).toEqual({ direction: -1, depth: 1 });
    expect(touchSelectionEdge(256, geometry)).toEqual({ direction: 1, depth: 8 / 64 });
  });

  /** A terminal whose viewport really scrolls, with test-driven animation frames. */
  function scrollingTerminal() {
    const frames: Array<(time: number) => void> = [],
      cancelled: number[] = [];
    let viewportY = 20;
    const terminal = {
      cols: 10,
      geometry: () => ({ ...geometry, viewportY }),
      lineText: (row: number) => (row === 21 ? 'GNU nano 8.4' : 'older line'),
      select: vi.fn(),
      clearSelection: vi.fn(),
      scrollLines: vi.fn((lines: number) => {
        viewportY = Math.max(0, viewportY + lines);
      }),
      frame: vi.fn((callback: (time: number) => void) => {
        const index = frames.push(callback) - 1;
        return () => cancelled.push(index);
      }),
    } satisfies TouchSelectionTerminal;
    const runFrame = (time: number) => {
      frames[frames.length - 1](time);
    };
    return { terminal, frames, cancelled, runFrame, viewport: () => viewportY };
  }

  it('walks hold → drag into the top band → scroll and re-extend → drag back → lift', () => {
    const { terminal, frames, cancelled, runFrame, viewport } = scrollingTerminal(),
      selection = createTouchSelectionController(terminal);
    selection.begin(at(5, 21));
    // Inside the grid: no auto-scroll.
    selection.extend(at(5, 22));
    expect(terminal.frame).not.toHaveBeenCalled();
    // Far past the top edge: one frame loop starts (repeated moves never start a second one).
    selection.extend({ x: 140, y: 100 });
    selection.extend({ x: 140, y: 90 });
    expect(frames).toHaveLength(1);
    runFrame(0);
    runFrame(50);
    runFrame(100);
    // At full depth (48 rows/s) 100ms earns about 4 rows; each scrolled frame re-extends the selection.
    expect(terminal.scrollLines).toHaveBeenCalled();
    expect(terminal.scrollLines.mock.calls.every(([lines]) => lines < 0)).toBe(true);
    expect(viewport()).toBeLessThan(20);
    const [, row, length] = Object.values(terminal.select.mock.lastCall![0]);
    expect(row).toBe(viewport());
    expect(length).toBeGreaterThan(8);
    // Back inside the grid: the loop stops and nothing more scrolls.
    const scrolled = terminal.scrollLines.mock.calls.length;
    // (A fixed screen point inside the grid: `at()` assumes the unscrolled viewport.)
    selection.extend({ x: 140, y: 230 });
    expect(cancelled.length).toBeGreaterThan(0);
    runFrame(400);
    expect(terminal.scrollLines.mock.calls.length).toBe(scrolled);
    // Into the bottom band, then lift: the loop is cancelled and later frames do nothing.
    selection.extend({ x: 140, y: 300 });
    selection.finish();
    runFrame(500);
    runFrame(600);
    expect(terminal.scrollLines.mock.calls.length).toBe(scrolled);
  });

  it('never auto-scrolls without an anchor, after clear, or on a terminal that cannot scroll', () => {
    const { terminal, frames } = scrollingTerminal(),
      selection = createTouchSelectionController(terminal);
    selection.extend({ x: 140, y: 100 });
    expect(frames).toHaveLength(0);
    selection.begin(at(5, 21));
    selection.clear();
    selection.extend({ x: 140, y: 100 });
    expect(frames).toHaveLength(0);
    // Empty-then-refill: a new hold anchors and auto-scrolls again.
    selection.begin(at(5, 21));
    selection.extend({ x: 140, y: 100 });
    expect(frames).toHaveLength(1);
    const plain = fakeTerminal({ 21: 'GNU nano 8.4' }),
      noScroll = createTouchSelectionController(plain);
    noScroll.begin(at(5, 21));
    expect(() => {
      noScroll.extend({ x: 140, y: 100 });
    }).not.toThrow();
  });
});
