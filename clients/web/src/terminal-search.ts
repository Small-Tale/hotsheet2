/**
 * Search within a terminal (HS2-TWZSZF).
 *
 * The drawer rail's collapsible search field asks the selected terminal's interactive viewport to
 * find text through a DOM event, as the clipboard and clear actions do. The viewport runs xterm's
 * search addon, which selects the active match and scrolls it into view: typing searches
 * incrementally, Enter finds the next match and Shift-Enter the previous one, and an empty query
 * clears the selection.
 */
/** Event dispatched on an interactive viewport to search its buffer. */
export const TERMINAL_SEARCH_EVENT = 'hotsheet-terminal-search';

export type TerminalSearchDirection = 'next' | 'previous';

export interface TerminalSearchDetail {
  query: string;
  direction: TerminalSearchDirection;
  /** Extend the current match while typing instead of moving past it. */
  incremental: boolean;
  /** Set by the viewport: whether the query matched anything; undefined when no viewport answered. */
  found?: boolean;
}

/** The SearchField id of the terminal drawer's search. */
export const TERMINAL_SEARCH_FIELD_ID = 'terminal-search';

/** Search one viewport; returns whether it found a match, or undefined when no viewport answered. */
export function searchTerminalViewport(
  viewport: EventTarget,
  query: string,
  direction: TerminalSearchDirection,
  incremental: boolean,
): boolean | undefined {
  const detail: TerminalSearchDetail = { query, direction, incremental };
  viewport.dispatchEvent(new CustomEvent(TERMINAL_SEARCH_EVENT, { detail }));
  return detail.found;
}
