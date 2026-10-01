/** Mobile single-column layout rules (HS2-ZK51WP).
 *
 * Below the desktop size floor the project sidebar and ticket inspector stop taking horizontal
 * space and overlay the single main column instead. The overlay state itself is each Workbench
 * rail's `collapsed` signal, which Kerf's `wireWorkbench` keeps exclusive and dismisses on Escape or
 * an outside press (HS2-Y1B1Y1); these pure helpers own the breakpoint and the tap policies around it
 * so they stay unit-testable. */

/** Viewport width (px) at and above which the desktop side-by-side layout is used. Matches the old
 * app-shell `min-width` floor so there is no broken sub-floor desktop dead zone. */
export const MOBILE_BREAKPOINT = 1024;

/** Whether a viewport width should use the mobile single-column layout. */
export function isMobileViewport(width: number): boolean {
  return width < MOBILE_BREAKPOINT;
}

/**
 * Whether the app may focus a terminal or drawer input on its own (open, connect, refit, drawer resize).
 * In the mobile layout focusing raises the on-screen keyboard and puts a terminal in focus mode, so only
 * the user's own tap focuses there (HS2-YD7RZ7).
 */
export function automaticInputFocusAllowed(width: number): boolean {
  return !isMobileViewport(width);
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
