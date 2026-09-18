import { describe, expect, it } from 'vitest';

import {
  closeMobileOverlay,
  isMobileViewport,
  MOBILE_BREAKPOINT,
  MOBILE_OVERLAYS_CLOSED,
  openMobileOverlay,
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
