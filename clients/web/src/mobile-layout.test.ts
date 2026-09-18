import { describe, expect, it } from 'vitest';

import {
  closeMobileOverlay,
  isMobileViewport,
  MOBILE_BREAKPOINT,
  MOBILE_OVERLAYS_CLOSED,
  openMobileOverlay,
  shouldAutoOpenInspectorOnTap,
  toggleMobileSidebar,
} from './mobile-layout';

describe('mobile layout', () => {
  it('treats widths below the breakpoint as mobile', () => {
    expect(isMobileViewport(390)).toBe(true);
    expect(isMobileViewport(1023)).toBe(true);
    expect(isMobileViewport(MOBILE_BREAKPOINT)).toBe(false);
    expect(isMobileViewport(1280)).toBe(false);
  });

  it('opens exactly one overlay at a time', () => {
    expect(openMobileOverlay('sidebar')).toEqual({ sidebar: true, inspector: false });
    expect(openMobileOverlay('inspector')).toEqual({ sidebar: false, inspector: true });
  });

  it('toggles the sidebar and closes the inspector when opening it', () => {
    // Opening the sidebar from an open inspector switches to the sidebar (one at a time).
    expect(toggleMobileSidebar({ sidebar: false, inspector: true })).toEqual({ sidebar: true, inspector: false });
    // Toggling an open sidebar closes everything.
    expect(toggleMobileSidebar({ sidebar: true, inspector: false })).toEqual(MOBILE_OVERLAYS_CLOSED);
  });

  it('closes a single overlay without opening the other', () => {
    expect(closeMobileOverlay({ sidebar: false, inspector: true }, 'inspector')).toEqual(MOBILE_OVERLAYS_CLOSED);
    expect(closeMobileOverlay({ sidebar: true, inspector: false }, 'sidebar')).toEqual(MOBILE_OVERLAYS_CLOSED);
  });

  it('auto-opens the inspector only on a plain mobile workspace tap (HS2-N7RPFP)', () => {
    const base = { mobile: true, rail: false, shiftKey: false, metaKey: false, ctrlKey: false };
    expect(shouldAutoOpenInspectorOnTap(base)).toBe(true);
    expect(shouldAutoOpenInspectorOnTap({ ...base, mobile: false })).toBe(false); // desktop keeps the side inspector
    expect(shouldAutoOpenInspectorOnTap({ ...base, rail: true })).toBe(false); // terminal ticket rail
    expect(shouldAutoOpenInspectorOnTap({ ...base, shiftKey: true })).toBe(false); // range multi-select
    expect(shouldAutoOpenInspectorOnTap({ ...base, metaKey: true })).toBe(false); // toggle multi-select
    expect(shouldAutoOpenInspectorOnTap({ ...base, ctrlKey: true })).toBe(false);
  });

  it('walks a realistic open → switch → dismiss sequence', () => {
    let state = MOBILE_OVERLAYS_CLOSED;
    state = toggleMobileSidebar(state);
    expect(state).toEqual({ sidebar: true, inspector: false });
    state = openMobileOverlay('inspector'); // switching (e.g. tapping a ticket) closes the sidebar
    expect(state).toEqual({ sidebar: false, inspector: true });
    state = closeMobileOverlay(state, 'inspector'); // scrim dismiss
    expect(state).toEqual(MOBILE_OVERLAYS_CLOSED);
  });
});
