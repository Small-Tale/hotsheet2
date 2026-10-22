import { signal } from 'kerfjs';
import { afterEach, beforeEach, describe, expect, it, vi } from 'vitest';

import { MOBILE_TERMINAL_COLUMNS_CHANGE_EVENT } from '../mobile-terminal-columns';
import { INACTIVE_MOBILE_TERMINAL_FOCUS } from '../mobile-terminal-focus';
import { NO_TERMINAL_MODIFIERS } from '../terminal-keys';
import { TERMINAL_DRAWER_RESIZE_END_EVENT } from '../terminal-viewport';
import { createTerminalDrawerGeometryController, currentMobileViewportGeometry } from './terminal-drawer-geometry';

const stored = new Map<string, string>();
let visualViewport: { offsetLeft: number; offsetTop: number; width: number; height: number; scale: number };
const events: string[] = [];

beforeEach(() => {
  stored.clear();
  events.length = 0;
  visualViewport = { offsetLeft: 0, offsetTop: 0, width: 390, height: 844, scale: 1 };
  vi.stubGlobal('localStorage', {
    getItem: (key: string) => stored.get(key) ?? null,
    setItem: (key: string, value: string) => stored.set(key, value),
  });
  vi.stubGlobal('window', {
    get visualViewport() {
      return visualViewport;
    },
    innerWidth: 390,
    innerHeight: 844,
    dispatchEvent: (event: Event) => events.push(event.type),
  });
  vi.stubGlobal('requestAnimationFrame', (callback: FrameRequestCallback) => {
    callback(0);
    return 1;
  });
  vi.stubGlobal('document', { querySelector: () => null });
});
afterEach(() => {
  vi.unstubAllGlobals();
});

function owner(mobile = true) {
  const state = {
    terminalDrawerMin: signal(120),
    terminalDrawerMax: signal(520),
    terminalDrawerSize: signal(320),
    terminalDrawerMaximized: signal(false),
    mobileTerminalFocus: signal(INACTIVE_MOBILE_TERMINAL_FOCUS),
    mobileTerminalColumns: signal(80),
    mobileViewportGeometry: signal(currentMobileViewportGeometry()),
    terminalModifiers: signal(NO_TERMINAL_MODIFIERS),
    terminalFunctionRow: signal(false),
  };
  const isMobile = signal(mobile),
    showToast = vi.fn(),
    updateTerminalDrawerBounds = vi.fn(),
    setDrawerSize = vi.fn((size: number) => {
      state.terminalDrawerSize.value = size;
    });
  const controller = createTerminalDrawerGeometryController({
    ...state,
    viewportMobile: () => isMobile.value,
    updateTerminalDrawerBounds,
    setDrawerSize,
    showToast,
  });
  return { ...state, isMobile, showToast, updateTerminalDrawerBounds, setDrawerSize, controller };
}

describe('terminal drawer geometry owner (HS2-T1DX2V)', () => {
  it('walks phone focus mode through enter, viewport change, exit, and a desktop focus', () => {
    const { controller, mobileTerminalFocus, isMobile } = owner();
    controller.syncMobileTerminalViewport();
    expect(mobileTerminalFocus.value).toBe(INACTIVE_MOBILE_TERMINAL_FOCUS);
    controller.enterMobileTerminalFocus('t1');
    expect(mobileTerminalFocus.value).toMatchObject({ active: true, terminalId: 't1', viewport: { height: 844 } });
    visualViewport = { ...visualViewport, height: 500, offsetTop: 40 };
    controller.syncMobileTerminalViewport();
    expect(mobileTerminalFocus.value.viewport).toEqual({ left: 0, top: 40, width: 390, height: 500 });
    controller.exitMobileTerminalFocus();
    expect(mobileTerminalFocus.value).toBe(INACTIVE_MOBILE_TERMINAL_FOCUS);
    controller.exitMobileTerminalFocus();
    expect(mobileTerminalFocus.value).toBe(INACTIVE_MOBILE_TERMINAL_FOCUS);
    controller.enterMobileTerminalFocus('t2');
    isMobile.value = false;
    controller.enterMobileTerminalFocus('t2');
    expect(mobileTerminalFocus.value).toBe(INACTIVE_MOBILE_TERMINAL_FOCUS);
  });

  it('publishes viewport geometry only when it changes and reports the keyboard', () => {
    const { controller, mobileViewportGeometry } = owner();
    const before = mobileViewportGeometry.value;
    controller.syncMobileViewportGeometry();
    expect(mobileViewportGeometry.value).toBe(before);
    visualViewport = { ...visualViewport, height: 500 };
    controller.syncMobileViewportGeometry();
    expect(mobileViewportGeometry.value).toMatchObject({ keyboardVisible: true, viewport: { height: 500 } });
  });

  it('cycles the magnified column count, persists it, and announces it', () => {
    const { controller, mobileTerminalColumns, showToast } = owner();
    controller.cycleMobileTerminalColumns();
    expect(mobileTerminalColumns.value).not.toBe(80);
    expect(events).toContain(MOBILE_TERMINAL_COLUMNS_CHANGE_EVENT);
    expect(showToast).toHaveBeenCalledWith(`Terminal text size: ${mobileTerminalColumns.value} columns`);
    expect(controller.mobileMagnifiedTerminal()).toMatchObject({ columns: mobileTerminalColumns.value });
  });

  it('exposes the magnified terminal only on phones', () => {
    const { controller, isMobile } = owner();
    expect(controller.mobileMagnifiedTerminal()).toMatchObject({
      columns: 80,
      keyBar: { modifiers: NO_TERMINAL_MODIFIERS, functionRow: false },
    });
    isMobile.value = false;
    expect(controller.mobileMagnifiedTerminal()).toBeUndefined();
  });

  it('toggles maximize and settles drawer geometry with a resize-end event', () => {
    const { controller, terminalDrawerMaximized } = owner(false);
    controller.toggleTerminalDrawerMaximized();
    expect(terminalDrawerMaximized.value).toBe(true);
    expect(events).toEqual([TERMINAL_DRAWER_RESIZE_END_EVENT]);
    controller.toggleTerminalDrawerMaximized();
    expect(terminalDrawerMaximized.value).toBe(false);
    // Without a mounted shell the maximum stays put.
    controller.syncTerminalDrawerMaximum();
  });

  it('restores a manually max-height drawer to its default before the next maximize', () => {
    const { controller, terminalDrawerSize, terminalDrawerMaximized, setDrawerSize } = owner(false);
    terminalDrawerSize.value = 520;
    controller.toggleTerminalDrawerMaximized();
    expect(setDrawerSize).toHaveBeenCalledWith(320);
    expect(terminalDrawerSize.value).toBe(320);
    expect(terminalDrawerMaximized.value).toBe(false);
    controller.toggleTerminalDrawerMaximized();
    expect(terminalDrawerMaximized.value).toBe(true);
    controller.toggleTerminalDrawerMaximized();
    expect(terminalDrawerMaximized.value).toBe(false);
    expect(terminalDrawerSize.value).toBe(320);
    expect(events).toEqual(Array(3).fill(TERMINAL_DRAWER_RESIZE_END_EVENT));
  });
});
