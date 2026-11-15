/**
 * Search within a terminal (HS2-TWZSZF, HS2-M5QPED).
 *
 * The drawer rail's collapsible search field asks the selected terminal's interactive viewport to
 * find text through a DOM event, as the clipboard and clear actions do. The viewport runs xterm's
 * search addon, which highlights every match, selects the active one and scrolls it into view:
 * typing searches incrementally, Enter finds the next match and Shift-Enter the previous one, and
 * an empty query clears the highlights and the selection. The viewport reports each change of its
 * results (including re-counts after new output) with a bubbling results event, which the drawer
 * shows as a match count beside the field.
 */
import type { SearchFieldModel } from '@kerfjs/ui/search-field-model';

import type { TerminalDrawerSearch } from './components/terminal-drawer';

/** Event dispatched on an interactive viewport to search its buffer. */
export const TERMINAL_SEARCH_EVENT = 'hotsheet-terminal-search';
/** Bubbling event a viewport dispatches whenever its search results change (HS2-M5QPED). */
export const TERMINAL_SEARCH_RESULTS_EVENT = 'hotsheet-terminal-search-results';
/** Bubbling event a viewport dispatches once it can answer searches, so an open search can reach it. */
export const TERMINAL_SEARCH_READY_EVENT = 'hotsheet-terminal-search-ready';
/** Matches highlighted at most; xterm's addon stops counting there. */
export const TERMINAL_SEARCH_HIGHLIGHT_LIMIT = 1000;
/** xterm's minimum text contrast while matches are highlighted (WCAG AA), so text stays legible on them. */
export const TERMINAL_SEARCH_MINIMUM_CONTRAST = 4.5;

export type TerminalSearchDirection = 'next' | 'previous';

export interface TerminalSearchDetail {
  query: string;
  direction: TerminalSearchDirection;
  /** Extend the current match while typing instead of moving past it. */
  incremental: boolean;
  /** Set by the viewport: whether the query matched anything; undefined when no viewport answered. */
  found?: boolean;
}

/** One results report: the active match's index (-1 when none is active) and the match count. */
export interface TerminalSearchResults {
  resultIndex: number;
  resultCount: number;
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

/** The match count shown beside the field ("3 of 12", "No matches"); undefined shows nothing. */
export function terminalSearchCountText(
  results: TerminalSearchResults | undefined,
  limit = TERMINAL_SEARCH_HIGHLIGHT_LIMIT,
): string | undefined {
  if (!results) return undefined;
  const { resultIndex, resultCount } = results;
  if (resultCount <= 0) return 'No matches';
  const total = resultCount >= limit ? `${limit}+` : String(resultCount);
  if (resultIndex >= 0) return `${resultIndex + 1} of ${total}`;
  return resultCount === 1 ? '1 match' : `${total} matches`;
}

/**
 * The drawer's search prop from the field model, its open state, and the searched terminal's results
 * (HS2-M5QPED). Production and the catalog demo (HS2-174S6X) present the search through this one mapping.
 */
export function terminalDrawerSearch(
  model: SearchFieldModel,
  open: boolean,
  results: TerminalSearchResults | undefined,
): TerminalDrawerSearch {
  return { model, open, count: terminalSearchCountText(results), hasMatches: (results?.resultCount ?? 0) > 0 };
}

export interface TerminalSearchSyncOptions {
  /** The viewport the search applies to now: the drawer's visible interactive terminal, if any. */
  viewport: () => EventTarget | null | undefined;
  /** The query to apply: the field's text while it is open, otherwise empty. */
  query: () => string;
  /** Receives the searched viewport's results; undefined while nothing is searched. */
  publish: (results: TerminalSearchResults | undefined) => void;
}

export interface TerminalSearchSync {
  /** Apply the current query to the current viewport, clearing a viewport the search left. */
  sync(): void;
  /** Step to the next or previous match of the current query. */
  step(direction: TerminalSearchDirection): void;
  /** A viewport reported results; only the searched viewport's reports are published. */
  receive(viewport: EventTarget, results: TerminalSearchResults): void;
  /** A viewport became able to search; an open search reaches it if it is the current one. */
  ready(viewport: EventTarget): void;
}

/**
 * Keep one terminal search applied to the drawer's selected terminal (HS2-M5QPED). Switching
 * terminals clears the left terminal's highlights and searches the new one; emptying or closing the
 * field clears the highlights and the count. A viewport whose runtime is still loading answers
 * nothing, so it is searched again when it reports that it is ready.
 */
export function createTerminalSearchSync({ viewport, query, publish }: TerminalSearchSyncOptions): TerminalSearchSync {
  let searched: { viewport: EventTarget; query: string } | undefined;
  const leave = () => {
    if (!searched) return;
    const left = searched.viewport;
    searched = undefined;
    publish(undefined);
    searchTerminalViewport(left, '', 'next', true);
  };
  const sync = () => {
    const text = query();
    // Only a search this sync applied is ever cleared; the terminal's own selection is left alone.
    if (!searched && !text) return;
    const target = viewport() ?? undefined;
    if (searched && searched.viewport !== target) leave();
    if (!target || searched?.query === text) return;
    if (!text) {
      leave();
      return;
    }
    // Results arrive synchronously while the viewport searches, so it becomes the searched one first.
    const previous = searched;
    searched = { viewport: target, query: text };
    if (searchTerminalViewport(target, text, 'next', true) === undefined) {
      searched = previous;
      if (!previous) publish(undefined);
    }
  };
  return {
    sync,
    step(direction) {
      sync();
      if (!searched) return;
      searchTerminalViewport(searched.viewport, searched.query, direction, false);
    },
    receive(target, results) {
      if (searched?.viewport === target) publish(results);
    },
    ready(target) {
      if (target !== viewport()) return;
      // A remounted runtime starts with a fresh emulator, so search it again; a resumed one keeps its
      // match, since an incremental search extends the current selection.
      if (searched?.viewport === target) {
        searched = undefined;
        publish(undefined);
      }
      sync();
    },
  };
}
