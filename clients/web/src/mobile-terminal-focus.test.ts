import { describe, expect, it } from 'vitest';

import {
  INACTIVE_MOBILE_TERMINAL_FOCUS,
  mobileTerminalViewport,
  mobileVirtualKeyboardVisible,
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

  it('HS2-7HPXCY opens, keeps, and ends the full-screen search bar through every focus transition', () => {
    const focus = (terminalId: string, search?: boolean) =>
      ({ type: 'terminal-focus', mobile: true, terminalId, viewport: viewport(844), search }) as const;
    // Search opens nothing outside full screen.
    expect(transitionMobileTerminalFocus(INACTIVE_MOBILE_TERMINAL_FOCUS, { type: 'search', open: true })).toBe(
      INACTIVE_MOBILE_TERMINAL_FOCUS,
    );
    // Search from the rail enters full screen with the bar open.
    let state = transitionMobileTerminalFocus(INACTIVE_MOBILE_TERMINAL_FOCUS, focus('one', true));
    expect(state).toEqual({ active: true, terminalId: 'one', viewport: viewport(844), search: true });
    // Tapping into the same terminal keeps the bar; a keyboard resize keeps it too.
    state = transitionMobileTerminalFocus(state, focus('one'));
    expect(state.search).toBe(true);
    state = transitionMobileTerminalFocus(state, { type: 'viewport-change', viewport: viewport(492, 18) });
    expect(state.search).toBe(true);
    // Repeating an open is idempotent.
    expect(transitionMobileTerminalFocus(state, { type: 'search', open: true })).toBe(state);
    // Closing the bar stays in full screen; closing again is idempotent.
    state = transitionMobileTerminalFocus(state, { type: 'search', open: false });
    expect(state).toEqual({ active: true, terminalId: 'one', viewport: viewport(492, 18) });
    expect(transitionMobileTerminalFocus(state, { type: 'search', open: false })).toBe(state);
    // The focus-mode Search button reopens it.
    state = transitionMobileTerminalFocus(state, { type: 'search', open: true });
    expect(state.search).toBe(true);
    // Another terminal starts without the bar.
    expect(transitionMobileTerminalFocus(state, focus('two')).search).toBeUndefined();
    // Exit ends it, and the next tap-to-focus starts without it.
    state = transitionMobileTerminalFocus(state, { type: 'exit' });
    expect(state).toBe(INACTIVE_MOBILE_TERMINAL_FOCUS);
    expect(transitionMobileTerminalFocus(state, focus('one')).search).toBeUndefined();
    // A desktop-width focus never opens it.
    expect(
      transitionMobileTerminalFocus(INACTIVE_MOBILE_TERMINAL_FOCUS, { ...focus('one', true), mobile: false }),
    ).toBe(INACTIVE_MOBILE_TERMINAL_FOCUS);
  });
});

describe('mobile virtual keyboard detection (HS2-WMN626)', () => {
  it('detects a keyboard covering the layout viewport, ignoring small chrome changes', () => {
    expect(mobileVirtualKeyboardVisible(undefined, 844)).toBe(false);
    expect(mobileVirtualKeyboardVisible({ height: 844, scale: 1 }, 844)).toBe(false);
    expect(mobileVirtualKeyboardVisible({ height: 780, scale: 1 }, 844)).toBe(false);
    expect(mobileVirtualKeyboardVisible({ height: 500, scale: 1 }, 844)).toBe(true);
    expect(mobileVirtualKeyboardVisible({ height: 844, scale: 1 }, 844)).toBe(false);
  });

  it('does not mistake pinch zoom for a keyboard', () => {
    expect(mobileVirtualKeyboardVisible({ height: 422, scale: 2 }, 844)).toBe(false);
    expect(mobileVirtualKeyboardVisible({ height: 250, scale: 2 }, 844)).toBe(true);
    expect(mobileVirtualKeyboardVisible({ height: 500, scale: 0 }, 844)).toBe(true);
  });
});
