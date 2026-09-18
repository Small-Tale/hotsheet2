/** Mobile single-column layout state (HS2-ZK51WP).
 *
 * Below the desktop size floor the project sidebar and ticket inspector stop taking horizontal
 * space and overlay the single main column instead. Only one overlay is ever open at a time and a
 * click-away scrim dismisses whichever is open. These pure helpers own the breakpoint and the
 * mutually-exclusive open/close transitions so the rule is unit-testable and the app shell just
 * projects the resulting state. */

/** Viewport width (px) at and above which the desktop side-by-side layout is used. Matches the old
 * app-shell `min-width` floor so there is no broken sub-floor desktop dead zone. */
export const MOBILE_BREAKPOINT = 1024;

/** Whether a viewport width should use the mobile single-column layout. */
export function isMobileViewport(width: number): boolean {
  return width < MOBILE_BREAKPOINT;
}

/** Which overlay panels are currently open in the mobile layout. At most one is ever `true`. */
export interface MobileOverlayState {
  sidebar: boolean;
  inspector: boolean;
}

/** Both overlays closed — the default single-column state. */
export const MOBILE_OVERLAYS_CLOSED: MobileOverlayState = { sidebar: false, inspector: false };

/** Open exactly one overlay, closing the other (one-at-a-time). */
export function openMobileOverlay(which: keyof MobileOverlayState): MobileOverlayState {
  return { sidebar: which === 'sidebar', inspector: which === 'inspector' };
}

/** Toggle the sidebar overlay: open it (closing the inspector) if closed, otherwise close it. */
export function toggleMobileSidebar(state: MobileOverlayState): MobileOverlayState {
  return state.sidebar ? MOBILE_OVERLAYS_CLOSED : openMobileOverlay('sidebar');
}

/** Close a single overlay, leaving the other untouched (it is already closed under the
 * one-at-a-time rule, but this keeps the close path independent of that invariant). */
export function closeMobileOverlay(state: MobileOverlayState, which: keyof MobileOverlayState): MobileOverlayState {
  return { ...state, [which]: false };
}

/** Whether tapping a ticket row should auto-open the inspector overlay (HS2-N7RPFP). Only on mobile,
 * only for a plain tap in the main workspace list — a range/toggle multi-select tap or a tap inside
 * the terminal ticket rail must not hijack the inspector. */
export function shouldAutoOpenInspectorOnTap(input: {
  mobile: boolean;
  rail: boolean;
  shiftKey: boolean;
  metaKey: boolean;
  ctrlKey: boolean;
}): boolean {
  return input.mobile && !input.rail && !input.shiftKey && !input.metaKey && !input.ctrlKey;
}
