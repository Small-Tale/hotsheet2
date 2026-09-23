import { describe, expect, it } from 'vitest';

import {
  INACTIVE_MOBILE_TERMINAL_FOCUS,
  mobileTerminalViewport,
  transitionMobileTerminalFocus,
} from './mobile-terminal-focus';

const viewport = (height: number, top = 0) => ({ left: 0, top, width: 390, height });

describe('mobile terminal focus mode', () => {
  it('uses VisualViewport geometry and a bounded layout-viewport fallback', () => {
    expect(
      mobileTerminalViewport(
        { offsetLeft: 4, offsetTop: 18, width: 382, height: 492 },
        { innerWidth: 390, innerHeight: 844 },
      ),
    ).toEqual({ left: 4, top: 18, width: 382, height: 492 });
    expect(mobileTerminalViewport(undefined, { innerWidth: 0, innerHeight: -1 })).toEqual({
      left: 0,
      top: 0,
      width: 1,
      height: 1,
    });
  });

  it('walks mobile, keyboard, exit, stale resize, refill, and desktop transitions', () => {
    let state = INACTIVE_MOBILE_TERMINAL_FOCUS;
    state = transitionMobileTerminalFocus(state, {
      type: 'terminal-focus',
      mobile: true,
      terminalId: 'one',
      viewport: viewport(844),
    });
    expect(state).toEqual({ active: true, terminalId: 'one', viewport: viewport(844) });
    state = transitionMobileTerminalFocus(state, { type: 'viewport-change', viewport: viewport(492, 18) });
    expect(state.viewport).toEqual(viewport(492, 18));
    state = transitionMobileTerminalFocus(state, { type: 'exit' });
    expect(state).toBe(INACTIVE_MOBILE_TERMINAL_FOCUS);
    state = transitionMobileTerminalFocus(state, { type: 'viewport-change', viewport: viewport(300) });
    expect(state).toBe(INACTIVE_MOBILE_TERMINAL_FOCUS);
    state = transitionMobileTerminalFocus(state, {
      type: 'terminal-focus',
      mobile: true,
      terminalId: 'two',
      viewport: viewport(510),
    });
    expect(state.terminalId).toBe('two');
    state = transitionMobileTerminalFocus(state, {
      type: 'terminal-focus',
      mobile: false,
      terminalId: 'two',
      viewport: viewport(900),
    });
    expect(state).toBe(INACTIVE_MOBILE_TERMINAL_FOCUS);
  });

  it('makes repeated invalidation and exit idempotent while a newer focus replaces the owner', () => {
    const first = transitionMobileTerminalFocus(INACTIVE_MOBILE_TERMINAL_FOCUS, {
        type: 'terminal-focus',
        mobile: true,
        terminalId: 'one',
        viewport: viewport(844),
      }),
      second = transitionMobileTerminalFocus(first, {
        type: 'terminal-focus',
        mobile: true,
        terminalId: 'two',
        viewport: viewport(500),
      }),
      invalid = transitionMobileTerminalFocus(second, { type: 'context-invalid' });
    expect(second).toEqual({ active: true, terminalId: 'two', viewport: viewport(500) });
    expect(transitionMobileTerminalFocus(invalid, { type: 'context-invalid' })).toBe(invalid);
    expect(transitionMobileTerminalFocus(invalid, { type: 'exit' })).toBe(invalid);
  });
});
