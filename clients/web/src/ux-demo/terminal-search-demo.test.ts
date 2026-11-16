import { describe, expect, it } from 'vitest';

import {
  applyTerminalSearchDemoState,
  resetTerminalDrawerDemo,
  stepTerminalSearchDemo,
  terminalSearchDemoModel,
  terminalSearchDemoOpen,
  terminalSearchDemoResults,
  terminalSearchDemoState,
} from './terminal-components-demo';

describe('terminal drawer search demo (HS2-174S6X)', () => {
  const snapshot = () => ({
    state: terminalSearchDemoState.value,
    open: terminalSearchDemoOpen.value,
    query: terminalSearchDemoModel.state.value.query,
    results: terminalSearchDemoResults.value,
  });

  it('applies each state to the real field model and resets to collapsed', () => {
    applyTerminalSearchDemoState('open');
    expect(snapshot()).toEqual({ state: 'open', open: true, query: '', results: undefined });
    applyTerminalSearchDemoState('count');
    expect(snapshot()).toEqual({
      state: 'count',
      open: true,
      query: 'root',
      results: { resultIndex: 2, resultCount: 12 },
    });
    applyTerminalSearchDemoState('none');
    expect(snapshot()).toMatchObject({ open: true, query: 'webpack', results: { resultIndex: -1, resultCount: 0 } });
    applyTerminalSearchDemoState('limit');
    expect(snapshot()).toMatchObject({ results: { resultIndex: -1, resultCount: 1000 } });
    resetTerminalDrawerDemo();
    expect(snapshot()).toEqual({ state: 'collapsed', open: false, query: '', results: undefined });
  });

  it('steps through matches with wraparound and ignores steps without matches', () => {
    applyTerminalSearchDemoState('count');
    stepTerminalSearchDemo('next');
    expect(terminalSearchDemoResults.value).toEqual({ resultIndex: 3, resultCount: 12 });
    for (let step = 0; step < 4; step += 1) stepTerminalSearchDemo('previous');
    expect(terminalSearchDemoResults.value).toEqual({ resultIndex: 11, resultCount: 12 });
    stepTerminalSearchDemo('next');
    expect(terminalSearchDemoResults.value).toEqual({ resultIndex: 0, resultCount: 12 });
    // From the highlight limit's "no active match", Previous wraps to the last counted match.
    applyTerminalSearchDemoState('limit');
    stepTerminalSearchDemo('previous');
    expect(terminalSearchDemoResults.value).toEqual({ resultIndex: 999, resultCount: 1000 });
    applyTerminalSearchDemoState('none');
    stepTerminalSearchDemo('next');
    expect(terminalSearchDemoResults.value).toEqual({ resultIndex: -1, resultCount: 0 });
    // A state's fixture is copied, so stepping never alters the next application of that state.
    applyTerminalSearchDemoState('count');
    expect(terminalSearchDemoResults.value).toEqual({ resultIndex: 2, resultCount: 12 });
    resetTerminalDrawerDemo();
  });
});
