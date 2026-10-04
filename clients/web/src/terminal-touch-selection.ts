/**
 * Touch text selection on an interactive terminal (HS2-EYR96N).
 *
 * xterm selects with a mouse drag, which a finger cannot do: a finger drag scrolls
 * (`terminal-touch-scroll.ts`). The conventional mobile gesture is used instead: a still long-press
 * (`terminal-long-press.ts`) selects the word under the finger, continuing to drag extends the range
 * from that word to the cell under the finger, and lifting opens the terminal edit menu, which can copy
 * the selection. Selection itself is xterm's (`terminal.select`), so it renders, survives output, and
 * reads back exactly like a desktop mouse selection.
 */

/** A buffer cell: `row` is an absolute buffer line (scrollback included), `col` a column. */
export interface TerminalCell {
  col: number;
  row: number;
}

/** An inclusive range of buffer cells, `start` never after `end` in reading order. */
export interface TerminalCellRange {
  start: TerminalCell;
  end: TerminalCell;
}

/** Arguments for xterm's `terminal.select(column, row, length)`. */
export interface TerminalSelectSpan {
  col: number;
  row: number;
  length: number;
}

/** Characters that end a word, matching xterm's default double-click `wordSeparator` plus whitespace. */
export const TERMINAL_WORD_SEPARATORS = ' ()[]{}\',"`';

/** Where the terminal's rendered grid sits and how far its viewport is scrolled. */
export interface TerminalGridGeometry {
  /** The `.xterm-screen` client rect; it already includes any CSS scale on the terminal. */
  rect: Pick<DOMRect, 'left' | 'top' | 'width' | 'height'>;
  cols: number;
  rows: number;
  /** The buffer line shown in the top viewport row. */
  viewportY: number;
}

/** The buffer cell under a viewport point, clamped to the visible grid. */
export function terminalCellAt(point: { x: number; y: number }, geometry: TerminalGridGeometry): TerminalCell {
  const { rect, cols, rows, viewportY } = geometry,
    cellWidth = rect.width / cols,
    cellHeight = rect.height / rows,
    clamp = (value: number, max: number) => Math.min(Math.max(value, 0), max);
  return {
    col: clamp(Math.floor((point.x - rect.left) / cellWidth), cols - 1),
    row: viewportY + clamp(Math.floor((point.y - rect.top) / cellHeight), rows - 1),
  };
}

/**
 * The word containing `col` in `line`, as inclusive columns; undefined on a separator or blank cell,
 * where a long-press selects nothing.
 */
export function terminalWordAt(
  line: string,
  col: number,
  separators = TERMINAL_WORD_SEPARATORS,
): { start: number; end: number } | undefined {
  const isWord = (index: number) => index >= 0 && index < line.length && !separators.includes(line[index]);
  if (!isWord(col)) return undefined;
  let start = col,
    end = col;
  while (isWord(start - 1)) start -= 1;
  while (isWord(end + 1)) end += 1;
  return { start, end };
}

function cellIndex(cell: TerminalCell, cols: number): number {
  return cell.row * cols + cell.col;
}

/**
 * The range from an anchored word to the cell under the finger: dragging forward keeps the word's
 * start, dragging backward keeps its end, so the held word always stays selected.
 */
export function terminalExtendedRange(anchor: TerminalCellRange, point: TerminalCell, cols: number): TerminalCellRange {
  if (cellIndex(point, cols) < cellIndex(anchor.start, cols)) return { start: point, end: anchor.end };
  if (cellIndex(point, cols) > cellIndex(anchor.end, cols)) return { start: anchor.start, end: point };
  return anchor;
}

/** The `terminal.select` arguments covering an inclusive range. */
export function terminalSelectSpan(range: TerminalCellRange, cols: number): TerminalSelectSpan {
  return {
    col: range.start.col,
    row: range.start.row,
    length: cellIndex(range.end, cols) - cellIndex(range.start, cols) + 1,
  };
}

/** The slice of an xterm terminal the selection controller drives. */
export interface TouchSelectionTerminal {
  readonly cols: number;
  /** Grid geometry at the moment of the touch, or undefined when the terminal is not rendered. */
  geometry(): TerminalGridGeometry | undefined;
  /** The text of an absolute buffer line, untrimmed. */
  lineText(row: number): string;
  select(span: TerminalSelectSpan): void;
  clearSelection(): void;
}

export interface TouchSelectionController {
  /** A long-press fired at `point`: select the word there. Returns whether a word was selected. */
  begin(point: { x: number; y: number }): boolean;
  /** The held finger moved: extend the selection from the anchored word. */
  extend(point: { x: number; y: number }): void;
  /** The finger lifted: stop extending but keep the selection for the edit menu's Copy. */
  finish(): void;
  /** Drop the selection (a new touch on the terminal, teardown). */
  clear(): void;
  /** Whether the current long-press anchored a word that drags extend. */
  readonly selecting: boolean;
}

export function createTouchSelectionController(terminal: TouchSelectionTerminal): TouchSelectionController {
  let anchor: TerminalCellRange | undefined;
  const apply = (range: TerminalCellRange) => {
    terminal.select(terminalSelectSpan(range, terminal.cols));
  };
  return {
    begin(point) {
      anchor = undefined;
      const geometry = terminal.geometry();
      if (!geometry) return false;
      const cell = terminalCellAt(point, geometry),
        word = terminalWordAt(terminal.lineText(cell.row), cell.col);
      if (!word) {
        terminal.clearSelection();
        return false;
      }
      anchor = { start: { col: word.start, row: cell.row }, end: { col: word.end, row: cell.row } };
      apply(anchor);
      return true;
    },
    extend(point) {
      const geometry = anchor && terminal.geometry();
      if (!anchor || !geometry) return;
      apply(terminalExtendedRange(anchor, terminalCellAt(point, geometry), terminal.cols));
    },
    finish() {
      anchor = undefined;
    },
    clear() {
      anchor = undefined;
      terminal.clearSelection();
    },
    get selecting() {
      return anchor !== undefined;
    },
  };
}
