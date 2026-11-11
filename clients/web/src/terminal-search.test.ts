import { describe, expect, it } from 'vitest';

import { searchTerminalViewport, TERMINAL_SEARCH_EVENT, type TerminalSearchDetail } from './terminal-search';

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
