import { describe, expect, it, vi } from 'vitest';

import {
  createTouchSelectionController,
  terminalCellAt,
  terminalExtendedRange,
  type TerminalGridGeometry,
  terminalSelectSpan,
  terminalWordAt,
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
