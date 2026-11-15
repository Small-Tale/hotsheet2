import { describe, expect, it } from 'vitest';

import {
  createTerminalSearchSync,
  searchTerminalViewport,
  TERMINAL_SEARCH_EVENT,
  terminalSearchCountText,
  type TerminalSearchDetail,
  type TerminalSearchResults,
} from './terminal-search';

describe('terminal search (HS2-TWZSZF)', () => {
  it('asks a viewport to search and reports whether it matched', () => {
    const viewport = new EventTarget(),
      requests: TerminalSearchDetail[] = [];
    // No mounted emulator answers: the result is unknown, not "no match".
    expect(searchTerminalViewport(viewport, 'ls', 'next', true)).toBeUndefined();
    viewport.addEventListener(TERMINAL_SEARCH_EVENT, (event) => {
      const detail = (event as CustomEvent<TerminalSearchDetail>).detail;
      requests.push({ ...detail });
      detail.found = detail.query === 'ls';
    });
    expect(searchTerminalViewport(viewport, 'ls', 'next', true)).toBe(true);
    expect(searchTerminalViewport(viewport, 'missing', 'previous', false)).toBe(false);
    expect(searchTerminalViewport(viewport, '', 'next', true)).toBe(false);
    expect(requests).toEqual([
      { query: 'ls', direction: 'next', incremental: true },
      { query: 'missing', direction: 'previous', incremental: false },
      { query: '', direction: 'next', incremental: true },
    ]);
  });
});

describe('terminal search match count (HS2-M5QPED)', () => {
  it('formats the active match, totals, no matches, and the highlight limit', () => {
    expect(terminalSearchCountText(undefined)).toBeUndefined();
    expect(terminalSearchCountText({ resultIndex: -1, resultCount: 0 })).toBe('No matches');
    expect(terminalSearchCountText({ resultIndex: 2, resultCount: 12 })).toBe('3 of 12');
    expect(terminalSearchCountText({ resultIndex: 0, resultCount: 1 })).toBe('1 of 1');
    expect(terminalSearchCountText({ resultIndex: -1, resultCount: 1 })).toBe('1 match');
    expect(terminalSearchCountText({ resultIndex: -1, resultCount: 7 })).toBe('7 matches');
    expect(terminalSearchCountText({ resultIndex: 4, resultCount: 1000 })).toBe('5 of 1000+');
    expect(terminalSearchCountText({ resultIndex: -1, resultCount: 1000 })).toBe('1000+ matches');
    expect(terminalSearchCountText({ resultIndex: 0, resultCount: 3 }, 3)).toBe('1 of 3+');
  });
});

/** A viewport whose emulator counts occurrences of the query in its text and reports them like the runtime. */
class FakeViewport extends EventTarget {
  requests: TerminalSearchDetail[] = [];
  active = -1;
  ready = true;
  onResults?: (viewport: EventTarget, results: TerminalSearchResults) => void;

  constructor(public text: string) {
    super();
    this.addEventListener(TERMINAL_SEARCH_EVENT, (event) => {
      if (!this.ready) return;
      const detail = (event as CustomEvent<TerminalSearchDetail>).detail;
      this.requests.push({ ...detail });
      if (!detail.query) {
        this.active = -1;
        detail.found = false;
        return;
      }
      const count = this.text.split(detail.query).length - 1;
      if (!count) this.active = -1;
      else if (this.active < 0) this.active = 0;
      else if (!detail.incremental) this.active = (this.active + (detail.direction === 'next' ? 1 : count - 1)) % count;
      else this.active = Math.min(this.active, count - 1);
      detail.found = count > 0;
      this.onResults?.(this, { resultIndex: this.active, resultCount: count });
    });
  }

  queries() {
    return this.requests.map((request) => request.query);
  }
}

function harness() {
  const state = {
    open: true,
    query: '',
    current: undefined as FakeViewport | undefined,
    published: [] as (TerminalSearchResults | undefined)[],
  };
  const sync = createTerminalSearchSync({
    viewport: () => state.current,
    query: () => (state.open ? state.query : ''),
    publish: (results) => state.published.push(results),
  });
  const attach = (viewport: FakeViewport) => {
    viewport.onResults = (target, results) => {
      sync.receive(target, results);
    };
    return viewport;
  };
  const last = () => state.published.at(-1);
  return { state, sync, attach, last };
}

describe('terminal search sync (HS2-M5QPED)', () => {
  it('searches while typing, re-counts query edits, and clears on an emptied field', () => {
    const { state, sync, attach, last } = harness(),
      viewport = attach(new FakeViewport('nano nano nano'));
    state.current = viewport;
    // Opening an empty field leaves the terminal's own selection alone: nothing is dispatched.
    sync.sync();
    expect(viewport.requests).toEqual([]);
    state.query = 'nano';
    sync.sync();
    expect(last()).toEqual({ resultIndex: 0, resultCount: 3 });
    // Repeating the same state does not search again.
    sync.sync();
    expect(viewport.queries()).toEqual(['nano']);
    state.query = 'nano nano';
    sync.sync();
    expect(last()).toEqual({ resultIndex: 0, resultCount: 1 });
    state.query = 'absent';
    sync.sync();
    expect(last()).toEqual({ resultIndex: -1, resultCount: 0 });
    state.query = '';
    sync.sync();
    expect(last()).toBeUndefined();
    expect(viewport.queries()).toEqual(['nano', 'nano nano', 'absent', '']);
    // Empty then refill.
    state.query = 'nano';
    sync.sync();
    expect(last()).toEqual({ resultIndex: 0, resultCount: 3 });
  });

  it('steps through matches and wraps in both directions', () => {
    const { state, sync, attach, last } = harness(),
      viewport = attach(new FakeViewport('a a a'));
    state.current = viewport;
    state.query = 'a';
    sync.step('next');
    // The step first applies the typed query, then moves past its first match.
    expect(viewport.requests.map(({ query, direction, incremental }) => [query, direction, incremental])).toEqual([
      ['a', 'next', true],
      ['a', 'next', false],
    ]);
    expect(last()).toEqual({ resultIndex: 1, resultCount: 3 });
    sync.step('next');
    sync.step('next');
    expect(last()).toEqual({ resultIndex: 0, resultCount: 3 });
    sync.step('previous');
    expect(last()).toEqual({ resultIndex: 2, resultCount: 3 });
    // An emptied field clears once, and a later step searches nothing.
    state.query = '';
    const before = viewport.requests.length;
    sync.step('next');
    expect(viewport.queries().slice(before)).toEqual(['']);
    sync.step('previous');
    expect(viewport.requests).toHaveLength(before + 1);
  });

  it('publishes re-counts the searched viewport reports after new output, and ignores other viewports', () => {
    const { state, sync, attach, last } = harness(),
      viewport = attach(new FakeViewport('ok')),
      other = attach(new FakeViewport('ok ok'));
    state.current = viewport;
    state.query = 'ok';
    sync.sync();
    expect(last()).toEqual({ resultIndex: 0, resultCount: 1 });
    // New output arrives: the runtime's addon re-counts on its own and reports it.
    sync.receive(viewport, { resultIndex: 0, resultCount: 4 });
    expect(last()).toEqual({ resultIndex: 0, resultCount: 4 });
    sync.receive(other, { resultIndex: 0, resultCount: 9 });
    expect(state.published).toHaveLength(2);
  });

  it('clears highlights and the count when the field closes, and restores them when it reopens', () => {
    const { state, sync, attach, last } = harness(),
      viewport = attach(new FakeViewport('x x'));
    state.current = viewport;
    state.query = 'x';
    sync.sync();
    state.open = false;
    sync.sync();
    expect(viewport.queries()).toEqual(['x', '']);
    expect(last()).toBeUndefined();
    state.open = true;
    sync.sync();
    expect(last()).toEqual({ resultIndex: 0, resultCount: 2 });
  });

  it('moves the search to a newly selected terminal and clears the one it left', () => {
    const { state, sync, attach, last } = harness(),
      first = attach(new FakeViewport('log log')),
      second = attach(new FakeViewport('log'));
    state.current = first;
    state.query = 'log';
    sync.sync();
    state.current = second;
    sync.sync();
    expect(first.queries()).toEqual(['log', '']);
    expect(second.queries()).toEqual(['log']);
    expect(last()).toEqual({ resultIndex: 0, resultCount: 1 });
    // The grid has no terminal: the search clears and the count disappears.
    state.current = undefined;
    sync.sync();
    expect(second.queries()).toEqual(['log', '']);
    expect(last()).toBeUndefined();
    // Back to the first terminal: searched again, from scratch.
    state.current = first;
    sync.sync();
    expect(first.queries()).toEqual(['log', '', 'log']);
    expect(last()).toEqual({ resultIndex: 0, resultCount: 2 });
    // Late reports from a terminal the search left are ignored.
    const count = state.published.length;
    sync.receive(second, { resultIndex: 0, resultCount: 5 });
    expect(state.published).toHaveLength(count);
  });

  it('reaches a terminal whose runtime loads after the query, and a remounted runtime', () => {
    const { state, sync, attach, last } = harness(),
      viewport = attach(new FakeViewport('cargo cargo'));
    viewport.ready = false;
    state.current = viewport;
    state.query = 'cargo';
    sync.sync();
    // Nothing answered, so nothing is searched and no count shows.
    expect(viewport.requests).toEqual([]);
    expect(last()).toBeUndefined();
    viewport.ready = true;
    // A runtime that is not the current terminal is ignored.
    sync.ready(new FakeViewport('cargo'));
    expect(viewport.requests).toEqual([]);
    sync.ready(viewport);
    expect(last()).toEqual({ resultIndex: 0, resultCount: 2 });
    // A remount starts a fresh emulator: the same element is searched again.
    viewport.active = -1;
    sync.ready(viewport);
    expect(viewport.queries()).toEqual(['cargo', 'cargo']);
    expect(last()).toEqual({ resultIndex: 0, resultCount: 2 });
    // With an empty field, readiness searches nothing.
    state.query = '';
    sync.sync();
    const before = viewport.requests.length;
    sync.ready(viewport);
    expect(viewport.requests).toHaveLength(before);
  });
});
